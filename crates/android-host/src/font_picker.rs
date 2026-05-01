//! Pure font-family selection helpers — host-runnable (no `cfg(target_os = "android")`
//! gate), so we get unit-test coverage for the OEM-naming-variation logic
//! without device dependence.
//!
//! The font_render module proper is android-only because it pulls in
//! cosmic-text + ndk_sys, but the pure pick-by-name selection logic is
//! useful to test on the dev host CI lane.

/// V1-prep emoji-family picker. Given a list of font-family names that
/// fontdb has loaded, return the preferred emoji family for the
/// rasterizer to use, or `None` if no emoji-related family is found.
///
/// ## Preference order (best swash 0.1.x compatibility first)
///
/// 1. Any `Samsung*Emoji*` family — Samsung devices ship a CBDT/CBLC
///    bitmap-based color emoji font; swash 0.1.x decodes these via
///    `Source::ColorBitmap`, yielding `SwashContent::Color` glyphs.
/// 2. Any `Noto*Emoji*` family — the Android stock emoji font; on
///    modern Android (15+) this ships COLR v1 which swash 0.1.x
///    cannot decode → falls through to a monochrome alpha mask.
///    Selecting it here is still the right thing on non-Samsung
///    devices because the alternative is "no emoji glyph at all".
/// 3. None — no emoji-related family found; cosmic-text's own
///    fallback chain takes over (typically picks the first family
///    in DB, which won't have emoji coverage).
///
/// Case-insensitive substring match. Names are matched as opaque
/// strings — fontdb may surface OEM-specific naming variants
/// ("SamsungColorEmoji", "SEC Color Emoji", "Samsung One UI Emoji",
/// etc.) — the heuristic stays loose enough to catch them all.
pub fn pick_emoji_family(names: &[&str]) -> Option<String> {
    // First pass: prefer Samsung's CBDT-based emoji where available.
    for name in names {
        let lower = name.to_ascii_lowercase();
        if lower.contains("samsung") && lower.contains("emoji") {
            return Some((*name).to_string());
        }
    }
    // Second pass: fall through to Noto Color Emoji (or any Noto-family
    // emoji variant — older Android shipped "Noto Emoji" as monochrome
    // before "Noto Color Emoji" became standard).
    for name in names {
        let lower = name.to_ascii_lowercase();
        if lower.contains("noto") && lower.contains("emoji") {
            return Some((*name).to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_samsung_when_present() {
        let names = vec!["Roboto", "SamsungColorEmoji", "Noto Color Emoji"];
        assert_eq!(pick_emoji_family(&names).as_deref(), Some("SamsungColorEmoji"));
    }

    #[test]
    fn falls_back_to_noto_on_pixel() {
        // Pixel-class device: Roboto + Noto Color Emoji (no Samsung font).
        let names = vec!["Roboto", "Noto Sans CJK SC", "Noto Color Emoji"];
        assert_eq!(pick_emoji_family(&names).as_deref(), Some("Noto Color Emoji"));
    }

    #[test]
    fn returns_none_when_no_emoji_font() {
        // Bare-bones device: only Latin + CJK fonts.
        let names = vec!["Roboto", "Droid Sans Mono"];
        assert!(pick_emoji_family(&names).is_none());
    }

    #[test]
    fn case_insensitive_match() {
        // OEM-vendor font naming may use any case style.
        let names = vec!["samsungcoloremoji"];
        assert_eq!(pick_emoji_family(&names).as_deref(), Some("samsungcoloremoji"));
    }

    #[test]
    fn handles_naming_variants() {
        // Various plausible Samsung naming styles. All should match.
        for variant in [
            "Samsung Color Emoji",     // space-separated
            "SamsungColorEmoji",       // PascalCase compound
            "SEC Samsung Emoji",       // SEC prefix variant
            "Samsung One UI Emoji",    // One UI branding variant
        ] {
            let names = vec![variant];
            assert_eq!(
                pick_emoji_family(&names).as_deref(),
                Some(variant),
                "should detect Samsung emoji variant: {}",
                variant
            );
        }
    }

    #[test]
    fn skips_non_emoji_samsung_fonts() {
        // "Samsung Sans" must NOT match — only emoji fonts.
        let names = vec!["Samsung Sans", "Samsung Color Emoji"];
        assert_eq!(pick_emoji_family(&names).as_deref(), Some("Samsung Color Emoji"));

        // ...and if Samsung Color Emoji isn't there, Samsung Sans alone
        // should NOT be picked as the emoji family.
        let names_no_emoji = vec!["Samsung Sans", "Samsung Mono"];
        assert!(pick_emoji_family(&names_no_emoji).is_none());
    }

    #[test]
    fn skips_non_emoji_noto_fonts() {
        // "Noto Sans" must NOT match the Noto-emoji branch.
        let names = vec!["Noto Sans", "Noto Sans CJK SC"];
        assert!(pick_emoji_family(&names).is_none());
    }

    #[test]
    fn first_match_wins_within_tier() {
        // If a device somehow has multiple Samsung emoji variants,
        // the first one wins (deterministic per fontdb iteration order).
        let names = vec!["SamsungColorEmoji", "Samsung Backup Emoji"];
        assert_eq!(pick_emoji_family(&names).as_deref(), Some("SamsungColorEmoji"));
    }

    #[test]
    fn samsung_priority_beats_noto_in_mixed_list() {
        // Order in input shouldn't matter: even if Noto comes first,
        // Samsung wins.
        let names = vec!["Noto Color Emoji", "Roboto", "SamsungColorEmoji"];
        assert_eq!(pick_emoji_family(&names).as_deref(), Some("SamsungColorEmoji"));
    }
}
