//  Copyright 2023 The Android Open Source Project
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

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

#[cfg(test)]
use tracing::info;
use tracing_log::{LogTracer, NormalizeEvent};
#[cfg(not(feature = "cuttlefish"))]
use tracing_subscriber::EnvFilter;
use tracing_subscriber::{
    Layer, filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt,
};

/// Maximum number of non-fatal error code buckets per session (255 distinct codes + 1 overflow bucket).
pub const MAX_ERROR_BUCKETS: usize = 256;

/// Sentinel value representing an unoccupied bucket slot in [`ErrorStatsTable`].
pub const EMPTY_ERROR_CODE: u32 = 0;

/// Fallback error code for unparsed 3P or C/FFI targets outside the Netsim workspace.
pub const UNPARSED_EXTERNAL_ERROR_CODE: u32 = 9999;

/// Reserved error code used for the final bucket when more than `MAX_ERROR_BUCKETS - 1`
/// (255) distinct error codes are recorded in a single session (`4294967295`).
pub const OVERFLOW_ERROR_CODE: u32 = u32::MAX;

/// Lock-free, fixed-size open-addressed hash table for session error code counts.
#[derive(Debug)]
pub struct ErrorStatsTable {
    buckets: [(AtomicU32, AtomicU32); MAX_ERROR_BUCKETS],
}

impl Default for ErrorStatsTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorStatsTable {
    /// Creates a new zero-initialized [`ErrorStatsTable`].
    pub const fn new() -> Self {
        Self {
            buckets: [const { (AtomicU32::new(EMPTY_ERROR_CODE), AtomicU32::new(0)) };
                MAX_ERROR_BUCKETS],
        }
    }

    /// Atomically records an occurrence of `code` using lock-free linear probing.
    ///
    /// Ignores [`EMPTY_ERROR_CODE`] (`0`). Once the first `MAX_ERROR_BUCKETS - 1` (`255`)
    /// slots are occupied by distinct codes, any additional distinct error code (or a direct
    /// call with [`OVERFLOW_ERROR_CODE`]) increments the reserved final bucket under
    /// [`OVERFLOW_ERROR_CODE`].
    pub fn record(&self, code: u32) {
        if code == EMPTY_ERROR_CODE {
            return;
        }
        if code != OVERFLOW_ERROR_CODE {
            for (slot_code, slot_count) in &self.buckets[..MAX_ERROR_BUCKETS - 1] {
                let current = slot_code.load(Ordering::Acquire);
                if current == code {
                    slot_count.fetch_add(1, Ordering::Release);
                    return;
                }
                if current == EMPTY_ERROR_CODE {
                    match slot_code.compare_exchange(
                        EMPTY_ERROR_CODE,
                        code,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => {
                            slot_count.fetch_add(1, Ordering::Release);
                            return;
                        }
                        Err(actual) if actual == code => {
                            slot_count.fetch_add(1, Ordering::Release);
                            return;
                        }
                        Err(_) => continue,
                    }
                }
            }
        }
        let (overflow_code, overflow_count) = &self.buckets[MAX_ERROR_BUCKETS - 1];
        overflow_code.store(OVERFLOW_ERROR_CODE, Ordering::Release);
        overflow_count.fetch_add(1, Ordering::Release);
    }

    /// Reads all active error buckets non-destructively,
    /// returning `(error_code, count)` pairs sorted ascending by `error_code`.
    pub fn snapshot(&self) -> Vec<(u32, u32)> {
        let mut records: Vec<(u32, u32)> = self
            .buckets
            .iter()
            .filter_map(|(slot_code, slot_count)| {
                let code = slot_code.load(Ordering::Acquire);
                let count = slot_count.load(Ordering::Acquire);
                (code != EMPTY_ERROR_CODE && count > 0).then_some((code, count))
            })
            .collect();
        records.sort_unstable_by_key(|&(code, _)| code);
        records
    }
}

static ERROR_STATS_TABLE: ErrorStatsTable = ErrorStatsTable::new();

/// Returns a non-destructive `(error_code, count)` snapshot of the process-global [`ErrorStatsTable`].
pub fn snapshot_error_stats() -> Vec<(u32, u32)> {
    ERROR_STATS_TABLE.snapshot()
}

/// Per-subsystem `CRATE_DEFAULT` (`X0000`) error code bases.
///
/// Each crate owns its specific error codes in its own `err.rs` and tags call
/// sites with `error!(code = err::CODE, %error, ...)`. These named bases are the single
/// definition of each subsystem's range: crate `err.rs` modules set their
/// `CRATE_DEFAULT` from them, and [`default_error_code_for_target`] uses them as
/// the fallback for untyped `error!` events. `netsim-common` sits at the bottom of
/// the crate graph, so it cannot import constants from the crates themselves.
pub mod err_bases {
    /// Daemon & startup (`daemon`, `common`).
    pub const DAEMON: u32 = 10_000;
    /// Actor framework & device actor.
    pub const DEVICE_ACTOR: u32 = 20_000;
    /// Packet stream, device/link APIs, model & packets.
    pub const PACKET_STREAM: u32 = 30_000;
    /// Frontend APIs (gRPC, REST, WebSocket, CLI).
    pub const FRONTEND: u32 = 40_000;
    /// Bluetooth actor & RootCanal.
    pub const BLUETOOTH: u32 = 50_000;
    /// Wi-Fi actor.
    pub const WIFI: u32 = 60_000;
    /// Access point actor & hostapd.
    pub const AP: u32 = 70_000;
    /// NFC actor.
    pub const NFC: u32 = 80_000;
    /// NFC external backend (in-process `casimir`), inside the NFC range.
    pub const NFC_CASIMIR: u32 = NFC + 8_000;
    /// UWB actor.
    pub const UWB: u32 = 90_000;
    /// UWB external backend (in-process `pica`), inside the UWB range.
    pub const UWB_PICA: u32 = UWB + 8_000;
    /// Cellular actor & modem.
    pub const CELL: u32 = 100_000;
    /// Slirp / Ethernet / HTTP proxy.
    pub const SLIRP: u32 = 110_000;
    /// Packet capture.
    pub const CAPTURE: u32 = 120_000;

    /// Width of each subsystem range (`[base, base + RANGE_SIZE)`).
    pub const RANGE_SIZE: u32 = 10_000;

    /// All top-level subsystem bases, in ascending order.
    pub const SUBSYSTEMS: [u32; 12] = [
        DAEMON,
        DEVICE_ACTOR,
        PACKET_STREAM,
        FRONTEND,
        BLUETOOTH,
        WIFI,
        AP,
        NFC,
        UWB,
        CELL,
        SLIRP,
        CAPTURE,
    ];
}

/// Maps a `tracing` metadata target (or `log::` crate prefix) to its subsystem
/// `CRATE_DEFAULT` (`X0000`) fallback error code.
///
/// Only used when an `ERROR` event carries no integer `code` field.
/// Crates own their specific codes in `err.rs` and pass them explicitly via
/// `error!(code = err::CODE, %error, ...)`.
pub fn default_error_code_for_target(target: &str) -> u32 {
    use err_bases::*;
    let crate_prefix = target.split("::").next().unwrap_or(target);
    match crate_prefix {
        "daemon" | "netsim_daemon" | "netsimd" | "common" | "netsim_common" => DAEMON,
        "actor_framework" | "device_actor" | "link_actor" => DEVICE_ACTOR,
        "device_api"
        | "link_api"
        | "packet_stream"
        | "packet_stream_client"
        | "packets"
        | "netsim_packets"
        | "model"
        | "netsim_model" => PACKET_STREAM,
        "grpc_server" | "netsim_rest_api" | "rest_api" | "websocket_server" | "cli"
        | "netsim_cli" => FRONTEND,
        "bluetooth_actor" | "rootcanal" | "rootcanal_rs" | "rootcanal_server" | "hci_server" => {
            BLUETOOTH
        }
        "wifi_actor" => WIFI,
        "ap_actor" | "hostapd_rs" => AP,
        "nfc_actor" => NFC,
        "casimir" => NFC_CASIMIR,
        "uwb_actor" => UWB,
        "pica" => UWB_PICA,
        "cell_actor" | "modem_rs" => CELL,
        "slirp_actor" | "ethernet_actor" | "http_proxy" | "slirp" | "slirp_host"
        | "libslirp_rs" => SLIRP,
        "capture_actor" | "capture_api" => CAPTURE,
        _ => UNPARSED_EXTERNAL_ERROR_CODE,
    }
}

/// Custom `tracing_subscriber::Layer` that intercepts `ERROR` and `WARN` events
/// to populate [`ErrorStatsTable`] without heap allocations or locks on the hot path.
#[derive(Clone, Debug, Default)]
pub struct ErrorStatsLayer {
    table: Option<Arc<ErrorStatsTable>>,
}

impl ErrorStatsLayer {
    /// Creates a layer backed by the process-global [`ErrorStatsTable`].
    pub const fn new() -> Self {
        Self { table: None }
    }

    /// Creates a layer backed by an explicit [`ErrorStatsTable`] (useful for isolated tests).
    pub const fn with_table(table: Arc<ErrorStatsTable>) -> Self {
        Self { table: Some(table) }
    }

    fn record_code(&self, code: u32) {
        match &self.table {
            Some(table) => table.record(code),
            None => ERROR_STATS_TABLE.record(code),
        }
    }
}

/// Name of the dedicated error-code field at log sites, e.g.
/// `error!(code = err::CODE, %error, "...")`. `%error` (or any other field) keeps
/// carrying the `Display` chain for stderr; only this integer field is counted.
/// Other names (including `err`) are deliberately not read, to avoid clashing
/// with common `Err(err)` / `%err` bindings.
pub const ERROR_CODE_FIELD: &str = "code";

/// Extracts the numeric error code from an event's `code` field. Any other field,
/// and non-integer values (e.g. `%err` / `?err` `Display`/`Debug` objects such as
/// `Box<dyn Error>`, `std::io::Error`, `tonic::Status`), are ignored.
#[derive(Default)]
struct ErrCodeVisitor {
    err_code: Option<u32>,
}

impl tracing::field::Visit for ErrCodeVisitor {
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == ERROR_CODE_FIELD {
            self.err_code = u32::try_from(value).ok();
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if field.name() == ERROR_CODE_FIELD {
            self.err_code = u32::try_from(value).ok();
        }
    }

    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {}
}

impl<S> Layer<S> for ErrorStatsLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let normalized = event.normalized_metadata();
        let meta = normalized.as_ref().unwrap_or_else(|| event.metadata());
        let level = *meta.level();
        if level > tracing::Level::WARN {
            return;
        }

        let mut visitor = ErrCodeVisitor::default();
        event.record(&mut visitor);

        if let Some(code) = visitor.err_code.filter(|&c| c != EMPTY_ERROR_CODE) {
            self.record_code(code);
        } else if level == tracing::Level::ERROR {
            self.record_code(default_error_code_for_target(meta.target()));
        }
    }
}

/// Formats the current time for logging.
fn log_current_time() -> String {
    chrono::Utc::now().format("%m-%d %H:%M:%S%.3f").to_string()
}

/// Initiating the environment for logging with given prefix
///
/// The current log format follows the same format as Android Emulator team.
pub fn init(prefix: &'static str, is_verbose: bool) {
    let _ = LogTracer::init();

    let error_stats_layer = ErrorStatsLayer::new().with_filter(LevelFilter::WARN);

    #[cfg(not(feature = "cuttlefish"))]
    {
        let log_filter = if is_verbose { "debug" } else { "info" };
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_filter));
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .event_format(CustomFormatter { prefix })
            .with_filter(filter);
        let _ = tracing_subscriber::registry().with(error_stats_layer).with(fmt_layer).try_init();
    }

    #[cfg(feature = "cuttlefish")]
    {
        let level = if is_verbose { LevelFilter::DEBUG } else { LevelFilter::INFO };
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .event_format(CustomFormatter { prefix })
            .with_filter(level);
        let _ = tracing_subscriber::registry().with(error_stats_layer).with(fmt_layer).try_init();
    }
}

/// Initiating the environment for logging in Rust unit tests
///
/// The current log format follows the same format as Android Emulator team.
pub fn init_for_test() {
    let _ = LogTracer::init();

    let error_stats_layer = ErrorStatsLayer::new().with_filter(LevelFilter::WARN);

    #[cfg(not(feature = "cuttlefish"))]
    {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        let fmt_layer = tracing_subscriber::fmt::layer()
            .event_format(CustomFormatter { prefix: "netsim-test" })
            .with_test_writer()
            .with_filter(filter);
        let _ = tracing_subscriber::registry().with(error_stats_layer).with(fmt_layer).try_init();
    }

    #[cfg(feature = "cuttlefish")]
    {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .event_format(CustomFormatter { prefix: "netsim-test" })
            .with_test_writer()
            .with_filter(LevelFilter::INFO);
        let _ = tracing_subscriber::registry().with(error_stats_layer).with(fmt_layer).try_init();
    }
}

struct CustomFormatter {
    prefix: &'static str,
}

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for CustomFormatter
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let normalized = event.normalized_metadata();
        let meta = normalized.as_ref().unwrap_or_else(|| event.metadata());
        let level = match *meta.level() {
            tracing::Level::ERROR => "E",
            tracing::Level::WARN => "W",
            tracing::Level::INFO => "I",
            tracing::Level::DEBUG => "D",
            tracing::Level::TRACE => "T",
        };

        let file = meta
            .file()
            .and_then(|f| std::path::Path::new(f).file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("N/A");
        let line = meta.line().unwrap_or(0);

        write!(writer, "{} {} {} {}:{} - ", self.prefix, level, log_current_time(), file, line)?;

        {
            use std::fmt::Write;

            #[derive(Default)]
            struct Visitor {
                message: String,
                fields: String,
            }

            impl Visitor {
                fn record_field(
                    &mut self,
                    field: &tracing::field::Field,
                    value: std::fmt::Arguments<'_>,
                ) {
                    let name = field.name();
                    if name == "message" {
                        let _ = write!(self.message, "{value}");
                        return;
                    }
                    if name.starts_with("log.") {
                        return;
                    }
                    let key = name.strip_prefix("r#").unwrap_or(name);
                    if !self.fields.is_empty() {
                        self.fields.push(' ');
                    }
                    let _ = write!(self.fields, "{key}={value}");
                }
            }

            impl tracing::field::Visit for Visitor {
                fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
                    if field.name() == ERROR_CODE_FIELD {
                        return;
                    }
                    self.record_field(field, format_args!("{value}"));
                }

                fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
                    if field.name() == ERROR_CODE_FIELD {
                        return;
                    }
                    self.record_field(field, format_args!("{value}"));
                }

                fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                    self.record_field(field, format_args!("{value}"));
                }

                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    self.record_field(field, format_args!("{value:?}"));
                }
            }
            let mut visitor = Visitor::default();
            event.record(&mut visitor);
            match (visitor.message.is_empty(), visitor.fields.is_empty()) {
                (false, false) => write!(writer, "{} {}", visitor.message, visitor.fields)?,
                (false, true) => write!(writer, "{}", visitor.message)?,
                (true, false) => write!(writer, "{}", visitor.fields)?,
                (true, true) => {}
            }
        }
        writeln!(writer)
    }
}

/// This test is an example of having logs in Rust unit tests
///
/// Expected log: INFO  | netsim-test: Hello Netsim
#[test]
fn test_init_for_test() {
    init_for_test();
    info!("Hello Netsim");
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier, Mutex};

    use super::*;

    static LOG_BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());

    struct BufferWriter;

    impl std::io::Write for BufferWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            LOG_BUFFER.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // Since Subscriber can only be initialized once, we use with_default for
    // tracing tests.
    #[test]
    fn test_format_matches_legacy() {
        #[cfg(not(feature = "cuttlefish"))]
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("info"))
            .event_format(CustomFormatter { prefix: "netsim" })
            .with_writer(|| BufferWriter)
            .finish();

        #[cfg(feature = "cuttlefish")]
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .event_format(CustomFormatter { prefix: "netsim" })
            .with_writer(|| BufferWriter)
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            LOG_BUFFER.lock().unwrap().clear();
            info!("Test tracing message");
            let output = String::from_utf8(LOG_BUFFER.lock().unwrap().clone()).unwrap();
            // Match pattern: netsim I MM-DD HH:MM:SS.sss netsim_logger.rs:LINE - Test
            // tracing message
            let re = regex::Regex::new(r"netsim I \d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3} netsim_logger\.rs:\d+ - Test tracing message").unwrap();
            assert!(re.is_match(&output), "Output did not match legacy format: {}", output);
        });

        // For info!, it uses the global dispatcher. If another test already
        // initialized it, we might not capture it in LOG_BUFFER. We skip log
        // verification if initialization fails.
        let _ = LogTracer::init();
        #[cfg(not(feature = "cuttlefish"))]
        let _ = tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("info"))
            .event_format(CustomFormatter { prefix: "netsim" })
            .with_writer(|| BufferWriter)
            .try_init();

        #[cfg(feature = "cuttlefish")]
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .event_format(CustomFormatter { prefix: "netsim" })
            .with_writer(|| BufferWriter)
            .try_init();

        // Try log verification but don't fail if buffer is empty (meaning global was
        // already set elsewhere)
        LOG_BUFFER.lock().unwrap().clear();
        info!("Test log message");
        let output = String::from_utf8(LOG_BUFFER.lock().unwrap().clone()).unwrap();
        if !output.is_empty() {
            let re = regex::Regex::new(r"netsim I \d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3} netsim_logger\.rs:\d+ - Test log message").unwrap();
            assert!(re.is_match(&output), "Log output did not match legacy format: {}", output);
        }
    }

    #[derive(Clone)]
    struct LocalWriter(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for LocalWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LocalWriter {
        type Writer = LocalWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn test_format_structured_fields() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .event_format(CustomFormatter { prefix: "netsim" })
            .with_writer(LocalWriter(buffer.clone()))
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            let get_line = || {
                let mut buf = buffer.lock().unwrap();
                let output = String::from_utf8(buf.clone()).unwrap();
                buf.clear();
                output
            };

            let err = std::io::Error::other("disk full");
            tracing::error!(error = %err, "Failed to write stats");
            let line_a1 = get_line();
            assert!(
                line_a1.ends_with(" - Failed to write stats error=disk full\n"),
                "Expected line to end with ' - Failed to write stats error=disk full\\n', got: {line_a1:?}"
            );

            tracing::error!(error = "disk full", "Failed to write stats");
            let line_a2 = get_line();
            assert!(
                line_a2.ends_with(" - Failed to write stats error=disk full\n"),
                "Expected line to end with ' - Failed to write stats error=disk full\\n', got: {line_a2:?}"
            );

            tracing::warn!(error = ?std::io::Error::other("timeout"), "Operation timed out");
            let line_b = get_line();
            assert!(
                line_b.contains(" - Operation timed out error=Custom {"),
                "Expected line to contain ' - Operation timed out error=Custom {{', got: {line_b:?}"
            );

            tracing::error!(chip_id = 42, error = %"disconnected", "Bridge failed");
            let line_c = get_line();
            assert!(
                line_c.ends_with(" - Bridge failed chip_id=42 error=disconnected\n"),
                "Expected line to end with ' - Bridge failed chip_id=42 error=disconnected\\n', got: {line_c:?}"
            );

            tracing::error!(error = %"fatal error");
            let line_d = get_line();
            assert!(
                line_d.ends_with(" - error=fatal error\n"),
                "Expected line to end with ' - error=fatal error\\n', got: {line_d:?}"
            );

            tracing::warn!(
                log.target = "slirp",
                log.module_path = "slirp::tftp",
                log.file = "tftp.rs",
                log.line = 7u32,
                error = %"bad packet",
                "TFTP warning"
            );
            let line_e = get_line();
            assert!(
                line_e.ends_with(" - TFTP warning error=bad packet\n"),
                "Expected line to end with ' - TFTP warning error=bad packet\\n', got: {line_e:?}"
            );
        });
    }

    #[test]
    fn test_error_stats_table_record_and_snapshot_sorted() {
        let table = ErrorStatsTable::default();
        // Sentinel 0 is ignored.
        table.record(EMPTY_ERROR_CODE);
        assert!(table.snapshot().is_empty());

        // Insert out-of-order codes and verify non-destructive sorted snapshots.
        table.record(88001);
        table.record(66001);
        table.record(66001);
        table.record(10001);
        table.record(88001);
        table.record(66001);

        let snap1 = table.snapshot();
        assert_eq!(snap1, vec![(10001, 1), (66001, 3), (88001, 2)]);

        // Second snapshot must be non-destructive and accumulate further counts.
        table.record(10001);
        let snap2 = table.snapshot();
        assert_eq!(snap2, vec![(10001, 2), (66001, 3), (88001, 2)]);
    }

    #[test]
    fn test_error_stats_table_overflow_bucket_and_existing_codes() {
        let table = ErrorStatsTable::new();
        let regular_slots = (MAX_ERROR_BUCKETS - 1) as u32;

        // Fill the 255 regular slots with distinct codes 1..=255.
        for code in 1..=regular_slots {
            table.record(code);
        }
        let snap_before_overflow = table.snapshot();
        assert_eq!(snap_before_overflow.len(), MAX_ERROR_BUCKETS - 1);
        assert!(
            snap_before_overflow
                .iter()
                .all(|&(code, count)| code != OVERFLOW_ERROR_CODE && count == 1)
        );

        // Recording new distinct codes once 255 slots are full increments OVERFLOW_ERROR_CODE.
        table.record(10_001);
        table.record(20_002);
        let snap_after_overflow = table.snapshot();
        assert_eq!(snap_after_overflow.len(), MAX_ERROR_BUCKETS);
        assert_eq!(snap_after_overflow.last(), Some(&(OVERFLOW_ERROR_CODE, 2)));

        // Existing codes among the 255 slots still increment their own counters.
        table.record(1);
        table.record(regular_slots);
        let snap_existing = table.snapshot();
        assert_eq!(snap_existing.len(), MAX_ERROR_BUCKETS);
        assert_eq!(snap_existing.first(), Some(&(1, 2)));
        assert_eq!(snap_existing.get(MAX_ERROR_BUCKETS - 2), Some(&(regular_slots, 2)));
        assert_eq!(snap_existing.last(), Some(&(OVERFLOW_ERROR_CODE, 2)));

        // Direct record(OVERFLOW_ERROR_CODE) increments the reserved overflow bucket without duplicating it.
        table.record(OVERFLOW_ERROR_CODE);
        let snap_direct_overflow = table.snapshot();
        assert_eq!(snap_direct_overflow.len(), MAX_ERROR_BUCKETS);
        assert_eq!(snap_direct_overflow.last(), Some(&(OVERFLOW_ERROR_CODE, 3)));
    }

    #[test]
    fn test_error_stats_table_concurrent_contention_and_capacity() {
        let table = Arc::new(ErrorStatsTable::new());
        let num_threads = 8usize;
        let events_per_thread = 1000usize;
        let distinct_codes = 300u32;
        let barrier = Arc::new(Barrier::new(num_threads));

        let mut handles = Vec::with_capacity(num_threads);
        for tid in 0..num_threads {
            let table_clone = Arc::clone(&table);
            let barrier_clone = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier_clone.wait();
                for i in 0..events_per_thread {
                    let code = 60000 + ((tid + i) as u32 % distinct_codes) + 1;
                    table_clone.record(code);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let snap = table.snapshot();
        assert_eq!(snap.len(), MAX_ERROR_BUCKETS);
        assert_eq!(snap.last().map(|&(code, _)| code), Some(OVERFLOW_ERROR_CODE));

        // Verify strict ascending sort and no duplicate slots under concurrent insertion.
        for window in snap.windows(2) {
            assert!(window[0].0 < window[1].0);
        }

        // Every recorded event is accounted for across the 255 regular buckets + 1 overflow bucket.
        let total_recorded: u32 = snap.iter().map(|&(_, count)| count).sum();
        assert_eq!(total_recorded, (num_threads * events_per_thread) as u32);
    }

    #[test]
    fn test_error_stats_table_concurrent_same_code_no_duplicate_slots() {
        let table = Arc::new(ErrorStatsTable::new());
        let num_threads = 8usize;
        let events_per_thread = 500usize;
        let barrier = Arc::new(Barrier::new(num_threads));

        let mut handles = Vec::with_capacity(num_threads);
        for _ in 0..num_threads {
            let table_clone = Arc::clone(&table);
            let barrier_clone = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier_clone.wait();
                for _ in 0..events_per_thread {
                    table_clone.record(66001);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let snap = table.snapshot();
        assert_eq!(snap, vec![(66001, (num_threads * events_per_thread) as u32)]);
    }

    #[test]
    fn test_error_stats_layer_filtering_and_stderr_format() {
        let table = Arc::new(ErrorStatsTable::new());
        let buffer = Arc::new(Mutex::new(Vec::new()));

        let error_stats_layer =
            ErrorStatsLayer::with_table(Arc::clone(&table)).with_filter(LevelFilter::WARN);
        let fmt_layer = tracing_subscriber::fmt::layer()
            .event_format(CustomFormatter { prefix: "netsim" })
            .with_writer(LocalWriter(Arc::clone(&buffer)))
            .with_filter(LevelFilter::DEBUG);
        let subscriber = tracing_subscriber::registry().with(error_stats_layer).with(fmt_layer);

        tracing::subscriber::with_default(subscriber, || {
            let get_line = || {
                let mut buf = buffer.lock().unwrap();
                let output = String::from_utf8(buf.clone()).unwrap();
                buf.clear();
                output
            };

            // 1. Typed ERROR and WARN increment their u32 code without printing `err=` to stderr.
            tracing::error!(code = 66001u32, "HWSIM frame parse failed");
            let err_line = get_line();
            assert!(
                err_line.ends_with(" - HWSIM frame parse failed\n"),
                "Expected stderr line without code=66001, got: {err_line:?}"
            );

            tracing::warn!(code = 66001i64, chip_id = 7, "HWSIM retry warning");
            let warn_line = get_line();
            assert!(
                warn_line.ends_with(" - HWSIM retry warning chip_id=7\n"),
                "Expected stderr line with chip_id=7 and without err=66001, got: {warn_line:?}"
            );

            tracing::warn!(code = 62001i64, chip_id = -1i64, "Socket retry");
            let neg_line = get_line();
            assert!(
                neg_line.ends_with(" - Socket retry chip_id=-1\n"),
                "Expected stderr line with chip_id=-1 and without err=62001, got: {neg_line:?}"
            );

            // 2. Untyped WARN, INFO, and DEBUG are ignored by ErrorStatsLayer.
            tracing::warn!("Transient untyped warning");
            tracing::info!("Normal info message");
            tracing::debug!("Verbose debug message");
            let _ = get_line();

            // 3. Untyped ERROR (or ERROR with code = 0) increments the crate's X0000 CRATE_DEFAULT (10000 for `common`).
            tracing::error!("Uncategorized error in common crate");
            let _ = get_line();
            tracing::error!(code = 0u32, "Zero err code falls back to crate default");
            let _ = get_line();
        });

        let snap = table.snapshot();
        assert_eq!(snap, vec![(10000, 2), (62001, 1), (66001, 2)]);
    }

    #[test]
    fn test_error_stats_layer_decoupled_from_env_filter_off() {
        let table = Arc::new(ErrorStatsTable::new());
        let buffer = Arc::new(Mutex::new(Vec::new()));

        let error_stats_layer =
            ErrorStatsLayer::with_table(Arc::clone(&table)).with_filter(LevelFilter::WARN);
        #[cfg(not(feature = "cuttlefish"))]
        let fmt_layer = tracing_subscriber::fmt::layer()
            .event_format(CustomFormatter { prefix: "netsim" })
            .with_writer(LocalWriter(Arc::clone(&buffer)))
            .with_filter(EnvFilter::new("off"));
        #[cfg(feature = "cuttlefish")]
        let fmt_layer = tracing_subscriber::fmt::layer()
            .event_format(CustomFormatter { prefix: "netsim" })
            .with_writer(LocalWriter(Arc::clone(&buffer)))
            .with_filter(LevelFilter::OFF);

        let subscriber = tracing_subscriber::registry().with(error_stats_layer).with(fmt_layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(code = 66001u32, "Suppressed from stderr but counted");
            tracing::warn!(code = 84002u32, "Suppressed warn from stderr but counted");
            tracing::error!(target: "wifi_actor::medium", "Untyped wifi error");
        });

        assert!(buffer.lock().unwrap().is_empty());
        let snap = table.snapshot();
        assert_eq!(snap, vec![(60000, 1), (66001, 1), (84002, 1)]);
    }

    #[test]
    fn test_error_stats_layer_code_field_with_display_error() {
        let table = Arc::new(ErrorStatsTable::new());
        let buffer = Arc::new(Mutex::new(Vec::new()));

        let error_stats_layer =
            ErrorStatsLayer::with_table(Arc::clone(&table)).with_filter(LevelFilter::WARN);
        let fmt_layer = tracing_subscriber::fmt::layer()
            .event_format(CustomFormatter { prefix: "netsim" })
            .with_writer(LocalWriter(Arc::clone(&buffer)))
            .with_filter(LevelFilter::DEBUG);
        let subscriber = tracing_subscriber::registry().with(error_stats_layer).with(fmt_layer);

        tracing::subscriber::with_default(subscriber, || {
            let get_line = || {
                let mut buf = buffer.lock().unwrap();
                let output = String::from_utf8(buf.clone()).unwrap();
                buf.clear();
                output
            };

            // `code` carries the counted error code; `%err` carries the Display chain.
            let err: Box<dyn std::error::Error> = std::io::Error::other("disk full").into();
            tracing::error!(code = 66002u32, %err, "Write failed");
            let line = get_line();
            assert!(
                line.ends_with(" - Write failed err=disk full\n"),
                "Expected Display chain kept and code= hidden, got: {line:?}"
            );

            // A `%err` Display object alone does not break extraction: falls back to the
            // crate default rather than being mistaken for a code.
            tracing::error!(%err, "Untyped failure");
            let _ = get_line();

            // `err` is not a code field: an integer `err` is ignored and the ERROR falls
            // back to the crate default, and it is printed like any other field.
            tracing::error!(err = 62004u32, "Integer err is not a code");
            let line = get_line();
            assert!(
                line.ends_with(" - Integer err is not a code err=62004\n"),
                "Expected err=62004 to be printed, got: {line:?}"
            );
        });

        let snap = table.snapshot();
        assert_eq!(snap, vec![(10000, 2), (66002, 1)]);
    }

    #[test]
    fn test_default_error_code_for_target_mappings() {
        assert_eq!(default_error_code_for_target("daemon::netsimd"), 10000);
        assert_eq!(default_error_code_for_target("common::util"), 10000);
        assert_eq!(default_error_code_for_target("device_actor::service"), 20000);
        assert_eq!(default_error_code_for_target("actor_framework"), 20000);
        assert_eq!(default_error_code_for_target("link_actor"), 20000);
        assert_eq!(default_error_code_for_target("packet_stream::transport"), 30000);
        assert_eq!(default_error_code_for_target("grpc_server::frontend"), 40000);
        assert_eq!(default_error_code_for_target("netsim_rest_api"), 40000);
        assert_eq!(default_error_code_for_target("websocket_server"), 40000);
        assert_eq!(default_error_code_for_target("bluetooth_actor::actor"), 50000);
        assert_eq!(default_error_code_for_target("rootcanal::ffi"), 50000);
        assert_eq!(default_error_code_for_target("hci_server"), 50000);
        assert_eq!(default_error_code_for_target("wifi_actor::stats"), 60000);
        assert_eq!(default_error_code_for_target("ap_actor::hostapd"), 70000);
        assert_eq!(default_error_code_for_target("nfc_actor::actor"), 80000);
        assert_eq!(default_error_code_for_target("casimir::controller"), 88000);
        assert_eq!(default_error_code_for_target("uwb_actor::ranging"), 90000);
        assert_eq!(default_error_code_for_target("pica::uci"), 98000);
        assert_eq!(default_error_code_for_target("cell_actor"), 100000);
        assert_eq!(default_error_code_for_target("modem_rs::at"), 100000);
        assert_eq!(default_error_code_for_target("slirp_actor"), 110000);
        assert_eq!(default_error_code_for_target("ethernet_actor"), 110000);
        assert_eq!(default_error_code_for_target("http_proxy"), 110000);
        assert_eq!(default_error_code_for_target("slirp::context"), 110000);
        assert_eq!(default_error_code_for_target("capture_actor"), 120000);
        assert_eq!(default_error_code_for_target("hyper::proto::h2"), UNPARSED_EXTERNAL_ERROR_CODE);
    }

    #[test]
    fn test_err_bases_unique_aligned_non_overlapping() {
        use err_bases::*;
        for pair in SUBSYSTEMS.windows(2) {
            assert!(pair[0] < pair[1], "bases must be strictly ascending: {pair:?}");
            assert!(pair[0] + RANGE_SIZE <= pair[1], "ranges overlap: {pair:?}");
        }
        for base in SUBSYSTEMS {
            assert_eq!(base % RANGE_SIZE, 0, "base {base} not aligned to {RANGE_SIZE}");
            assert!(base > UNPARSED_EXTERNAL_ERROR_CODE, "base {base} collides with external");
        }
        // In-process external backends stay inside their parent subsystem range.
        assert!((NFC..NFC + RANGE_SIZE).contains(&NFC_CASIMIR));
        assert!((UWB..UWB + RANGE_SIZE).contains(&UWB_PICA));
    }

    #[test]
    fn test_global_error_stats_via_tracing() {
        let subscriber = tracing_subscriber::registry().with(ErrorStatsLayer::new());
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(code = 10001u32, "global table recording via tracing");
        });
        let snap = snapshot_error_stats();
        assert!(snap.iter().any(|&(code, count)| code == 10001 && count >= 1));
    }
}
