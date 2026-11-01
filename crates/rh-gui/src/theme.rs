//! The Rusty Host palette.
//!
//! These constants mirror the values in `ui/app.slint`. Keeping them here makes
//! the "Rust red" requirement testable rather than a matter of opinion, and lets
//! non-UI code (for example a future terminal theme) reference the same colours.

/// Rust brand red — the primary accent.
pub const RUST: (u8, u8, u8) = (0xCE, 0x42, 0x2B);
/// Darker red for pressed and hover states.
pub const RUST_DARK: (u8, u8, u8) = (0xA8, 0x34, 0x21);
/// Warm near-black panel background.
pub const PANEL: (u8, u8, u8) = (0x2B, 0x1B, 0x17);
/// Slightly lighter panel for cards and rows.
pub const PANEL_RAISED: (u8, u8, u8) = (0x3A, 0x26, 0x20);
/// Warm off-white text.
pub const TEXT: (u8, u8, u8) = (0xF5, 0xF1, 0xEE);
/// Muted text.
pub const TEXT_DIM: (u8, u8, u8) = (0xB5, 0xA8, 0xA2);
/// A running service.
pub const RUNNING: (u8, u8, u8) = (0x4C, 0xAF, 0x50);
/// A stopped service.
pub const STOPPED: (u8, u8, u8) = (0x7A, 0x6A, 0x64);
/// A failed service.
pub const FAILED: (u8, u8, u8) = (0xE5, 0x39, 0x35);

/// The palette as hex strings, matching the `.slint` file.
pub fn as_hex() -> Vec<(&'static str, String)> {
    vec![
        ("rust", hex(RUST)),
        ("rust-dark", hex(RUST_DARK)),
        ("panel", hex(PANEL)),
        ("panel-raised", hex(PANEL_RAISED)),
        ("text", hex(TEXT)),
        ("text-dim", hex(TEXT_DIM)),
        ("running", hex(RUNNING)),
        ("stopped", hex(STOPPED)),
        ("failed", hex(FAILED)),
    ]
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_accent_is_rust_brand_red() {
        // #CE422B is the Rust logo red. If this changes, the UI is no longer
        // "Rust red" and the requirement has regressed.
        assert_eq!(hex(RUST), "#CE422B");
    }

    #[test]
    fn palette_is_complete_and_valid() {
        let palette = as_hex();
        assert_eq!(palette.len(), 9, "expected 9 palette entries");
        for (name, value) in &palette {
            assert!(
                value.starts_with('#') && value.len() == 7,
                "{name} is not a hex colour: {value}"
            );
            assert!(
                u32::from_str_radix(&value[1..], 16).is_ok(),
                "{name} is not valid hex: {value}"
            );
        }
    }

    #[test]
    fn panel_is_dark_and_text_is_light() {
        let lum = |(r, g, b): (u8, u8, u8)| (r as u32 + g as u32 + b as u32) / 3;
        assert!(lum(PANEL) < 80, "panel must be dark for contrast");
        assert!(lum(TEXT) > 180, "text must be light for contrast");
    }

    #[test]
    fn service_colours_are_distinguishable() {
        assert_ne!(RUNNING, STOPPED);
        assert_ne!(RUNNING, FAILED);
        assert_ne!(STOPPED, FAILED);
    }
}
