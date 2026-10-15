// Copyright 2024 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! # This module provides a safe Rust wrapper for the libslirp library.

//! It allows to embed a virtual network stack within your Rust applications.
//!
//! ## Features
//!
//! * **Safe API:**  Wraps the libslirp C API in a safe and idiomatic Rust
//!   interface.
//! * **Networking:**  Provides functionality for virtual networking, including
//!   TCP/IP, UDP, and ICMP.
//! * **Proxy Support:**  Allows integration with proxy managers for handling
//!   external connections.
//! * **Threading:**  Handles communication between the Rust application and the
//!   libslirp event loop.
//!
//! ## Usage
//!
//!
//! ## Example with Proxy
//!
//!
//! This module abstracts away the complexities of interacting with the libslirp
//! C library, providing a more convenient and reliable way to use it in your
//! Rust projects.

use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::io::AsRawFd;
#[cfg(target_os = "windows")]
use std::os::windows::io::AsRawSocket;
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{CStr, c_char, c_int, c_void},
    io::{Read, Write},
    mem::ManuallyDrop,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    rc::Rc,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use bytes::Bytes;
use slirp::{ProxyConnect, ProxyManager};
use tracing::{debug, info, warn};

use crate::{
    libslirp_config,
    libslirp_config::SlirpConfigs,
    libslirp_sys::{
        self, SLIRP_POLL_ERR, SLIRP_POLL_HUP, SLIRP_POLL_IN, SLIRP_POLL_OUT, SLIRP_POLL_PRI,
        SlirpPollType, SlirpProxyConnectFunc, SlirpTimerId,
    },
};

type TimerOpaque = usize;

const TIMEOUT_SECS: u64 = 1;

struct TimerManager {
    clock: RefCell<Instant>,
    map: RefCell<HashMap<TimerOpaque, Timer>>,
    timers: AtomicUsize,
}

#[derive(Clone)]
struct Timer {
    id: SlirpTimerId,
    cb_opaque: usize,
    expire_time: u64,
}

/// The operations performed on the slirp thread
#[derive(Debug)]
enum SlirpCmd {
    Input(Bytes),
    PollResult(Vec<PollFd>, c_int),
    TimerModified,
    Shutdown,
    ProxyConnect(SlirpProxyConnectFunc, usize, i32, i32),
    Notify,
}

/// Trait to allow abstracting the mechanism of sending packets out of Slirp.
pub trait PacketSender: Send {
    /// Sends a packet.
    fn send(&self, packet: Bytes);
}

impl PacketSender for mpsc::Sender<Bytes> {
    fn send(&self, packet: Bytes) {
        let _ = self.send(packet);
    }
}

impl PacketSender for tokio::sync::mpsc::UnboundedSender<Bytes> {
    fn send(&self, packet: Bytes) {
        let _ = self.send(packet);
    }
}

struct CallbackContext {
    tx_bytes: Box<dyn PacketSender>,
    tx_cmds: mpsc::Sender<SlirpCmd>,
    poll_fds: Rc<RefCell<Vec<PollFd>>>,
    proxy_manager: Option<Box<dyn ProxyManager>>,
    tx_proxy_bytes: Option<mpsc::Sender<Bytes>>,
    timer_manager: Rc<TimerManager>,
}

/// A poll thread request has a poll_fds and a timeout
type PollRequest = (Vec<PollFd>, u32);

/// API to LibSlirp
pub struct LibSlirp {
    tx_cmds: mpsc::Sender<SlirpCmd>,
    slirp_thread_handle: Option<thread::JoinHandle<()>>,
    poll_thread_handle: Option<thread::JoinHandle<()>>,
}

impl TimerManager {
    fn next_timer(&self) -> TimerOpaque {
        self.timers.fetch_add(1, Ordering::SeqCst) as TimerOpaque
    }

    /// Finds expired Timers, clears then clones them
    fn collect_expired(&self) -> Vec<Timer> {
        let now_ms = self.get_elapsed().as_millis() as u64;
        self.map
            .borrow_mut()
            .iter_mut()
            .filter(|(_, timer)| timer.expire_time < now_ms)
            .map(|(_, &mut ref mut timer)| {
                timer.expire_time = u64::MAX;
                timer.clone()
            })
            .collect()
    }

    /// Return the minimum duration until the next timer
    fn min_duration(&self) -> Duration {
        match self.map.borrow().iter().min_by_key(|(_, timer)| timer.expire_time) {
            Some((_, timer)) => {
                let now_ms = self.get_elapsed().as_millis() as u64;
                // Duration is >= 0
                Duration::from_millis(timer.expire_time.saturating_sub(now_ms))
            }
            None => Duration::from_millis(u64::MAX),
        }
    }

    fn get_elapsed(&self) -> Duration {
        self.clock.borrow().elapsed()
    }

    fn remove(&self, timer_key: &TimerOpaque) -> Option<Timer> {
        self.map.borrow_mut().remove(timer_key)
    }

    fn insert(&self, timer_key: TimerOpaque, value: Timer) {
        self.map.borrow_mut().insert(timer_key, value);
    }

    fn timer_mod(&self, timer_key: &TimerOpaque, expire_time: u64) {
        if let Some(&mut ref mut timer) = self.map.borrow_mut().get_mut(timer_key) {
            // expire_time is >= 0
            timer.expire_time = expire_time;
        } else {
            warn!("Unknown timer {timer_key}");
        }
    }
}

// Create and return reader and writer TcpStreams of a connected pipe
fn create_pipe() -> std::io::Result<(TcpStream, TcpStream)> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .or_else(|_| TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, 0))))?;
    let addr = listener.local_addr()?;

    let writer_stream = TcpStream::connect(addr)?;
    let (reader_stream, _) = listener.accept()?;

    reader_stream.set_nonblocking(true)?;
    writer_stream.set_nonblocking(true)?;
    Ok((reader_stream, writer_stream))
}

impl LibSlirp {
    /// Creates a new `LibSlirp` instance.
    pub fn new(
        config: libslirp_config::SlirpConfig,
        tx_bytes: Box<dyn PacketSender>,
        proxy_manager: Option<Box<dyn ProxyManager>>,
        tx_proxy_bytes: Option<mpsc::Sender<Bytes>>,
    ) -> LibSlirp {
        let (tx_cmds, rx_cmds) = mpsc::channel::<SlirpCmd>();
        let (tx_poll, rx_poll) = mpsc::channel::<PollRequest>();
        let (signal_rx, signal_tx) = create_pipe().expect("failed to create signal pipe");

        // Create channels for polling thread and launch
        let tx_cmds_poll = tx_cmds.clone();
        let poll_thread_handle =
            match thread::Builder::new().name("slirp_poll".to_string()).spawn(move || {
                slirp_poll_thread(rx_poll, tx_cmds_poll, signal_rx);
            }) {
                Ok(handle) => Some(handle),
                Err(e) => {
                    warn!("Failed to start slirp poll thread: {}", e);
                    None
                }
            };

        let tx_cmds_slirp = tx_cmds.clone();
        let slirp_thread_handle =
            match thread::Builder::new().name("slirp".to_string()).spawn(move || {
                slirp_thread(
                    config,
                    tx_bytes,
                    tx_cmds_slirp,
                    rx_cmds,
                    tx_poll,
                    proxy_manager,
                    tx_proxy_bytes,
                    signal_tx,
                )
            }) {
                Ok(handle) => Some(handle),
                Err(e) => {
                    warn!("Failed to start slirp poll thread: {}", e);
                    None
                }
            };

        LibSlirp { tx_cmds, slirp_thread_handle, poll_thread_handle }
    }

    /// Shuts down the `LibSlirp` instance.
    pub fn shutdown(mut self) {
        if let Err(e) = self.tx_cmds.send(SlirpCmd::Shutdown) {
            warn!("Failed to send Shutdown cmd: {}", e);
        }
        if let Some(handle) = self.slirp_thread_handle.take()
            && let Err(e) = handle.join()
        {
            warn!("Failed to join slirp thread: {e:?}");
        }
        if let Some(handle) = self.poll_thread_handle.take()
            && let Err(e) = handle.join()
        {
            warn!("Failed to join poll thread: {e:?}");
        }
    }

    /// Inputs network data into the `LibSlirp` instance.
    pub fn input(&self, bytes: Bytes) {
        if let Err(e) = self.tx_cmds.send(SlirpCmd::Input(bytes)) {
            warn!("Failed to send Input cmd: {}", e);
        }
    }

    /// Returns a cloneable handle for inputting data into LibSlirp.
    pub fn input_handle(&self) -> LibSlirpInputHandle {
        LibSlirpInputHandle { tx_cmds: self.tx_cmds.clone() }
    }
}

/// A cloneable handle for inputting data into LibSlirp.
#[derive(Clone)]
pub struct LibSlirpInputHandle {
    tx_cmds: mpsc::Sender<SlirpCmd>,
}

impl LibSlirpInputHandle {
    pub fn input(&self, bytes: Bytes) {
        if let Err(e) = self.tx_cmds.send(SlirpCmd::Input(bytes)) {
            warn!("Failed to send Input cmd: {}", e);
        }
    }
}

struct ConnectRequest {
    tx_cmds: mpsc::Sender<SlirpCmd>,
    connect_func: SlirpProxyConnectFunc,
    connect_id: usize,
    af: i32,
    start: Instant,
}

impl ProxyConnect for ConnectRequest {
    fn proxy_connect(&self, fd: i32, addr: SocketAddr) {
        // Send it to Slirp after try_connect() completed
        let duration = self.start.elapsed().as_secs();
        if duration > TIMEOUT_SECS {
            warn!(
                "ConnectRequest for connection ID {} to {} took too long: {:?}",
                self.connect_id, addr, duration
            );
        }
        let _ = self.tx_cmds.send(SlirpCmd::ProxyConnect(
            self.connect_func,
            self.connect_id,
            fd,
            self.af,
        ));
    }
}

/// Converts a libslirp callback's `opaque` handle into a
/// `CallbackContext.`
///
/// Wrapped in a `ManuallyDrop` because we do not want to release the
/// storage when the callback returns.
///
/// # Safety
///
/// * `opaque` must be a valid pointer to a `CallbackContext` originally passed
///   to the slirp API.
unsafe fn callback_context_from_raw(opaque: *mut c_void) -> ManuallyDrop<Box<CallbackContext>> {
    ManuallyDrop::new(
        // Safety:
        //
        // * `opaque` is a valid pointer to a `CallbackContext` originally passed to the slirp API.
        //   The `callback_context_from_raw` function itself is marked `unsafe` to enforce this
        //   precondition on its callers.
        unsafe { Box::from_raw(opaque.cast::<CallbackContext>()) },
    )
}

/// A Rust struct for the fields held by `slirp` C library through its
/// lifetime.
///
/// All libslirp C calls are impl on this struct.
struct Slirp {
    slirp: *mut libslirp_sys::Slirp,
    // These fields are held by slirp C library
    _configs: Box<SlirpConfigs>,
    _callbacks: Box<libslirp_sys::SlirpCb>,
    // Passed to API calls and then to callbacks
    callback_context: Box<CallbackContext>,
}

impl Slirp {
    fn new(config: libslirp_config::SlirpConfig, callback_context: Box<CallbackContext>) -> Slirp {
        let callbacks = Box::new(libslirp_sys::SlirpCb {
            send_packet: Some(send_packet_cb),
            guest_error: Some(guest_error_cb),
            clock_get_ns: Some(clock_get_ns_cb),
            timer_new: None,
            timer_free: Some(timer_free_cb),
            timer_mod: Some(timer_mod_cb),
            register_poll_fd: Some(register_poll_fd_cb),
            unregister_poll_fd: Some(unregister_poll_fd_cb),
            notify: Some(notify_cb),
            init_completed: Some(init_completed_cb),
            timer_new_opaque: Some(timer_new_opaque_cb),
            try_connect: Some(try_connect_cb),
            remove: Some(remove_cb),
        });
        let configs = Box::new(SlirpConfigs::new(&config));

        // Call libslrip "C" library to create a new instance of a slirp
        // protocol stack.
        //
        // Safety: We ensure that:
        //
        // * `configs.c_slirp_config` is a valid pointer to the "C" config struct. It is
        //   held by the "C" slirp library for lifetime of the slirp instance.
        //
        // * `callbacks` is a valid pointer to an array of callback functions. It is
        //   held by the "C" slirp library for the lifetime of the slirp instance.
        //
        // * `callback_context` is an arbitrary opaque type passed back to callback
        //   functions by libslirp.
        let slirp = unsafe {
            libslirp_sys::slirp_new(
                configs.c_slirp_config(),
                &*callbacks,
                &*callback_context as *const CallbackContext as *mut c_void,
            )
        };

        Slirp { slirp, _configs: configs, _callbacks: callbacks, callback_context }
    }

    fn handle_timer(&self, timer: Timer) {
        // Safety: We ensure that:
        //
        // * self.slirp is a valid state returned by `slirp_new()`
        //
        // * timer.id is a valid c_uint from "C" slirp library calling
        //   `timer_new_opaque_cb()`
        //
        // * timer.cb_opaque is an usize representing a pointer to callback function
        //   from
        // "C" slirp library calling `timer_new_opaque_cb()`
        unsafe {
            libslirp_sys::slirp_handle_timer(self.slirp, timer.id, timer.cb_opaque as *mut c_void);
        };
    }
}

impl Drop for Slirp {
    /// # Safety
    ///
    /// * self.slirp is always slirp pointer initialized by slirp_new to the
    ///   slirp API.
    fn drop(&mut self) {
        // Safety:
        //
        // * self.slirp is a slirp pointer initialized by slirp_new;
        // it's private to the struct and is only constructed that
        // way.
        unsafe { libslirp_sys::slirp_cleanup(self.slirp) };
    }
}

#[allow(clippy::too_many_arguments)]
fn slirp_thread(
    config: libslirp_config::SlirpConfig,
    tx_bytes: Box<dyn PacketSender>,
    tx_cmds: mpsc::Sender<SlirpCmd>,
    rx: mpsc::Receiver<SlirpCmd>,
    tx_poll: mpsc::Sender<PollRequest>,
    proxy_manager: Option<Box<dyn ProxyManager>>,
    tx_proxy_bytes: Option<mpsc::Sender<Bytes>>,
    mut signal_tx: TcpStream,
) {
    // Data structures wrapped in an RC are referenced through the
    // libslirp callbacks and this code (both in the same thread).

    let timer_manager = Rc::new(TimerManager {
        clock: RefCell::new(Instant::now()),
        map: RefCell::new(HashMap::new()),
        timers: AtomicUsize::new(1),
    });

    let poll_fds = Rc::new(RefCell::new(Vec::new()));

    let callback_context = Box::new(CallbackContext {
        tx_bytes,
        tx_cmds,
        poll_fds: poll_fds.clone(),
        proxy_manager,
        tx_proxy_bytes,
        timer_manager: timer_manager.clone(),
    });

    let slirp = Slirp::new(config, callback_context);

    slirp.pollfds_fill_and_send(&poll_fds, &tx_poll);

    let min_duration = timer_manager.min_duration();
    loop {
        let command = rx.recv_timeout(min_duration);
        let start = Instant::now();

        let cmd_str = format!("{command:?}");
        match command {
            // The dance to tell libslirp which FDs have IO ready
            // starts with a response from a worker thread sending a
            // PollResult, followed by pollfds_poll forwarding the FDs
            // to libslirp, followed by giving the worker thread
            // another set of fds to poll (and block).
            Ok(SlirpCmd::PollResult(poll_fds_result, select_error)) => {
                poll_fds.borrow_mut().clone_from_slice(&poll_fds_result);
                slirp.pollfds_poll(select_error);
                slirp.pollfds_fill_and_send(&poll_fds, &tx_poll);
            }
            Ok(SlirpCmd::Input(bytes)) => slirp.input(&bytes),

            // A timer has been modified, new expired_time value
            Ok(SlirpCmd::TimerModified) => continue,

            // A notification to wake up the poll thread
            Ok(SlirpCmd::Notify) => {
                if let Err(e) = signal_tx.write(&[0]) {
                    warn!("Failed to signal poll thread: {}", e);
                }
                continue;
            }

            // Exit the while loop and shutdown
            Ok(SlirpCmd::Shutdown) => break,

            Ok(SlirpCmd::ProxyConnect(func, connect_id, fd, af)) => match func {
                // Safety: we ensure that func (`SlirpProxyConnectFunc`)
                // and `connect_opaque` are valid because they originated
                // from the libslirp call to `try_connect_cb.`
                //
                // Parameter `fd` will be >= 0 and the descriptor for the
                // active socket to use, `af` will be either AF_INET or
                // AF_INET6. On failure `fd` will be negative.
                Some(func) => unsafe { func(connect_id as *mut c_void, fd as c_int, af as c_int) },
                None => warn!("Proxy connect function not found"),
            },

            // Timeout... process any timers
            Err(mpsc::RecvTimeoutError::Timeout) => continue,

            // Error
            _ => break,
        }

        // Explicitly store expired timers to release lock
        let timers = timer_manager.collect_expired();
        // Handle any expired timers' callback in the slirp thread
        for timer in timers {
            slirp.handle_timer(timer);
        }
        let duration = start.elapsed().as_secs();
        if duration > TIMEOUT_SECS {
            warn!("libslirp command '{cmd_str}' took too long to complete: {duration:?}");
        }
    }
    // Shuts down the instance of a slirp stack and release slirp storage. No
    // callbacks occur after this since it calls slirp_cleanup.
    drop(slirp);

    // Shutdown slirp_poll_thread -- worst case it sends a PollResult that is
    // ignored since this thread is no longer processing Slirp commands.
    drop(tx_poll);
}

#[derive(Clone, Debug)]
struct PollFd {
    fd: c_int,
    events: SlirpPollType,
    revents: SlirpPollType,
}

impl Slirp {
    /// Fill the pollfds from libslirp and pass the request to the polling
    /// thread.
    ///
    /// This is called by the application when it is about to sleep through
    /// poll().  *timeout is set to the amount of virtual time (in ms) that
    /// the application intends to wait (UINT32_MAX if
    /// infinite). slirp_pollfds_fill updates it according to e.g. TCP
    /// timers, so the application knows it should sleep a smaller amount
    /// of time. slirp_pollfds_fill calls add_poll for each file descriptor
    /// that should be monitored along the sleep. The opaque pointer is
    /// passed as such to add_poll, and add_poll returns an index.
    ///
    /// # Safety
    ///
    /// `slirp` must be a valid Slirp state returned by `slirp_new()`
    fn pollfds_fill_and_send(
        &self,
        poll_fds: &RefCell<Vec<PollFd>>,
        tx: &mpsc::Sender<PollRequest>,
    ) {
        let mut timeout: u32 = u32::MAX;
        poll_fds.borrow_mut().clear();

        // Safety: we ensure that:
        //
        // * self.slirp has a slirp pointer initialized by slirp_new,
        // as it's private to the struct and is only constructed that way.
        //
        // * timeout is a valid ptr to a mutable u32.  The "C" slirp
        // library stores into timeout.
        //
        // * slirp_add_poll_cb is a valid `SlirpAddPollCb` function.
        //
        // * self.callback_context is a CallbackContext
        unsafe {
            libslirp_sys::slirp_pollfds_fill(
                self.slirp,
                &mut timeout,
                Some(slirp_add_poll_cb),
                &*self.callback_context as *const CallbackContext as *mut c_void,
            );
        }
        if let Err(e) = tx.send((poll_fds.borrow().to_vec(), timeout)) {
            warn!("Failed to send poll fds: {}", e);
        }
    }
}

/// "C" library callback that is called for each file descriptor that
/// should be monitored.
///
/// # Safety
///
/// * opaque must be a CallbackContext
unsafe extern "C" fn slirp_add_poll_cb(fd: c_int, events: c_int, opaque: *mut c_void) -> c_int {
    // Safety:
    //
    // * opaque is a CallbackContext
    unsafe { callback_context_from_raw(opaque) }.add_poll(fd, events)
}

impl CallbackContext {
    fn add_poll(&mut self, fd: c_int, events: c_int) -> c_int {
        let idx = self.poll_fds.borrow().len();
        self.poll_fds.borrow_mut().push(PollFd { fd, events: events as SlirpPollType, revents: 0 });
        idx as i32
    }
}

impl Slirp {
    /// Pass the result from the polling thread back to libslirp.
    ///
    /// * select_error should be 1 if poll() returned an error, else 0.
    fn pollfds_poll(&self, select_error: c_int) {
        // Call libslrip "C" library to fill poll return event information
        // using slirp_get_revents_cb callback function.
        //
        // Safety: we ensure that:
        //
        // * self.slirp has a slirp pointer initialized by slirp_new,
        // as it's private to the struct and is only constructed that way.
        //
        // * slirp_get_revents_cb is a valid `SlirpGetREventsCb` callback
        // function.
        //
        // * select_error should be 1 if poll() returned an error, else 0.
        //
        // * self.callback_context is a CallbackContext
        unsafe {
            libslirp_sys::slirp_pollfds_poll(
                self.slirp,
                select_error,
                Some(slirp_get_revents_cb),
                &*self.callback_context as *const CallbackContext as *mut c_void,
            );
        }
    }
}

/// "C" library callback that is called on each file descriptor, giving
/// it the index that add_poll returned.
///
/// # Safety
///
/// * opaque must be a CallbackContext
unsafe extern "C" fn slirp_get_revents_cb(idx: c_int, opaque: *mut c_void) -> c_int {
    // Safety:
    //
    // * opaque is a CallbackContext
    unsafe { callback_context_from_raw(opaque) }.get_events(idx)
}

impl CallbackContext {
    fn get_events(&self, idx: c_int) -> c_int {
        if let Some(poll_fd) = self.poll_fds.borrow().get(idx as usize) {
            poll_fd.revents as c_int
        } else {
            0
        }
    }
}

macro_rules! ternary {
    ($cond:expr, $true_expr:expr) => {
        if $cond != 0 { $true_expr } else { 0 }
    };
}

/// Worker thread that performs blocking `poll` operations on file descriptors.
///
/// It receives polling requests from the `rx` channel, performs the `poll`, and
/// sends the results back to the slirp thread via the `tx` channel. This allows
/// the slirp stack to be notified about network events without busy waiting.
///
/// The function handles platform-specific differences in polling mechanisms
/// between Linux/macOS and Windows. It also converts between Slirp's
/// `SlirpPollType` and the OS-specific poll event types.
fn slirp_poll_thread(
    rx: mpsc::Receiver<PollRequest>,
    tx: mpsc::Sender<SlirpCmd>,
    mut signal_rx: TcpStream,
) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    use libc::{
        POLLERR as OS_POLL_ERR, POLLHUP as OS_POLL_HUP, POLLIN as OS_POLL_IN,
        POLLNVAL as OS_POLL_NVAL, POLLOUT as OS_POLL_OUT, POLLPRI as OS_POLL_PRI,
        nfds_t as OsPollFdsLenType, poll, pollfd,
    };
    #[cfg(target_os = "windows")]
    use windows_sys::Win32::Networking::WinSock::{
        POLLERR as OS_POLL_ERR, POLLHUP as OS_POLL_HUP, POLLNVAL as OS_POLL_NVAL,
        POLLRDBAND as OS_POLL_PRI, POLLRDNORM as OS_POLL_IN, POLLWRNORM as OS_POLL_OUT,
        SOCKET as FdType, WSAPOLLFD as pollfd, WSAPoll as poll,
    };
    #[cfg(target_os = "windows")]
    type OsPollFdsLenType = u32;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    type FdType = c_int;

    // Convert Slirp poll (input) events to OS events definitions
    fn to_os_events(events: SlirpPollType) -> i16 {
        ternary!(events & SLIRP_POLL_IN, OS_POLL_IN)
            | ternary!(events & SLIRP_POLL_OUT, OS_POLL_OUT)
            | ternary!(events & SLIRP_POLL_PRI, OS_POLL_PRI)
    }
    // Convert OS (input) "events" to Slirp (input) events definitions
    fn to_slirp_events(events: i16) -> SlirpPollType {
        ternary!(events & OS_POLL_IN, SLIRP_POLL_IN)
            | ternary!(events & OS_POLL_OUT, SLIRP_POLL_OUT)
            | ternary!(events & OS_POLL_PRI, SLIRP_POLL_PRI)
    }
    // Convert OS (output) "revents" to Slirp revents definitions which includes
    // ERR and HUP
    fn to_slirp_revents(revents: i16) -> SlirpPollType {
        to_slirp_events(revents)
            | ternary!(revents & OS_POLL_ERR, SLIRP_POLL_ERR)
            | ternary!(revents & OS_POLL_HUP, SLIRP_POLL_HUP)
    }

    let mut prev_poll_fds_len = 0;
    while let Ok((mut poll_fds, timeout)) = rx.recv() {
        if poll_fds.len() != prev_poll_fds_len {
            prev_poll_fds_len = poll_fds.len();
            debug!("slirp_poll_thread recv poll_fds.len(): {:?}", prev_poll_fds_len);
        }
        #[cfg(not(windows))]
        let signal_fd = signal_rx.as_raw_fd();
        #[cfg(windows)]
        let signal_fd = signal_rx.as_raw_socket() as i32;

        poll_fds.push(PollFd { fd: signal_fd, events: SLIRP_POLL_IN, revents: 0 });
        // Create a c format array with the same size as poll
        let mut os_poll_fds: Vec<pollfd> = Vec::with_capacity(poll_fds.len());
        for fd in &poll_fds {
            os_poll_fds.push(pollfd {
                fd: fd.fd as FdType,
                events: to_os_events(fd.events),
                revents: 0,
            });
        }

        let mut poll_result = 0;
        // WSAPoll requires an array of one or more POLLFD structures.
        // When nfds == 0, WSAPoll returns immediately with result -1, ignoring
        // the timeout. (This is different from poll on Linux/macOS,
        // which will wait for the timeout.) Therefore when nfds == 0 we
        // will explicitly sleep for the timeout regardless of OS.
        if os_poll_fds.is_empty() {
            // If there are no FDs to poll, sleep for the specified timeout.
            thread::sleep(Duration::from_millis(timeout as u64));
        } else {
            // Safety: we ensure that:
            //
            // `os_poll_fds` is a valid ptr to a vector of pollfd which
            // the `poll` system call can write into. Note `os_poll_fds`
            // is created and allocated above.
            poll_result = unsafe {
                poll(
                    os_poll_fds.as_mut_ptr(),
                    os_poll_fds.len() as OsPollFdsLenType,
                    timeout as i32,
                )
            };
        }
        // POLLHUP and POLLERR are always allowed revents.
        // if other events were not requested, then don't return them in the
        // revents.
        let allowed_revents = OS_POLL_HUP | OS_POLL_ERR;
        let mut slirp_poll_fds: Vec<PollFd> = Vec::with_capacity(poll_fds.len());
        for &fd in &os_poll_fds {
            if fd.fd == signal_fd as FdType {
                if (fd.revents & OS_POLL_IN) != 0 {
                    // Drain the signal pipe.
                    let mut buf = [0u8; 64];
                    while let Ok(n) = signal_rx.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                    }
                }
            } else {
                // Slrip does not handle POLLNVAL - print warning and skip
                if fd.events & OS_POLL_NVAL != 0 {
                    warn!("POLLNVAL event - Skip poll for fd: {:?}", fd.fd);
                    continue;
                }
                slirp_poll_fds.push(PollFd {
                    fd: fd.fd as c_int,
                    events: to_slirp_events(fd.events),
                    revents: to_slirp_revents(fd.revents & (fd.events | allowed_revents)),
                });
            }
        }

        // 'select_error' should be 1 if poll() returned an error, else 0.
        if let Err(e) = tx.send(SlirpCmd::PollResult(slirp_poll_fds, (poll_result < 0) as i32)) {
            warn!("Failed to send slirp PollResult cmd: {}", e);
        }
    }
}

impl Slirp {
    /// Sends raw input bytes to the slirp stack.
    ///
    /// This function is called by the application to inject network data into
    /// the virtual network stack. The `bytes` slice contains the raw packet
    /// data that should be processed by slirp.
    fn input(&self, bytes: &[u8]) {
        // Safety: The "C" library ensure that the memory is not
        // referenced after the call and `bytes` does not need to remain
        // valid after the function returns.
        unsafe { libslirp_sys::slirp_input(self.slirp, bytes.as_ptr(), bytes.len() as i32) };
    }
}

/// Callback function invoked by the slirp stack to send an ethernet frame to
/// the guest network.
///
/// This function is called by the slirp stack when it has a network packet that
/// needs to be delivered to the guest network. The `buf` pointer points to the
/// raw packet data, and `len` specifies the length of the packet.
///
/// If the guest is not ready to receive the packet, the function can drop the
/// data. TCP will handle retransmissions as needed.
///
/// # Safety
///
/// * `buf` must be a valid pointer to `len` bytes of memory.
/// * `len` must be greater than 0.
/// * `opaque` must be a valid `CallbackContext` pointer.
///
/// # Returns
///
/// The number of bytes sent (which should be equal to `len`).
unsafe extern "C" fn send_packet_cb(
    buf: *const c_void,
    len: usize,
    opaque: *mut c_void,
) -> libslirp_sys::slirp_ssize_t {
    // Safety:
    //
    // * `buf` is a valid pointer to `len` bytes of memory.
    // * `len` is greater than 0.
    // * `opaque` is a valid `CallbackContext` pointer.
    unsafe { callback_context_from_raw(opaque) }.send_packet(buf, len)
}

impl CallbackContext {
    fn send_packet(&self, buf: *const c_void, len: usize) -> libslirp_sys::slirp_ssize_t {
        // Safety: The caller ensures that `buf` is contains `len` bytes of
        // data.
        let c_slice = unsafe { std::slice::from_raw_parts(buf.cast::<u8>(), len) };
        // Bytes::from(slice: &'static [u8]) creates a Bytes object without
        // copying the data. To own its data, copy &'static [u8] to
        // Vec<u8> before converting to Bytes.
        let bytes = Bytes::from(c_slice.to_vec());
        self.tx_bytes.send(bytes.clone());
        // When HTTP Proxy is enabled, it tracks DNS packets.
        if let Some(tx_proxy) = &self.tx_proxy_bytes {
            let _ = tx_proxy.send(bytes);
        }
        len as libslirp_sys::slirp_ssize_t
    }
}

/// Callback function invoked by the slirp stack to report an error caused by
/// guest misbehavior.
///
/// This function is called by the slirp stack when it encounters an error
/// condition that is attributed to incorrect or unexpected behavior from the
/// guest network. The `msg` parameter contains a human-readable error message
/// describing the issue.
///
/// # Safety
///
/// * `msg` must be a valid C string.
/// * `opaque` must be a valid `CallbackContext` pointer.
unsafe extern "C" fn guest_error_cb(msg: *const c_char, opaque: *mut c_void) {
    // Safety:
    //  * `msg` is guaranteed to be a valid C string by the caller.
    let msg = String::from_utf8_lossy(unsafe { CStr::from_ptr(msg) }.to_bytes());
    // Safety:
    //  * `opaque` is guaranteed to be a valid, non-null pointer to a
    //    `CallbackContext` struct that was originally passed to `slirp_new()` and
    //    is guaranteed to be valid for the lifetime of the Slirp instance.
    //  * `callback_context_from_raw()` safely converts the raw `opaque` pointer
    //    back to a `CallbackContext` reference. This is safe because the `opaque`
    //    pointer is guaranteed to be valid.
    unsafe { callback_context_from_raw(opaque) }.guest_error(msg.to_string());
}

impl CallbackContext {
    fn guest_error(&self, msg: String) {
        warn!("libslirp: {msg}");
    }
}

/// Callback function invoked by the slirp stack to get the current time in
/// nanoseconds.
///
/// This function is called by the slirp stack to obtain the current time, which
/// is used for various timing-related operations within the virtual network
/// stack.
///
/// # Safety
///
/// * `opaque` must be a valid `CallbackContext` pointer.
///
/// # Returns
///
/// The current time in nanoseconds.
unsafe extern "C" fn clock_get_ns_cb(opaque: *mut c_void) -> i64 {
    // Safety:
    //
    // * `opaque` is a valid `CallbackContext` pointer.
    //
    unsafe { callback_context_from_raw(opaque) }.clock_get_ns()
}

impl CallbackContext {
    fn clock_get_ns(&self) -> i64 {
        self.timer_manager.get_elapsed().as_nanos() as i64
    }
}

/// Callback function invoked by the slirp stack to signal that initialization
/// is complete.
///
/// This function is called by the slirp stack once it has finished its
/// initialization process and is ready to handle network traffic.
///
/// # Safety
///
/// * `_slirp` is a raw pointer to the slirp instance, but it's not used in this
///   callback.
/// * `opaque` must be a valid `CallbackContext` pointer.
unsafe extern "C" fn init_completed_cb(_slirp: *mut libslirp_sys::Slirp, opaque: *mut c_void) {
    // Safety:
    //
    // * `_slirp` is a raw pointer to the slirp instance, but it's not used in this
    //   callback.
    // * `opaque` is a valid `CallbackContext` pointer.
    unsafe { callback_context_from_raw(opaque) }.init_completed();
}

impl CallbackContext {
    fn init_completed(&self) {
        info!("libslirp: initialization completed.");
    }
}

/// Callback function invoked by the slirp stack to create a new timer.
///
/// This function is called by the slirp stack when it needs to create a new
/// timer. The `id` parameter is a unique identifier for the timer, and
/// `cb_opaque` is an opaque pointer that will be passed back to the timer
/// callback function when the timer expires.
///
/// # Safety
///
/// * `opaque` must be a valid `CallbackContext` pointer.
/// * `cb_opaque` should be a valid pointer that can be passed back to libslirp.
///
/// # Returns
///
/// An opaque pointer to the newly created timer.
unsafe extern "C" fn timer_new_opaque_cb(
    id: SlirpTimerId,
    cb_opaque: *mut c_void,
    opaque: *mut c_void,
) -> *mut c_void {
    // Safety:
    //  * `opaque` is a valid, non-null pointer to a `CallbackContext` struct that
    //    was originally passed to `slirp_new()` and is guaranteed to be valid for
    //    the lifetime of the Slirp instance.
    //  * `callback_context_from_raw()` safely converts the raw `opaque` pointer
    //    back to a `CallbackContext` reference. This is safe because the `opaque`
    //    pointer is guaranteed to be valid.
    unsafe { callback_context_from_raw(opaque).timer_new_opaque(id, cb_opaque) }
}

impl CallbackContext {
    /// Creates a new timer and stores it in the timer manager.
    ///
    /// # Safety
    ///
    /// * `cb_opaque` should be a valid pointer that can be passed back to
    ///   libslirp.
    unsafe fn timer_new_opaque(&self, id: SlirpTimerId, cb_opaque: *mut c_void) -> *mut c_void {
        let timer = self.timer_manager.next_timer();
        self.timer_manager
            .insert(timer, Timer { expire_time: u64::MAX, id, cb_opaque: cb_opaque as usize });
        timer as *mut c_void
    }
}

/// Callback function invoked by the slirp stack to free a timer.
///
/// This function is called by the slirp stack when a timer is no longer needed
/// and should be removed. The `timer` parameter is an opaque pointer to the
/// timer that was created previously using `timer_new_opaque_cb`.
///
/// # Safety
///
/// * `timer` must be a valid `TimerOpaque` key that was previously returned by
///   `timer_new_opaque_cb`.
/// * `opaque` must be a valid `CallbackContext` pointer.
unsafe extern "C" fn timer_free_cb(timer: *mut c_void, opaque: *mut c_void) {
    // Safety:
    //
    // * `timer` is a valid `TimerOpaque` key that was previously returned by
    //   `timer_new_opaque_cb`.
    // * `opaque` is a valid `CallbackContext` pointer.
    unsafe { callback_context_from_raw(opaque) }.timer_free(timer);
}

impl CallbackContext {
    /// Removes a timer from the timer manager.
    ///
    /// If the timer is not found in the manager, a warning is logged.
    fn timer_free(&self, timer: *mut c_void) {
        let timer = timer as TimerOpaque;
        if self.timer_manager.remove(&timer).is_none() {
            warn!("Unknown timer {timer}");
        }
    }
}

/// Callback function invoked by the slirp stack to modify an existing timer.
///
/// This function is called by the slirp stack when it needs to change the
/// expiration time of an existing timer. The `timer` parameter is an opaque
/// pointer to the timer that was created previously using
/// `timer_new_opaque_cb`. The `expire_time` parameter specifies the new
/// expiration time for the timer, in nanoseconds.
///
/// # Safety
///
/// * `timer` must be a valid `TimerOpaque` key that was previously returned by
///   `timer_new_opaque_cb`.
/// * `opaque` must be a valid `CallbackContext` pointer.
unsafe extern "C" fn timer_mod_cb(timer: *mut c_void, expire_time: i64, opaque: *mut c_void) {
    // Safety:
    //
    // * `timer` is a valid `TimerOpaque` key that was previously returned by
    //   `timer_new_opaque_cb`.
    // * `opaque` is a valid `CallbackContext` pointer.
    unsafe { callback_context_from_raw(opaque) }.timer_mod(timer, expire_time);
}

impl CallbackContext {
    /// Modifies the expiration time of a timer in the timer manager.
    ///
    /// This function updates the expiration time of the specified timer. It
    /// also sends a notification to the slirp command thread to wake it up
    /// and reset its sleep duration,
    fn timer_mod(&self, timer: *mut c_void, expire_time: i64) {
        let timer_key = timer as TimerOpaque;
        let expire_time = std::cmp::max(expire_time, 0) as u64;
        self.timer_manager.timer_mod(&timer_key, expire_time);
        // Wake up slirp command thread to reset sleep duration
        let _ = self.tx_cmds.send(SlirpCmd::TimerModified);
    }

    fn notify(&self) {
        if let Err(e) = self.tx_cmds.send(SlirpCmd::Notify) {
            warn!("Failed to send Notify cmd: {}", e);
        }
    }
}

extern "C" fn register_poll_fd_cb(_fd: c_int, opaque: *mut c_void) {
    // Safety: `opaque` is a valid `CallbackContext` pointer.
    unsafe { callback_context_from_raw(opaque) }.notify();
}

extern "C" fn unregister_poll_fd_cb(_fd: c_int, opaque: *mut c_void) {
    // Safety: `opaque` is a valid `CallbackContext` pointer.
    unsafe { callback_context_from_raw(opaque) }.notify();
}

extern "C" fn notify_cb(opaque: *mut c_void) {
    // Safety:
    //
    // * `opaque` is a valid `CallbackContext` pointer.
    unsafe { callback_context_from_raw(opaque) }.notify()
}

/// Callback function invoked by the slirp stack to initiate a proxy connection.
///
/// This function is called by the slirp stack when it needs to establish a
/// connection through a proxy. The `addr` parameter points to the address to
/// connect to, `connect_func` is a callback function that should be called to
/// notify libslirp of the connection result, and `connect_opaque` is an opaque
/// pointer that will be passed back to `connect_func`.
///
/// # Safety
///
/// * `addr` must be a valid pointer to a `sockaddr_storage` structure.
/// * `connect_func` must be a valid callback function pointer.
/// * `connect_opaque` should be a valid pointer that can be passed back to
///   libslirp.
/// * `opaque` must be a valid `CallbackContext` pointer.
///
/// # Returns
///
/// `true` if the proxy connection request was initiated successfully, `false`
/// otherwise.
unsafe extern "C" fn try_connect_cb(
    addr: *const libslirp_sys::sockaddr_storage,
    connect_func: SlirpProxyConnectFunc,
    connect_opaque: *mut c_void,
    opaque: *mut c_void,
) -> bool {
    // Safety:
    //
    // * `addr` is a valid pointer to a `sockaddr_storage` structure.
    // * `connect_func` is a valid callback function pointer.
    // * `connect_opaque` is a valid pointer that can be passed back to libslirp.
    // * `opaque` is a valid `CallbackContext` pointer.
    unsafe {
        callback_context_from_raw(opaque).try_connect(addr, connect_func, connect_opaque as usize)
    }
}

impl CallbackContext {
    /// Attempts to establish a proxy connection.
    ///
    /// This function uses the `proxy_manager` to initiate a connection to the
    /// specified address. If the proxy manager is not available, it returns
    /// `false`.
    ///
    /// # Safety
    ///
    /// * `addr` must be a valid pointer to a `sockaddr_storage` structure.
    unsafe fn try_connect(
        &self,
        addr: *const libslirp_sys::sockaddr_storage,
        connect_func: SlirpProxyConnectFunc,
        connect_id: usize,
    ) -> bool {
        if let Some(proxy_manager) = &self.proxy_manager {
            // Safety:
            //
            //  * `addr` is a valid pointer to a `sockaddr_storage` structure, as guaranteed
            //    by the caller
            //  * Obtaining the `ss_family` field from a valid `sockaddr_storage` struct is
            //    safe
            let storage = unsafe { *addr };
            let af = storage.ss_family as i32;
            let Ok(socket_addr) = SocketAddr::try_from(storage) else {
                warn!("Unsupported address family: {}", storage.ss_family);
                return false;
            };
            proxy_manager.try_connect(
                socket_addr,
                connect_id,
                Box::new(ConnectRequest {
                    tx_cmds: self.tx_cmds.clone(),
                    connect_func,
                    connect_id,
                    af,
                    start: Instant::now(),
                }),
            )
        } else {
            false
        }
    }
}

/// Callback function invoked by the slirp stack to remove a proxy connection.
///
/// This function is called by the slirp stack when a proxy connection is no
/// longer needed and should be removed. The `connect_opaque` parameter is an
/// opaque pointer that was originally passed to `try_connect_cb` when the
/// connection was initiated.
///
/// # Safety
///
/// * `connect_opaque` must be a valid pointer that was previously passed to
///   `try_connect_cb`.
/// * `opaque` must be a valid `CallbackContext` pointer.
unsafe extern "C" fn remove_cb(connect_opaque: *mut c_void, opaque: *mut c_void) {
    //  Safety:
    //
    // * `connect_opaque` is a valid pointer that was previously passed to
    //   `try_connect_cb`.
    // * `opaque` is a valid `CallbackContext` pointer.
    unsafe { callback_context_from_raw(opaque) }.remove(connect_opaque as usize);
}

impl CallbackContext {
    /// Removes a proxy connection from the proxy manager.
    ///
    /// This function calls the `remove` method on the `proxy_manager` to remove
    /// the connection associated with the given `connect_id`.
    fn remove(&self, connect_id: usize) {
        if let Some(proxy_connector) = &self.proxy_manager {
            proxy_connector.remove(connect_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_string() {
        // Safety:
        // Function returns a constant c_str
        let c_version_str = unsafe { CStr::from_ptr(crate::libslirp_sys::slirp_version_string()) };
        assert_eq!("4.7.0", c_version_str.to_str().unwrap());
    }

    // Utility function to create and launch a slirp polling thread
    fn launch_polling_thread() -> (
        mpsc::Sender<SlirpCmd>,
        mpsc::Receiver<SlirpCmd>,
        mpsc::Sender<PollRequest>,
        thread::JoinHandle<()>,
    ) {
        let (tx_cmds, rx_cmds) = mpsc::channel::<SlirpCmd>();
        let (tx_poll, rx_poll) = mpsc::channel::<PollRequest>();
        let (signal_rx, _signal_tx) = create_pipe().unwrap();

        let tx_cmds_clone = tx_cmds.clone();
        let handle = thread::Builder::new()
            .name("test_slirp_poll".to_string())
            .spawn(move || slirp_poll_thread(rx_poll, tx_cmds_clone, signal_rx))
            .unwrap();

        (tx_cmds, rx_cmds, tx_poll, handle)
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn to_os_fd(stream: &impl AsRawFd) -> i32 {
        stream.as_raw_fd()
    }
    #[cfg(target_os = "windows")]
    fn to_os_fd(stream: &impl AsRawSocket) -> i32 {
        stream.as_raw_socket() as i32
    }

    /// Polls a file descriptor and asserts the received events.
    ///
    /// On some systems (e.g. macOS), the socket state may not update instantly,
    /// leading to poll not returning the expected events immediately.
    /// A short delay and a single retry handle this transient condition.
    fn poll_and_assert_result(
        tx_poll: &mpsc::Sender<PollRequest>,
        rx_cmds: &mpsc::Receiver<SlirpCmd>,
        fd: i32,
        poll_events: SlirpPollType,
        expected_revents: SlirpPollType,
    ) {
        const POLL_RETRIES: u32 = 2;
        const POLL_RETRY_DELAY_MS: u64 = 100;

        for i in 0..POLL_RETRIES {
            assert!(
                tx_poll.send((vec![PollFd { fd, events: poll_events, revents: 0 }], 1000)).is_ok(),
                "failed to send poll request"
            );

            let poll_fd = match rx_cmds.recv() {
                Ok(SlirpCmd::PollResult(mut poll_fds, _)) if poll_fds.len() == 1 => {
                    poll_fds.remove(0)
                }
                Ok(other) => panic!("unexpected command from poll thread: {other:?}"),
                Err(e) => panic!("failed to receive command from poll thread: {e}"),
            };

            assert_eq!(poll_fd.fd, fd, "poll fd mismatch");

            if poll_fd.revents == expected_revents {
                return; // Success
            }

            if i < POLL_RETRIES - 1 {
                warn!(
                    "poll retry for fd {fd}: got {}, expected {}",
                    poll_fd.revents, expected_revents
                );
                thread::sleep(Duration::from_millis(POLL_RETRY_DELAY_MS));
            } else {
                assert_eq!(poll_fd.revents, expected_revents, "revents mismatch after retries");
            }
        }
    }

    // Create and return TcpListener and TcpStream of a connected pipe
    fn create_stream_pipe() -> (TcpListener, TcpStream) {
        // Create a TcpStream pipe for testing.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let writer = TcpStream::connect(addr).unwrap();
        (listener, writer)
    }

    // Create and return reader and writer TcpStreams of an accepted pipe
    fn create_accepted_stream_pipe() -> (TcpStream, TcpStream) {
        // Create a TcpStream pipe
        let (listener, writer) = create_stream_pipe();
        // Accept the connection
        let (reader, _) = listener.accept().unwrap();
        (reader, writer)
    }

    // Initialize an accepted TcpStream pipe with initial data
    fn init_pipe() -> (TcpStream, TcpStream) {
        // Create an accepted TcpStream pipe
        let (reader, mut writer) = create_accepted_stream_pipe();
        // Write initial data to pipe
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        writer.write_all(&[1]).unwrap();
        #[cfg(target_os = "windows")]
        writer.write_all(b"1").unwrap();

        (reader, writer)
    }

    #[test]
    fn test_slirp_poll_thread_exit() {
        let (_tx_cmds, _rx_cmds, tx_poll, handle) = launch_polling_thread();
        // Drop the sender to end the polling thread and wait for the polling
        // thread to exit
        drop(tx_poll);
        handle.join().unwrap();
    }

    #[test]
    fn test_poll_invalid_fd() {
        // Launch the slirp polling thread.
        let (_tx_cmds, rx_cmds, tx_poll, handle) = launch_polling_thread();

        let invalid_fd = -1;
        // Check that the poll result indicates 0 (fd not ready).
        poll_and_assert_result(&tx_poll, &rx_cmds, invalid_fd, SLIRP_POLL_IN, 0);

        // Drop the sender to end the polling thread and wait for the polling
        // thread to exit
        drop(tx_poll);
        handle.join().unwrap();
    }

    #[test]
    fn test_close_fd_before_accept() {
        // Launch the slirp polling thread.
        let (_tx_cmds, rx_cmds, tx_poll, handle) = launch_polling_thread();

        // Init a "broken" pipe that is closed before being accepted
        let (listener, writer) = create_stream_pipe();

        // Close the listener before accepting the connection
        drop(listener);

        // Check the expected result when file descriptor is not ready.
        #[cfg(target_os = "linux")]
        let expected_revents = SLIRP_POLL_IN | SLIRP_POLL_HUP | SLIRP_POLL_ERR;
        // TODO: Identify way to trigger and test POLL_ERR for macOS
        #[cfg(target_os = "macos")]
        let expected_revents = SLIRP_POLL_IN | SLIRP_POLL_HUP;
        #[cfg(target_os = "windows")]
        let expected_revents = SLIRP_POLL_HUP | SLIRP_POLL_ERR;
        poll_and_assert_result(
            &tx_poll,
            &rx_cmds,
            to_os_fd(&writer),
            SLIRP_POLL_IN,
            expected_revents,
        );

        // Drop the sender to end the polling thread and wait for the polling
        // thread to exit
        drop(tx_poll);
        handle.join().unwrap();
    }

    #[test]
    fn test_accept_close_before_write() {
        // Launch the slirp polling thread.
        let (_tx_cmds, rx_cmds, tx_poll, handle) = launch_polling_thread();
        // Init a "broken" pipe that is accepted but no initial data is written
        let (reader, writer) = create_accepted_stream_pipe();
        let reader_fd = to_os_fd(&reader);
        // Close the writer end of the pipe
        drop(writer);

        // Check the expected poll result when writer is closed before data is
        // written
        #[cfg(target_os = "linux")]
        let expected_revents = SLIRP_POLL_IN;
        #[cfg(target_os = "macos")]
        let expected_revents = SLIRP_POLL_IN | SLIRP_POLL_HUP;
        #[cfg(target_os = "windows")]
        let expected_revents = SLIRP_POLL_HUP;
        poll_and_assert_result(&tx_poll, &rx_cmds, reader_fd, SLIRP_POLL_IN, expected_revents);

        // Drop the sender to end the polling thread and wait for the polling
        // thread to exit
        drop(tx_poll);
        handle.join().unwrap();
    }

    #[test]
    fn test_accept_write_close() {
        // Launch the slirp polling thread.
        let (_tx_cmds, rx_cmds, tx_poll, handle) = launch_polling_thread();
        // Init a pipe for testing and get its reader file descriptor.
        let (mut reader, writer) = init_pipe();
        let reader_fd = to_os_fd(&reader);

        // --- Test polling for POLLIN event ---

        // Send a poll request and check that the poll result has POLLIN only
        poll_and_assert_result(&tx_poll, &rx_cmds, reader_fd, SLIRP_POLL_IN, SLIRP_POLL_IN);

        // Read / remove the data from the pipe.
        let mut buf = [0; 1];
        reader.read_exact(&mut buf).unwrap();

        // --- Test polling for no event after reading ---

        // Check that the poll result contains no event since there is no more
        // data
        poll_and_assert_result(&tx_poll, &rx_cmds, reader_fd, SLIRP_POLL_IN, 0);

        // --- Test polling for POLLHUP event when writer is closed ---

        // Close the writer
        drop(writer);

        // Shutdown the write half of the reader
        reader.shutdown(std::net::Shutdown::Write).unwrap();

        // Try to read from the socket to force an update of its state.
        // A read on a closed socket should return 0, or WouldBlock if
        // non-blocking. We don't need the result.
        let mut buf = [0; 1];
        let _ = reader.read(&mut buf);

        // Check that expected poll result when writer end is dropped
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let expected_revents = SLIRP_POLL_IN | SLIRP_POLL_HUP;
        #[cfg(target_os = "windows")]
        let expected_revents = SLIRP_POLL_HUP;
        poll_and_assert_result(&tx_poll, &rx_cmds, reader_fd, SLIRP_POLL_IN, expected_revents);

        // Drop the sender to end the polling thread and wait for the polling
        // thread to exit
        drop(tx_poll);
        handle.join().unwrap();
    }

    // TODO: Add testing for POLLNVAL case
}
