// SPDX-License-Identifier: GPL-3.0-only
//! Display brightness helpers, matching cosmic-osd / cosmic-applet-battery.

/// Lowest raw value the slider allows, so the screen never goes fully black.
#[must_use]
pub fn floor(max: i32) -> i32 {
    if max <= 0 { 0 } else { (max / 100).max(1) }
}

/// Snap a raw brightness to the 20 coarse steps used by COSMIC when the
/// backlight has very few levels; finer backlights are used as-is.
#[must_use]
pub fn snap(raw: i32, max: i32) -> i32 {
    let raw = raw.clamp(floor(max), max.max(0));
    if max > 0 && max <= 20 {
        let (raw, max) = (i64::from(raw), i64::from(max));
        let step = ((raw * 20 + max / 2) / max).clamp(0, 20);
        i32::try_from((step * max + 10) / 20)
            .unwrap_or(0)
            .max(floor(max as i32))
    } else {
        raw
    }
}

/// Brightness as a percentage of `max`.
#[must_use]
pub fn percent(raw: i32, max: i32) -> u8 {
    if max <= 0 {
        return 0;
    }
    u8::try_from((i64::from(raw.clamp(0, max)) * 100 / i64::from(max)).clamp(0, 100)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_reaches_zero() {
        assert_eq!(floor(96_000), 960);
        assert_eq!(floor(10), 1);
        assert_eq!(snap(0, 96_000), 960);
        assert_eq!(snap(0, 10), 1);
    }

    #[test]
    fn coarse_backlights_snap_fine_ones_pass_through() {
        assert_eq!(snap(7, 10), 7);
        assert_eq!(snap(5_123, 96_000), 5_123);
        assert_eq!(snap(999_999, 96_000), 96_000);
    }

    #[test]
    fn percent_is_bounded() {
        assert_eq!(percent(48_000, 96_000), 50);
        assert_eq!(percent(-5, 100), 0);
        assert_eq!(percent(5, 0), 0);
    }
}
