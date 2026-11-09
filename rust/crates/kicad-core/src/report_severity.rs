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

//! Port of `include/widgets/report_severity.h` and `common/widgets/report_severity.cpp`
//! (`SEVERITY`, `formatSeverities`), together with the severity/string conversions
//! `SeverityFromString` / `SeverityToString` from `common/widgets/ui_common.cpp` and
//! `RC_ITEM::getSeverityString` from `common/rc_item.cpp`.

/// Port of `enum SEVERITY`.
///
/// The values are bit flags (use `as i32` or [`Severity::as_i32`]); sets of severities are
/// passed around as `int` bit masks in C++ and as `i32` in the port.
///
/// Note: On windows, SEVERITY_ERROR collides with a system declaration, so C++ uses
/// `RPT_SEVERITY_xxx` instead of `SEVERITY_xxx`; `RPT_SEVERITY_ERROR` is `Severity::Error`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum Severity {
    Undefined = 0x01,
    Info = 0x02,
    Exclusion = 0x04,
    Action = 0x08,
    Warning = 0x10,
    Error = 0x20,
    Ignore = 0x40,
    Debug = 0x80,
}

impl Severity {
    /// The numeric (bit flag) value of the severity.
    #[inline]
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    /// Convert a numeric `SEVERITY` value back into the enum; `None` when the value is not
    /// exactly one of the enumerators.
    pub const fn from_i32(value: i32) -> Option<Severity> {
        Some(match value {
            0x01 => Severity::Undefined,
            0x02 => Severity::Info,
            0x04 => Severity::Exclusion,
            0x08 => Severity::Action,
            0x10 => Severity::Warning,
            0x20 => Severity::Error,
            0x40 => Severity::Ignore,
            0x80 => Severity::Debug,
            _ => return None,
        })
    }
}

impl From<Severity> for i32 {
    fn from(severity: Severity) -> i32 {
        severity as i32
    }
}

/// Port of `formatSeverities`: convert a severity mask to a human-readable comma-separated
/// string like "Errors, Warnings".
pub fn format_severities(severities: i32) -> String {
    let mut items: Vec<&str> = Vec::new();

    if (severities & Severity::Error as i32) != 0 {
        items.push("Errors");
    }

    if (severities & Severity::Warning as i32) != 0 {
        items.push("Warnings");
    }

    if (severities & Severity::Exclusion as i32) != 0 {
        items.push("Exclusions");
    }

    if items.is_empty() {
        return "None".to_string();
    }

    items.join(", ")
}

/// Port of `SeverityFromString` (`common/widgets/ui_common.cpp`): "warning" and "ignore" map
/// to their severities, anything else is an error.
pub fn severity_from_string(severity: &str) -> Severity {
    if severity == "warning" {
        Severity::Warning
    } else if severity == "ignore" {
        Severity::Ignore
    } else {
        Severity::Error
    }
}

/// Port of `SeverityToString` (`common/widgets/ui_common.cpp`): "ignore", "warning" or
/// (for everything else) "error".
pub fn severity_to_string(severity: Severity) -> &'static str {
    if severity == Severity::Ignore {
        "ignore"
    } else if severity == Severity::Warning {
        "warning"
    } else {
        "error"
    }
}

/// Port of `RC_ITEM::getSeverityString` (`common/rc_item.cpp`): the severity name used in
/// DRC/ERC reports; empty for `RPT_SEVERITY_UNDEFINED` and `RPT_SEVERITY_IGNORE`.
pub fn get_severity_string(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Action => "action",
        Severity::Info => "info",
        Severity::Exclusion => "exclusion",
        Severity::Debug => "debug",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(Severity::Undefined as i32, 0x01);
        assert_eq!(Severity::Info as i32, 0x02);
        assert_eq!(Severity::Exclusion as i32, 0x04);
        assert_eq!(Severity::Action as i32, 0x08);
        assert_eq!(Severity::Warning as i32, 0x10);
        assert_eq!(Severity::Error as i32, 0x20);
        assert_eq!(Severity::Ignore as i32, 0x40);
        assert_eq!(Severity::Debug as i32, 0x80);

        for bit in 0..8 {
            let value = 1 << bit;
            assert_eq!(Severity::from_i32(value).map(Severity::as_i32), Some(value));
        }

        assert_eq!(Severity::from_i32(0), None);
        assert_eq!(Severity::from_i32(0x30), None);
        assert_eq!(i32::from(Severity::Error), 0x20);
    }

    #[test]
    fn strings() {
        assert_eq!(format_severities(0), "None");
        assert_eq!(format_severities(Severity::Info as i32), "None");
        assert_eq!(format_severities(Severity::Error as i32), "Errors");
        assert_eq!(
            format_severities(Severity::Error as i32 | Severity::Warning as i32),
            "Errors, Warnings"
        );
        assert_eq!(format_severities(0xFF), "Errors, Warnings, Exclusions");

        assert_eq!(severity_from_string("warning"), Severity::Warning);
        assert_eq!(severity_from_string("ignore"), Severity::Ignore);
        assert_eq!(severity_from_string("error"), Severity::Error);
        assert_eq!(severity_from_string("anything"), Severity::Error);

        assert_eq!(severity_to_string(Severity::Ignore), "ignore");
        assert_eq!(severity_to_string(Severity::Warning), "warning");
        assert_eq!(severity_to_string(Severity::Error), "error");
        assert_eq!(severity_to_string(Severity::Info), "error");

        assert_eq!(get_severity_string(Severity::Error), "error");
        assert_eq!(get_severity_string(Severity::Exclusion), "exclusion");
        assert_eq!(get_severity_string(Severity::Ignore), "");
        assert_eq!(get_severity_string(Severity::Undefined), "");
    }
}
