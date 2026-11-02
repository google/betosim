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

//! # Time Display class

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, Timelike, Utc};

/// A simple class that contains information required to display time
pub struct TimeDisplay {
    /// seconds since std::time::UNIX_EPOCH
    secs: i64,
    /// nano sub seconds since std::time::UNIX_EPOCH
    nsecs: u32,
}

impl TimeDisplay {
    /// Creates a new TimeDisplay with given secs and nsecs
    ///
    /// # Arguments
    ///
    /// * `secs` - seconds since std::time::UNIX_EPOCH
    /// * `nsecs` - nano sub seconds since std::time::UNIX_EPOCH
    pub fn new(secs: i64, nsecs: u32) -> TimeDisplay {
        TimeDisplay { secs, nsecs }
    }

    /// Creates a new TimeDisplay from a Duration since UNIX_EPOCH, saturating seconds at i64::MAX.
    pub fn from_duration(d: Duration) -> TimeDisplay {
        TimeDisplay {
            secs: i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
            nsecs: d.subsec_nanos(),
        }
    }

    /// Resolves the UTC DateTime if seconds and nanoseconds form a valid timestamp.
    fn utc_datetime(&self) -> Option<DateTime<Utc>> {
        DateTime::from_timestamp(self.secs, self.nsecs)
    }

    /// Displays date & time in UTC with a format YYYY-MM-DD-HH:MM:SS
    ///
    /// # Returns
    ///
    /// `String` display of utc time.
    pub fn utc_display(&self) -> String {
        if let Some(current_datetime) = self.utc_datetime() {
            return format!(
                "{}-{:02}-{:02}-{:02}-{:02}-{:02}",
                current_datetime.year(),
                current_datetime.month(),
                current_datetime.day(),
                current_datetime.hour(),
                current_datetime.minute(),
                current_datetime.second()
            );
        }
        "INVALID-TIMESTAMP".to_string()
    }

    /// Displays time in UTC without date with a format HH:MM:SS
    ///
    /// # Returns
    ///
    /// `Ok(String)` if the display was successful, `Error` otherwise.
    pub fn utc_display_hms(&self) -> String {
        if let Some(current_datetime) = self.utc_datetime() {
            return format!(
                "{:02}:{:02}:{:02}",
                current_datetime.hour(),
                current_datetime.minute(),
                current_datetime.second()
            );
        }
        "INVALID".to_string()
    }

    /// Displays time in UTC for logs
    fn utc_display_log(&self) -> String {
        if let Some(current_datetime) = self.utc_datetime() {
            return format!(
                "{:02}-{:02} {:02}:{:02}:{:02}.{:.3}",
                current_datetime.month(),
                current_datetime.day(),
                current_datetime.hour(),
                current_datetime.minute(),
                current_datetime.second(),
                current_datetime.timestamp_subsec_nanos().to_string(),
            );
        }
        "INVALID-TIMESTAMP".to_string()
    }
}

// Get TimeDisplay of current_time
fn get_current_time() -> TimeDisplay {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards");
    TimeDisplay::from_duration(since_epoch)
}

/// Return the timestamp of the current time for logs
pub fn log_current_time() -> String {
    get_current_time().utc_display_log()
}

/// Return the timestamp of the current time for files
pub fn file_current_time() -> String {
    get_current_time().utc_display()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::TimeDisplay;

    #[test]
    fn test_utc_display_ok() {
        let epoch_time = TimeDisplay::new(0, 0);
        let utc_epoch = epoch_time.utc_display();
        assert_eq!(utc_epoch, "1970-01-01-00-00-00");
        let twok_time = TimeDisplay::new(946684900, 0);
        let utc_twok = twok_time.utc_display();
        assert_eq!(utc_twok, "2000-01-01-00-01-40");
    }

    #[test]
    fn test_from_duration() {
        let twok_time = TimeDisplay::from_duration(Duration::new(946684900, 200));
        assert_eq!(twok_time.utc_display(), "2000-01-01-00-01-40");
        assert_eq!(twok_time.utc_display_log(), "01-01 00:01:40.200");
        let saturated = TimeDisplay::from_duration(Duration::MAX);
        assert_eq!(saturated.utc_display(), "INVALID-TIMESTAMP");
    }

    #[test]
    fn test_utc_display_err() {
        let max_seconds = TimeDisplay::new(i64::MAX, 0);
        assert_eq!("INVALID-TIMESTAMP", max_seconds.utc_display());
        let max_nanos = TimeDisplay::new(0, 2_000_000_000);
        assert_eq!("INVALID-TIMESTAMP", max_nanos.utc_display());
    }

    #[test]
    fn test_utc_display_hms() {
        let epoch_time = TimeDisplay::new(0, 0);
        let utc_epoch = epoch_time.utc_display_hms();
        assert_eq!(utc_epoch, "00:00:00");
        let twok_time = TimeDisplay::new(946684900, 0);
        let utc_twok = twok_time.utc_display_hms();
        assert_eq!(utc_twok, "00:01:40");
    }

    #[test]
    fn test_utc_display_log() {
        let epoch_time = TimeDisplay::new(0, 0);
        let utc_epoch = epoch_time.utc_display_log();
        assert_eq!(utc_epoch, "01-01 00:00:00.0");
        let twok_time = TimeDisplay::new(946684900, 200);
        let utc_twok = twok_time.utc_display_log();
        assert_eq!(utc_twok, "01-01 00:01:40.200");
    }
}
