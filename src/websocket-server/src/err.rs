// Copyright 2026 The Android Open Source Project
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

//! Typed error codes for `websocket-server` (`40001..=49999`, `code % 1000`
//! sub-band `+800..=+999`, `go/netsim-error-stats-dd`): `+0001..+1999` Config
//! & Input, `+2000..+3999` OS & System I/O, `+4000..+5999` Client & Transport,
//! `+6000..+7999` Internal & State (`+0000` reserved for untyped fallback).
//! Pass `code = crate::err::Code::Variant as u32` to `error!`/`warn!`; never
//! renumber or reuse existing codes.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum Code {
    // +0001..+1999: Config & Input (`+800..=+999`)
    InvalidUpgradeRequest = 40800,
    TextFrameAfterBinary = 40801,
    TextFrameNonBluetooth = 40802,
    TextFrameEmptyBtProps = 40803,
    TextFrameParseFailed = 40804,
    // +2000..+3999: OS & System I/O (`+800..=+999`)
    ListenerAcceptFailed = 42800,
    SetStreamBlockingFailed = 42801,
    StreamIntoStdFailed = 42802,
    StreamCloneFailed = 42803,
    // +4000..+5999: Client & Transport (`+800..=+999`)
    HandshakeFailed = 44800,
    ClientWriteFailed = 44801,
    TransportReadFailed = 44802,
    TransportWriteFailed = 44803,
    // +6000..+7999: Internal & State (`+800..=+999`)
    SetupVirtualChipFailed = 46800,
    ApplyBtConfigFailed = 46801,
    HandshakeResponseBuildFailed = 46802,
}

#[cfg(test)]
pub(crate) const EXPECTED_IO_CLOSE_KINDS: [std::io::ErrorKind; 4] = {
    use std::io::ErrorKind::*;
    [BrokenPipe, ConnectionReset, ConnectionAborted, NotConnected]
};

#[cfg(test)]
pub(crate) async fn assert_error_stats_async<F: std::future::Future<Output = ()> + Send>(
    fut: F,
    expected: Option<Code>,
) {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::Relaxed},
    };

    use common::util::netsim_logger::{ErrorStatsLayer, ErrorStatsTable};
    use tracing::{Event, Level, Subscriber, instrument::WithSubscriber};
    use tracing_subscriber::{Layer, layer::SubscriberExt};

    struct WarnOrErrorCounter(Arc<AtomicUsize>);
    impl<S: Subscriber> Layer<S> for WarnOrErrorCounter {
        fn on_event(&self, event: &Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
            if *event.metadata().level() <= Level::WARN {
                self.0.fetch_add(1, Relaxed);
            }
        }
    }

    // b/582874820: keep >1 Dispatch alive so tracing_core never caches INTEREST_NEVER.
    let _guard = tracing::Dispatch::new(tracing_subscriber::registry());
    let table = Arc::new(ErrorStatsTable::new());
    let warns = Arc::new(AtomicUsize::new(0));
    let sub = tracing_subscriber::registry()
        .with(ErrorStatsLayer::with_table(Arc::clone(&table)))
        .with(WarnOrErrorCounter(Arc::clone(&warns)));
    fut.with_subscriber(sub).await;
    assert_eq!(table.snapshot(), expected.map(|c| vec![(c as u32, 1)]).unwrap_or_default());
    assert_eq!(warns.load(Relaxed), usize::from(expected.is_some()));
}

#[cfg(test)]
pub(crate) fn write_error_cases(
    code: Code,
) -> Vec<(tungstenite::Message, std::io::ErrorKind, Option<Code>)> {
    use tungstenite::Message::*;
    let other = std::io::ErrorKind::Other;
    let bin = || Binary(bytes::Bytes::from_static(&[1]));
    let mut cases = vec![(bin(), other, Some(code)), (Close(None), other, None)];
    cases.extend(EXPECTED_IO_CLOSE_KINDS.into_iter().map(|k| (bin(), k, None)));
    cases
}

#[cfg(test)]
pub(crate) struct ErrWriteStream(pub(crate) std::io::ErrorKind);

#[cfg(test)]
impl std::io::Read for ErrWriteStream {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

#[cfg(test)]
impl std::io::Write for ErrWriteStream {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from(self.0))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Code;

    // Uniqueness is enforced at compile time by rustc (E0081).
    #[test]
    fn test_codes_in_websocket_server_subband() {
        use Code::*;
        for code in [
            InvalidUpgradeRequest,
            TextFrameAfterBinary,
            TextFrameNonBluetooth,
            TextFrameEmptyBtProps,
            TextFrameParseFailed,
            ListenerAcceptFailed,
            SetStreamBlockingFailed,
            StreamIntoStdFailed,
            StreamCloneFailed,
            HandshakeFailed,
            ClientWriteFailed,
            TransportReadFailed,
            TransportWriteFailed,
            SetupVirtualChipFailed,
            ApplyBtConfigFailed,
            HandshakeResponseBuildFailed,
        ] {
            let val = code as u32;
            assert!((40_001..=49_999).contains(&val), "{code:?} ({val}) outside 40001..=49999");
            assert!((val % 1000) >= 800, "{code:?} ({val}) outside +800..=+999 sub-band");
            // No wildcard: new variants fail to compile here.
            let band = match code {
                InvalidUpgradeRequest
                | TextFrameAfterBinary
                | TextFrameNonBluetooth
                | TextFrameEmptyBtProps
                | TextFrameParseFailed => 40_001..=41_999,
                ListenerAcceptFailed
                | SetStreamBlockingFailed
                | StreamIntoStdFailed
                | StreamCloneFailed => 42_000..=43_999,
                HandshakeFailed | ClientWriteFailed | TransportReadFailed
                | TransportWriteFailed => 44_000..=45_999,
                SetupVirtualChipFailed | ApplyBtConfigFailed | HandshakeResponseBuildFailed => {
                    46_000..=47_999
                }
            };
            assert!(band.contains(&val), "{code:?} ({val}) outside {band:?}");
        }
    }
}
