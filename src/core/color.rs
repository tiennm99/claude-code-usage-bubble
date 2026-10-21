//! Plain RGB colour helpers shared by the bubble, the panel and the tray badge.
//!
//! The webview takes colours as `#RRGGBB` strings, so [`Rgb::to_hex`] is the
//! boundary conversion.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse `#RRGGBB` or `RRGGBB`. Panics on malformed input — meant for
    /// hardcoded design-system literals at compile-known call sites.
    pub fn from_hex(hex: &str) -> Self {
        Self::parse_hex(hex).unwrap_or_else(|| panic!("invalid hex color literal: {hex:?}"))
    }

    /// Parse `#RRGGBB` or `RRGGBB`. Returns `None` on malformed input.
    pub fn parse_hex(hex: &str) -> Option<Self> {
        let s = hex.trim_start_matches('#');
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(Self { r, g, b })
    }

    /// Format as an uppercase `#RRGGBB` CSS colour.
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Linear interpolation between two colours. `t` is clamped to `[0, 1]`.
    pub fn lerp(self, other: Rgb, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        Rgb {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
        }
    }
}

/// Triangle wave in [0, 1] with a period of 24 ticks: 0 at phase 0, 0.5 at
/// phase 6 and 18, and 1 at phase 12. The bubble UI mirrors this exactly.
pub fn pulse_triangle(phase: u32) -> f64 {
    let p = (phase % 24) as i32;
    let dist = (p - 12).abs(); // 0..12
    1.0 - (dist as f64 / 12.0)
}

/// Linearly brighten `c` toward white by `t` in [0, 1].
pub fn brighten(c: Rgb, t: f64) -> Rgb {
    // Map t to a smaller brightness delta — the pulse should be a subtle nudge.
    let t = t.clamp(0.0, 1.0) * 0.30;
    Rgb::new(
        ((c.r as f64) + (255.0 - c.r as f64) * t).round() as u8,
        ((c.g as f64) + (255.0 - c.g as f64) * t).round() as u8,
        ((c.b as f64) + (255.0 - c.b as f64) * t).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_malformed_input() {
        let c = Rgb::parse_hex("#d97757").expect("valid hex");
        assert_eq!(c, Rgb::new(0xD9, 0x77, 0x57));
        assert_eq!(c.to_hex(), "#D97757");
        assert_eq!(Rgb::parse_hex("1A1A1A"), Some(Rgb::new(0x1A, 0x1A, 0x1A)));
        assert_eq!(Rgb::parse_hex("#12345"), None);
        assert_eq!(Rgb::parse_hex("#GG0000"), None);
        assert_eq!(Rgb::parse_hex("#ééé"), None);
    }

    #[test]
    fn pulse_triangle_peaks_mid_period() {
        assert_eq!(pulse_triangle(0), 0.0);
        assert_eq!(pulse_triangle(6), 0.5);
        assert_eq!(pulse_triangle(12), 1.0);
        assert_eq!(pulse_triangle(18), 0.5);
        assert_eq!(pulse_triangle(24), 0.0);
    }

    #[test]
    fn brighten_caps_at_thirty_percent_toward_white() {
        let c = Rgb::new(0xA0, 0x18, 0x18);
        assert_eq!(brighten(c, 0.0), c);
        // 160 + 95 * 0.3 = 188.5 -> 189; 24 + 231 * 0.3 = 93.3 -> 93
        assert_eq!(brighten(c, 1.0), Rgb::new(189, 93, 93));
        assert_eq!(brighten(c, 5.0), brighten(c, 1.0));
    }

    #[test]
    fn lerp_clamps_t() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(255, 255, 255);
        assert_eq!(a.lerp(b, -1.0), a);
        assert_eq!(a.lerp(b, 2.0), b);
        assert_eq!(a.lerp(b, 0.5), Rgb::new(128, 128, 128));
    }
}
