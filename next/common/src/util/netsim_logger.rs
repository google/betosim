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

#[cfg(test)]
use tracing::info;
use tracing_log::LogTracer;
#[cfg(not(feature = "cuttlefish"))]
use tracing_subscriber::EnvFilter;

/// Formats the current time for logging.
fn log_current_time() -> String {
    chrono::Utc::now().format("%m-%d %H:%M:%S%.3f").to_string()
}

/// Initiating the environment for logging with given prefix
///
/// The current log format follows the same format as Android Emulator team.
pub fn init(prefix: &'static str, is_verbose: bool) {
    let _ = LogTracer::init();

    let builder = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .event_format(CustomFormatter { prefix });

    #[cfg(not(feature = "cuttlefish"))]
    {
        let log_filter = if is_verbose { "debug" } else { "info" };
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_filter));
        let _ = builder.with_env_filter(filter).try_init();
    }

    #[cfg(feature = "cuttlefish")]
    {
        let level = if is_verbose { tracing::Level::DEBUG } else { tracing::Level::INFO };
        let _ = builder.with_max_level(level).try_init();
    }
}

/// Initiating the environment for logging in Rust unit tests
///
/// The current log format follows the same format as Android Emulator team.
pub fn init_for_test() {
    let _ = LogTracer::init();

    let builder = tracing_subscriber::fmt()
        .event_format(CustomFormatter { prefix: "netsim-test" })
        .with_test_writer();

    #[cfg(not(feature = "cuttlefish"))]
    {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        let _ = builder.with_env_filter(filter).try_init();
    }

    #[cfg(feature = "cuttlefish")]
    {
        let _ = builder.with_max_level(tracing::Level::INFO).try_init();
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
        let meta = event.metadata();
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
            struct Visitor<'a, 'b> {
                writer: &'a mut tracing_subscriber::fmt::format::Writer<'b>,
                result: std::fmt::Result,
            }
            impl tracing::field::Visit for Visitor<'_, '_> {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    if field.name() == "message" && self.result.is_ok() {
                        self.result = write!(self.writer, "{:?}", value);
                    }
                }
            }
            let mut visitor = Visitor { writer: &mut writer, result: Ok(()) };
            event.record(&mut visitor);
            visitor.result?;
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
    use std::sync::Mutex;

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
}
