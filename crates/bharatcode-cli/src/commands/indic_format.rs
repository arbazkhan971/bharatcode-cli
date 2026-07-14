//! Locale-aware number / currency grouping for `bharatcode cost`.
//!
//! By default BharatCode prints rupee amounts and large counts with ordinary
//! Western thousands grouping (groups of three from the right). This module
//! adds an opt-in *Indian* numbering mode — lakh / crore grouping, where the
//! last three digits form one group and every group above that is two digits
//! wide, e.g. `1234567` renders as `12,34,567` rather than `1,234,567`.
//!
//! The mode is gated on `BHARATCODE_NUMFMT=indian`. With the variable unset (or
//! any other value) [`indian_grouping_enabled`] returns `false`, every public
//! helper falls back to the Western grouping it already used, and the rendered
//! strings are byte-identical to the pre-existing output. Nothing here mutates
//! state, performs I/O or depends on a locale beyond that single env switch.
//!
//! All functions are pure (`u64` / `f64` in, `String` out) so they are trivially
//! unit-testable without touching the environment.
//!
//! Original BharatCode work; not ported from any third party.

/// Environment key that selects the numbering system used when rendering rupee
/// amounts and large counts. The only value that switches on Indian (lakh /
/// crore) grouping is `indian` (case-insensitive); absence or anything else
/// leaves Western thousands grouping in place so default output is unchanged.
pub const NUMFMT_KEY: &str = "BHARATCODE_NUMFMT";

/// Whether Indian (lakh / crore) digit grouping is enabled for this process.
///
/// Reads [`NUMFMT_KEY`] as a raw environment string and compares it,
/// case-insensitively and trimmed, against `indian`. Any other value — including
/// absence — is OFF, so callers fall back to Western grouping and existing
/// output stays byte-identical.
pub fn indian_grouping_enabled() -> bool {
    matches!(std::env::var(NUMFMT_KEY), Ok(raw) if is_indian_mode(&raw))
}

/// Whether the raw env value names the Indian numbering mode.
fn is_indian_mode(v: &str) -> bool {
    v.trim().eq_ignore_ascii_case("indian")
}

/// Group an unsigned integer using the Indian numbering system: the final three
/// digits form one group and the remaining (higher) digits are split into
/// two-digit groups from the right, e.g. `1234567` -> `12,34,567`. Values with
/// three or fewer digits are returned unchanged (`999` -> `999`).
pub fn group_indian(n: u64) -> String {
    let digits: Vec<u8> = n.to_string().into_bytes();
    let len = digits.len();
    if len <= 3 {
        return String::from_utf8(digits).expect("ascii digits are valid utf-8");
    }
    // Everything before the trailing group of three; the head is split into
    // 2-digit groups counted from its right edge (a leading group may be a
    // single digit, e.g. `1,00,000`).
    let head_len = len - 3;
    let mut out = String::with_capacity(len + len / 2);
    for (idx, ch) in digits.iter().enumerate() {
        if idx == head_len || (idx > 0 && idx < head_len && (head_len - idx).is_multiple_of(2)) {
            out.push(',');
        }
        out.push(*ch as char);
    }
    out
}

/// Group an unsigned integer using ordinary Western thousands grouping
/// (groups of three from the right), e.g. `1234567` -> `1,234,567`. This is the
/// default rendering used when Indian grouping is not enabled.
pub fn group_western(n: u64) -> String {
    let digits: Vec<u8> = n.to_string().into_bytes();
    let len = digits.len();
    if len <= 3 {
        return String::from_utf8(digits).expect("ascii digits are valid utf-8");
    }
    let mut out = String::with_capacity(len + len / 3);
    for (idx, ch) in digits.iter().enumerate() {
        if idx > 0 && (len - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*ch as char);
    }
    out
}

/// Format a rupee amount with full grouping and two paise digits, using Indian
/// (lakh / crore) grouping for the integer part, e.g. `1234567.0` -> the string
/// `₹12,34,567.00`. Paise are rounded; a carry out of the paise rolls into the
/// rupee (so `0.999` -> `₹1.00`). Non-finite input renders as `₹0.00`.
///
/// The argument is a rupee figure (not paise); the parameter name keeps the
/// public signature explicit about the accepted unit.
pub fn format_inr_indian(paise_or_rupees: f64) -> String {
    format_inr_grouped(paise_or_rupees, true)
}

/// Shared INR formatter. `indian` selects lakh / crore grouping; otherwise the
/// integer part uses Western thousands grouping. Kept private so the public
/// surface stays the two named helpers plus [`format_count`].
fn format_inr_grouped(rupees: f64, indian: bool) -> String {
    if !rupees.is_finite() {
        return "₹0.00".to_string();
    }
    let sign = if rupees < 0.0 { "-" } else { "" };
    let a = rupees.abs();
    let mut whole = a.trunc() as u64;
    let mut paise = (a.fract() * 100.0).round() as u64;
    // Rounding paise can carry into the rupee (e.g. 0.999 -> 1.00).
    if paise >= 100 {
        whole += 1;
        paise -= 100;
    }
    let grouped = if indian {
        group_indian(whole)
    } else {
        group_western(whole)
    };
    format!("{sign}₹{grouped}.{paise:02}")
}

/// Format a plain (non-currency) count such as a token total, grouping the
/// digits either Indian-style (`indian = true`) or Western-style. This lets a
/// caller route a count through the same locale switch as rupee amounts:
/// `format_count(n, indian_grouping_enabled())`.
pub fn format_count(n: u64, indian: bool) -> String {
    if indian {
        group_indian(n)
    } else {
        group_western(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_indian_matches_lakh_crore_spec() {
        assert_eq!(group_indian(1234567), "12,34,567");
        assert_eq!(group_indian(999), "999");
        assert_eq!(group_indian(100000), "1,00,000");
        // Boundaries around the first comma and the head's leading group.
        assert_eq!(group_indian(0), "0");
        assert_eq!(group_indian(1000), "1,000");
        assert_eq!(group_indian(12345), "12,345");
        assert_eq!(group_indian(123456), "1,23,456");
        // Crore-scale value: 1,00,00,00,000.
        assert_eq!(group_indian(10000000000), "10,00,00,00,000");
    }

    #[test]
    fn group_western_is_plain_thousands() {
        assert_eq!(group_western(999), "999");
        assert_eq!(group_western(1000), "1,000");
        assert_eq!(group_western(1234567), "1,234,567");
        assert_eq!(group_western(100000), "100,000");
        assert_eq!(group_western(0), "0");
    }

    #[test]
    fn format_inr_indian_groups_and_keeps_paise() {
        assert_eq!(format_inr_indian(0.0), "₹0.00");
        assert_eq!(format_inr_indian(999.0), "₹999.00");
        assert_eq!(format_inr_indian(1234.5), "₹1,234.50");
        assert_eq!(format_inr_indian(1_234_567.0), "₹12,34,567.00");
        // Paise rounding carries into the rupee.
        assert_eq!(format_inr_indian(0.999), "₹1.00");
        assert_eq!(format_inr_indian(-1234.5), "-₹1,234.50");
        assert!(format_inr_indian(f64::INFINITY) == "₹0.00");
    }

    #[test]
    fn format_count_switches_on_flag() {
        assert_eq!(format_count(1_234_567, true), "12,34,567");
        assert_eq!(format_count(1_234_567, false), "1,234,567");
        assert_eq!(format_count(42, true), "42");
        assert_eq!(format_count(42, false), "42");
    }

    #[test]
    fn enabled_only_for_indian_value() {
        let _guard = env_lock::lock_env([(NUMFMT_KEY, Some("indian"))]);
        assert!(indian_grouping_enabled());
    }

    #[test]
    fn enabled_is_case_insensitive_and_trimmed() {
        let _guard = env_lock::lock_env([(NUMFMT_KEY, Some("  Indian  "))]);
        assert!(indian_grouping_enabled());
    }

    #[test]
    fn disabled_when_unset_or_other_value() {
        {
            let _guard = env_lock::lock_env([(NUMFMT_KEY, None::<&str>)]);
            assert!(!indian_grouping_enabled());
        }
        {
            let _guard = env_lock::lock_env([(NUMFMT_KEY, Some("western"))]);
            assert!(!indian_grouping_enabled());
        }
        {
            let _guard = env_lock::lock_env([(NUMFMT_KEY, Some("1"))]);
            assert!(!indian_grouping_enabled());
        }
    }
}
