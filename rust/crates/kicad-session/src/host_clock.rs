// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! The wall-clock values the C++ jobs read with `wxDateTime::Now()`, as one host-supplied
//! value (no clock in library crates): `GetISO8601CurrentDateTime()` (DRC report date), the
//! `CURRENT_DATE` / `CURRENT_TIME_*` text variables (`common/title_block.cpp`
//! `GetCurrentDate` and friends) and the creation date of the Gerber / drill files
//! (`GBR_METADATA` `%TF.CreationDate`, Excellon header). The hosts build it from their clock
//! (`kicad-cli`: the system time; `kicad-wasm`: `Date`) or from a fixed date for reproducible
//! output.

use kicad_core::title_block::TitleBlockClock;
use kicad_fab::jobs::job_export_pcb_plot::GbrCreationDate;

/// A local date and time with its UTC offset (`wxDateTime::Now()` as the C++ formats it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostClock {
    /// Year, e.g. 2026.
    pub year: i32,
    /// Month 1..12.
    pub month: u32,
    /// Day of month 1..31.
    pub day: u32,
    /// Hour 0..23.
    pub hour: u32,
    /// Minute 0..59.
    pub minute: u32,
    /// Second 0..59.
    pub second: u32,
    /// Offset of the local time zone from UTC in minutes (east positive).
    pub utc_offset_minutes: i32,
}

impl HostClock {
    /// The local time `unix_seconds + utc_offset_minutes * 60` (proleptic Gregorian calendar,
    /// days from civil by H. Hinnant's algorithm).
    pub fn from_unix_seconds(unix_seconds: i64, utc_offset_minutes: i32) -> HostClock {
        let local = unix_seconds + i64::from(utc_offset_minutes) * 60;
        let days = local.div_euclid(86_400);
        let secs = local.rem_euclid(86_400);

        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);

        HostClock {
            year: year as i32,
            month: month as u32,
            day: day as u32,
            hour: (secs / 3600) as u32,
            minute: (secs % 3600 / 60) as u32,
            second: (secs % 60) as u32,
            utc_offset_minutes,
        }
    }

    /// Parses `YYYY-MM-DDTHH:MM:SS` (the fixed-date option of the hosts; UTC offset 0).
    pub fn parse_iso(text: &str) -> Option<HostClock> {
        let b = text.as_bytes();

        if b.len() != 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' {
            return None;
        }

        if b[16] != b':' {
            return None;
        }

        let num = |r: std::ops::Range<usize>| text.get(r)?.parse::<u32>().ok();

        let clock = HostClock {
            year: num(0..4)? as i32,
            month: num(5..7)?,
            day: num(8..10)?,
            hour: num(11..13)?,
            minute: num(14..16)?,
            second: num(17..19)?,
            utc_offset_minutes: 0,
        };

        let valid = (1..=12).contains(&clock.month)
            && (1..=31).contains(&clock.day)
            && clock.hour < 24
            && clock.minute < 60
            && clock.second < 60;

        valid.then_some(clock)
    }

    /// `GetISO8601CurrentDateTime()`: `wxDateTime::Now().FormatISOCombined( 'T' )`.
    pub fn iso8601(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// The `CURRENT_DATE` / `CURRENT_TIME_*` values (`GetCurrentDate()`: ISO date,
    /// `GetCurrentTimeHHMMSS()`: `%Hh%Mm%Ss`, `GetCurrentTimeLocale()`: `%X` in the C locale).
    pub fn title_block_clock(&self) -> TitleBlockClock {
        TitleBlockClock {
            current_date: format!("{:04}-{:02}-{:02}", self.year, self.month, self.day),
            current_time_hhmmss: format!("{:02}h{:02}m{:02}s", self.hour, self.minute, self.second),
            current_time_locale: format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second),
        }
    }

    /// The creation date of the Gerber / drill writers.
    pub fn gbr_creation_date(&self) -> GbrCreationDate {
        GbrCreationDate {
            year: self.year,
            month: self.month,
            day: self.day,
            hour: self.hour,
            minute: self.minute,
            second: self.second,
            utc_offset_minutes: self.utc_offset_minutes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_unix() {
        let c = HostClock::from_unix_seconds(0, 0);
        assert_eq!(c.iso8601(), "1970-01-01T00:00:00");

        // 2026-11-10T13:45:07Z
        let c = HostClock::from_unix_seconds(1_794_318_307, 0);
        assert_eq!(c.iso8601(), "2026-11-10T13:45:07");

        let c = HostClock::from_unix_seconds(1_794_318_307, 120);
        assert_eq!(c.iso8601(), "2026-11-10T15:45:07");
        assert_eq!(c.title_block_clock().current_time_hhmmss, "15h45m07s");

        // leap day
        let c = HostClock::from_unix_seconds(951_782_400, 0);
        assert_eq!(c.iso8601(), "2000-02-29T00:00:00");
    }

    #[test]
    fn parse_fixed_date() {
        let c = HostClock::parse_iso("2024-05-17T19:23:42").expect("valid");
        assert_eq!(c.iso8601(), "2024-05-17T19:23:42");
        assert!(HostClock::parse_iso("2024-13-17T19:23:42").is_none());
        assert!(HostClock::parse_iso("2024-05-17 19:23:42").is_none());
    }
}
