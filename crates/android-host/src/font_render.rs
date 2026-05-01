//! Device-side font-render harness (M2-S07).
//!
//! Mirrors the canonical Android `FontDB` implementation in
//! `warp-src/crates/warpui/src/platform/android/font.rs` but as a self-
//! contained module so the main-repo `android-host` crate can render
//! "Hello, 世界" on device without taking a cross-workspace `warpui`
//! dependency. The cross-workspace unification is M3 scope; for M2-S07 we
//! verify the rendering pipeline end-to-end on real hardware via this module.
//!
//! ## What it does
//!
//! 1. Discovers system fonts via `ASystemFontIterator` (NDK API 29+, available
//!    unconditionally on minSdk 31 per Plan Amendment 3); falls back to
//!    scanning `/system/fonts/` if the iterator returns null.
//! 2. Loads each face into a `cosmic_text::fontdb::Database` so cosmic-text's
//!    advanced shaping can drive font fallback (Roboto for ASCII, Noto Sans
//!    CJK for CJK, Noto Color Emoji for emoji).
//! 3. Shapes a single line of text via `cosmic_text::ShapeLine::new(...).layout(...)`.
//! 4. Rasterizes each glyph via `cosmic_text::SwashCache::get_image_uncached`.
//! 5. Composites every glyph (alpha-blended, white text) onto a caller-
//!    supplied RGBA buffer at a caller-specified baseline.
//!
//! The composite step proves the glyphs are real on device: the test driver
//! captures the magenta-cleared swapchain frame (M2-S04/S05), passes the
//! buffer through `compose_text_on_rgba`, and saves the PNG. The driver
//! verifies the output PNG contains non-magenta pixels in the expected band
//! around the baseline — concrete evidence that FontDB loaded fonts and
//! TextLayoutSystem shaped glyphs.
//!
//! ## Web-search references consulted (2026-04-30)
//!
//! - AFontMatcher / ASystemFontIterator NDK reference:
//!   <https://developer.android.com/ndk/reference/group/font>
//! - cosmic-text Android empty-DB note:
//!   <https://github.com/pop-os/cosmic-text/issues/243>
//! - Noto fonts on Android:
//!   <https://developer.android.com/about/versions/10/features#noto-fonts>
//! - cosmic-text fork pinned at `15198beba692162201c0ea8b15222cf5643ea068`:
//!   <https://github.com/warpdotdev/cosmic-text>

#![cfg(target_os = "android")]

use std::ffi::CStr;
use std::path::PathBuf;

use cosmic_text::{
    Align, Attrs, AttrsList, BidiParagraphs, CacheKey, CacheKeyFlags, FontSystem, ShapeLine,
    Shaping, SwashCache, SwashContent, Wrap,
};
use fontdb::Source;

/// Discovered system font path + collection index. The `index` field is
/// reserved for future `.ttc` collection-index loading; for now we always
/// pass `Source::File(path)` and let fontdb walk every face in the file.
#[allow(dead_code)]
struct DiscoveredFont {
    path: PathBuf,
    index: u32,
}

/// Classification of a contiguous codepoint run for per-script Attrs span
/// tagging. cosmic-text on Android (`font/fallback/other.rs`) ships empty
/// fallback tables, so we emulate `unix.rs::script_fallback` by classifying
/// CJK / emoji runs and tagging them with the right `Family::Name` hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunKind {
    Latin,
    Cjk,
    Emoji,
}

/// Walk `text` and yield contiguous (byte-range, kind) tuples for the spans
/// that need a non-default Family attribute. The base attrs (Latin) are
/// covered by AttrsList::new(), so we only emit ranges where kind != Latin
/// is needed AND ranges where Latin re-applies after a CJK/emoji segment.
fn classify_text_runs(text: &str) -> Vec<(std::ops::Range<usize>, RunKind)> {
    let mut runs: Vec<(std::ops::Range<usize>, RunKind)> = Vec::new();
    let mut current: Option<(usize, RunKind)> = None;
    for (byte_idx, ch) in text.char_indices() {
        let kind = classify_char(ch);
        match (current, kind) {
            (None, _) => current = Some((byte_idx, kind)),
            (Some((start, prev_kind)), new_kind) if prev_kind == new_kind => {
                let _ = (start,);
            }
            (Some((start, prev_kind)), new_kind) => {
                runs.push((start..byte_idx, prev_kind));
                current = Some((byte_idx, new_kind));
            }
        }
    }
    if let Some((start, kind)) = current {
        runs.push((start..text.len(), kind));
    }
    runs
}

fn classify_char(ch: char) -> RunKind {
    let cp = ch as u32;
    // CJK Unified Ideographs (U+4E00–U+9FFF), Extension A (U+3400–U+4DBF),
    // CJK Symbols/Punctuation (U+3000–U+303F), Hiragana (U+3040–U+309F),
    // Katakana (U+30A0–U+30FF), Bopomofo (U+3100–U+312F), Hangul Jamo
    // (U+1100–U+11FF), Hangul Syllables (U+AC00–U+D7AF), CJK Compatibility
    // (U+F900–U+FAFF, U+FE30–U+FE4F, U+FF00–U+FFEF for halfwidth/fullwidth).
    let is_cjk = matches!(
        cp,
        0x1100..=0x11FF
            | 0x3000..=0x303F
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0x3100..=0x312F
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFFEF
    );
    if is_cjk {
        return RunKind::Cjk;
    }
    // Emoji-presentation ranges (rough — sufficient for "Hello, 世界" path).
    let is_emoji = matches!(
        cp,
        0x1F300..=0x1F6FF | 0x1F900..=0x1F9FF | 0x1FA00..=0x1FAFF | 0x2600..=0x27BF
    );
    if is_emoji {
        return RunKind::Emoji;
    }
    RunKind::Latin
}

/// Use `ASystemFontIterator` to walk every system font. Returns `None` if the
/// iterator is unavailable (extremely unlikely on minSdk 31).
unsafe fn discover_via_iterator() -> Option<Vec<DiscoveredFont>> {
    let iter = ndk_sys::ASystemFontIterator_open();
    if iter.is_null() {
        return None;
    }
    let mut out: Vec<DiscoveredFont> = Vec::new();
    loop {
        let font = ndk_sys::ASystemFontIterator_next(iter);
        if font.is_null() {
            break;
        }
        let path_ptr = ndk_sys::AFont_getFontFilePath(font);
        if !path_ptr.is_null() {
            let cstr = CStr::from_ptr(path_ptr);
            let path = PathBuf::from(cstr.to_string_lossy().into_owned());
            let index = ndk_sys::AFont_getCollectionIndex(font) as u32;
            out.push(DiscoveredFont { path, index });
        }
        ndk_sys::AFont_close(font);
    }
    ndk_sys::ASystemFontIterator_close(iter);
    Some(out)
}

/// Fallback: scan `/system/fonts/*.ttf`/`*.otf`/`*.ttc`.
fn discover_via_dir_scan() -> Vec<DiscoveredFont> {
    let dir = std::path::Path::new("/system/fonts");
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            log::warn!(
                target: "WarpFont",
                "/system/fonts read_dir failed: {e}"
            );
            return Vec::new();
        }
    };
    let mut out: Vec<DiscoveredFont> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let ext_ok = match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => matches!(
                ext.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc"
            ),
            None => false,
        };
        if !ext_ok {
            continue;
        }
        out.push(DiscoveredFont { path, index: 0 });
    }
    out
}

/// Result of rendering a single text string: which fonts were loaded + how
/// many glyphs ended up shaped into the line + how many pixels we touched
/// during composition.
#[derive(Debug, Clone, Default)]
pub(crate) struct FontRenderStats {
    pub via: &'static str,
    pub fonts_loaded: usize,
    pub families_loaded: usize,
    pub primary_family: Option<String>,
    pub cjk_family: Option<String>,
    pub glyphs_total: usize,
    pub glyphs_missing: usize,
    pub composed_pixels: u64,
}

/// Build a `FontSystem` populated from `/system/fonts`. The returned tuple is
/// `(system, primary_family, cjk_family, emoji_family, fonts_loaded, via)`.
///
/// V1-prep: emoji_family is detected separately from the hardcoded
/// "Noto Color Emoji" — on Samsung devices, NotoColorEmoji.ttf ships
/// COLR v1 (which swash 0.1.x can't decode → falls through to monochrome
/// outline) but SamsungColorEmoji.ttf has CBDT/CBLC bitmaps (which swash
/// 0.1.x DOES decode). Routing emoji glyphs to Samsung's font on Samsung
/// devices yields actual color rendering for free. On non-Samsung devices,
/// falls back to Noto Color Emoji (still monochrome until upstream fixes
/// the COLR v1 decoder, but the routing logic doesn't care).
fn build_font_system() -> (
    FontSystem,
    Option<String>,
    Option<String>,
    Option<String>,
    usize,
    &'static str,
) {
    let mut db = fontdb::Database::new();
    let (discovered, via) = match unsafe { discover_via_iterator() } {
        Some(v) => (v, "ASystemFontIterator"),
        None => (discover_via_dir_scan(), "/system/fonts"),
    };
    let total = discovered.len();
    let mut loaded: usize = 0;
    let mut primary_family: Option<String> = None;
    let mut cjk_family: Option<String> = None;
    let mut emoji_family: Option<String> = None;
    let mut all_family_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for d in discovered {
        let ids = db.load_font_source(Source::File(d.path));
        if ids.is_empty() {
            continue;
        }
        loaded += ids.len();
        // Pick a primary (Roboto / Noto Sans Latin) and a CJK fallback to log.
        for id in ids {
            let face = match db.face(id) {
                Some(f) => f,
                None => continue,
            };
            for (family_name, _lang) in &face.families {
                all_family_names.insert(family_name.clone());
                let lower = family_name.to_ascii_lowercase();
                if primary_family.is_none()
                    && (lower == "roboto" || lower == "noto sans" || lower == "sans-serif")
                {
                    primary_family = Some(family_name.clone());
                }
                if cjk_family.is_none()
                    && (lower.contains("noto sans cjk")
                        || lower.contains("noto sans mono cjk")
                        || lower.contains("source han"))
                {
                    cjk_family = Some(family_name.clone());
                }
                // V1-prep: prefer Samsung Color Emoji on Samsung devices —
                // its CBDT/CBLC bitmaps decode under swash 0.1.x where
                // Noto Color Emoji's COLR v1 doesn't. Detection by name
                // matches "Samsung Color Emoji", "SEC Color Emoji",
                // "SamsungColorEmoji" etc.
                if emoji_family.is_none()
                    && lower.contains("samsung")
                    && lower.contains("emoji")
                {
                    emoji_family = Some(family_name.clone());
                }
            }
        }
    }
    // Fallback: prefer any *bitmap-based* color emoji family if Samsung's
    // wasn't found. Currently just "Noto Color Emoji" — caller logs which
    // path was taken so M4-S14-style verification can confirm.
    if emoji_family.is_none() {
        for name in &all_family_names {
            let lower = name.to_ascii_lowercase();
            if lower.contains("noto") && lower.contains("emoji") {
                emoji_family = Some(name.clone());
                break;
            }
        }
    }
    // If we still didn't find a CJK family, scan for any family whose face
    // covers a Han codepoint. Prefer Simplified Chinese ("SC") since the
    // M2-S07 acceptance test uses 世界 (Simplified). Falls back to any
    // CJK / Hans / Hant variant. Restricts the search to family names
    // containing "CJK" / "Hans" / "Hant" so we don't accidentally pick a
    // non-CJK family like Samsung Myanmar (Burmese).
    if cjk_family.is_none() {
        // First pass: prefer SC.
        for face in db.faces() {
            for (name, _lang) in &face.families {
                let lower = name.to_ascii_lowercase();
                let has_cjk_marker = lower.contains("cjk")
                    || lower.contains("hans")
                    || lower.contains("hant");
                if has_cjk_marker && (lower.ends_with(" sc") || lower.contains("hans")) {
                    cjk_family = Some(name.clone());
                    break;
                }
            }
            if cjk_family.is_some() {
                break;
            }
        }
    }
    if cjk_family.is_none() {
        // Second pass: any CJK variant.
        for face in db.faces() {
            for (name, _lang) in &face.families {
                let lower = name.to_ascii_lowercase();
                let has_cjk_marker = lower.contains("cjk")
                    || lower.contains("hans")
                    || lower.contains("hant");
                if has_cjk_marker {
                    cjk_family = Some(name.clone());
                    break;
                }
            }
            if cjk_family.is_some() {
                break;
            }
        }
    }
    let mut all_sorted: Vec<&String> = all_family_names.iter().collect();
    all_sorted.sort();
    let cjk_match: Vec<&&String> = all_sorted
        .iter()
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            l.contains("cjk") || l.contains("han ") || l.contains("hans") || l.contains("hant")
        })
        .collect();
    log::info!(
        target: "warp-android-host",
        "font_db family probe: total_unique={} cjk_match_count={} cjk_match={:?}",
        all_family_names.len(),
        cjk_match.len(),
        cjk_match
    );
    // Set CSS generic families so cosmic-text's `Family::SansSerif` /
    // `Family::Monospace` defaults resolve to Roboto + the closest mono. On
    // Android there's no fontconfig.xml to consult; we hard-wire the Android
    // platform defaults documented at
    // <https://source.android.com/docs/core/display/fontconfig> §"Font fallback"
    // ("Roboto" is the canonical system sans-serif since API 21+).
    if let Some(p) = &primary_family {
        db.set_sans_serif_family(p.clone());
    }
    db.set_monospace_family("Droid Sans Mono");
    db.set_serif_family("Noto Serif");
    log::info!(
        target: "WarpFont",
        "discovered={} loaded={} via={} primary={:?} cjk={:?} emoji={:?}",
        total,
        loaded,
        via,
        primary_family,
        cjk_family,
        emoji_family
    );
    let system = FontSystem::new_with_locale_and_db("en-US".to_string(), db);
    (system, primary_family, cjk_family, emoji_family, loaded, via)
}

/// Composite shaped glyphs of `text` onto an RGBA buffer at the given baseline.
///
/// `rgba` is a `width * height * 4` buffer (RGBA8 little-endian, premultiplied
/// alpha not assumed). The function rasterizes each glyph via swash, then
/// alpha-blends the result onto `rgba` at `(baseline_x + glyph.x,
/// baseline_y + glyph.y)`. Text is rendered as **white** (RGB = 0xFF) so the
/// driver can detect glyph coverage by looking for shifts away from the
/// magenta clear color.
///
/// Returns FontRenderStats summarising fonts loaded + glyphs shaped + pixels
/// touched.
pub(crate) fn compose_text_on_rgba(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    text: &str,
    font_size_px: f32,
    baseline_x: f32,
    baseline_y: f32,
) -> FontRenderStats {
    let (mut system, primary_family, cjk_family, emoji_family, fonts_loaded, via) = build_font_system();
    if fonts_loaded == 0 {
        log::error!(
            target: "WarpFont",
            "compose_text_on_rgba: no fonts loaded; skipping shape"
        );
        return FontRenderStats {
            via,
            fonts_loaded,
            families_loaded: 0,
            primary_family,
            cjk_family,
            glyphs_total: 0,
            glyphs_missing: 0,
            composed_pixels: 0,
        };
    }

    // Count distinct families for diagnostics.
    let families_loaded: usize = {
        let mut set = std::collections::HashSet::new();
        for face in system.db().faces() {
            for (name, _lang) in &face.families {
                set.insert(name.clone());
            }
        }
        set.len()
    };

    // Shape the line. Mirrors warpui/src/windowing/winit/fonts.rs:951-997.
    let combined = BidiParagraphs::new(text)
        .collect::<Vec<&str>>()
        .join("\u{200B}");

    // Build a per-range AttrsList so each script gets the right Family hint.
    // cosmic-text on Android uses `font/fallback/other.rs` which has *empty*
    // fallback tables (no script-aware Han/Bopomofo/Hangul lookups). We
    // emulate the unix `script_fallback` table here by tagging contiguous
    // CJK / emoji runs with `Family::Name("Noto Sans CJK SC")` /
    // `Family::Name("Noto Color Emoji")`. ASCII / Latin runs stay tagged
    // with the primary sans-serif family resolved earlier.
    //
    // Refs:
    //   <https://github.com/pop-os/cosmic-text/blob/15198be/src/font/fallback/other.rs>
    //   <https://github.com/pop-os/cosmic-text/blob/15198be/src/font/fallback/unix.rs>
    let primary_owned = primary_family.clone();
    let cjk_owned = cjk_family.clone();
    let emoji_owned = emoji_family.clone();
    let primary_str = primary_owned.as_deref().unwrap_or("Roboto");
    // Pick a CJK family if discovered. Otherwise default to "Noto Sans CJK SC"
    // — fontdb's `Family::Name` lookup will fail gracefully (returning the
    // first family in DB) if the device doesn't ship one, so the empty-DB
    // path stays graceful.
    let cjk_str = cjk_owned.as_deref().unwrap_or("Noto Sans CJK SC");
    // V1-prep: emoji_family is "Samsung Color Emoji" on Samsung devices
    // (CBDT/CBLC — color decodable by swash 0.1.x), or "Noto Color Emoji"
    // on stock Android (COLR v1 — currently rasterizes monochrome until
    // upstream cosmic-text absorbs swash 0.2). See M4-S14 post-close diag.
    let emoji_str = emoji_owned.as_deref().unwrap_or("Noto Color Emoji");
    log::info!(
        target: "warp-android-host",
        "compose_text_on_rgba: using primary='{}' cjk='{}' emoji='{}'",
        primary_str,
        cjk_str,
        emoji_str
    );
    // Diagnostic: enumerate faces matching the chosen CJK family so we can
    // verify the lookup will succeed at shape time.
    let cjk_face_count = system
        .db()
        .faces()
        .filter(|face| face.families.iter().any(|(n, _)| n == cjk_str))
        .count();
    log::info!(
        target: "warp-android-host",
        "compose_text_on_rgba: cjk_face_count={} for family='{}'",
        cjk_face_count, cjk_str
    );
    let primary_attrs = Attrs::new().family(cosmic_text::Family::Name(primary_str));
    let cjk_attrs = Attrs::new().family(cosmic_text::Family::Name(cjk_str));
    let emoji_attrs = Attrs::new().family(cosmic_text::Family::Name(emoji_str));

    let mut attrs_list = AttrsList::new(primary_attrs);
    let runs_classified = classify_text_runs(combined.as_str());
    log::info!(
        target: "warp-android-host",
        "compose_text_on_rgba: text={:?} bytes={} runs_classified={:?}",
        combined,
        combined.len(),
        runs_classified
    );
    for (range, kind) in runs_classified {
        match kind {
            RunKind::Latin => {} // already covered by the default primary_attrs
            RunKind::Cjk => attrs_list.add_span(range, cjk_attrs),
            RunKind::Emoji => attrs_list.add_span(range, emoji_attrs),
        }
    }

    let shape_line = ShapeLine::new(&mut system, combined.as_str(), &attrs_list, Shaping::Advanced, 4);
    let layout = shape_line.layout(font_size_px, Some(width as f32), Wrap::None, Some(Align::Left), None, None);
    let Some(first_line) = layout.into_iter().next() else {
        log::error!(
            target: "WarpFont",
            "compose_text_on_rgba: ShapeLine produced zero LayoutLines"
        );
        return FontRenderStats {
            via,
            fonts_loaded,
            families_loaded,
            primary_family,
            cjk_family,
            glyphs_total: 0,
            glyphs_missing: 0,
            composed_pixels: 0,
        };
    };

    let glyphs_total = first_line.glyphs.len();
    let mut glyphs_missing: usize = 0;
    let mut composed_pixels: u64 = 0;

    let mut swash_cache = SwashCache::new();
    for glyph in &first_line.glyphs {
        if glyph.glyph_id == 0 {
            glyphs_missing += 1;
        }
        let cache_key = CacheKey::new(
            glyph.font_id,
            glyph.glyph_id,
            font_size_px,
            (0., 0.),
            CacheKeyFlags::empty(),
        )
        .0;
        let image = match swash_cache.get_image_uncached(&mut system, cache_key) {
            Some(img) => img,
            None => continue,
        };
        if image.placement.width == 0 || image.placement.height == 0 {
            continue;
        }
        // Glyph bitmap origin within the destination buffer:
        //   x = baseline_x + glyph.x + image.placement.left
        //   y = baseline_y + glyph.y - image.placement.top
        let dst_x_origin = (baseline_x + glyph.x + image.placement.left as f32) as i32;
        let dst_y_origin = (baseline_y + glyph.y - image.placement.top as f32) as i32;

        // V1-prep: log the swash content variant so we can diagnose
        // why M4-S14's emoji raster came back grayscale despite the
        // Color blit path existing. If all glyphs report Mask, swash
        // isn't extracting COLR/CBDT layers — likely a feature-flag
        // or font-table issue (some "Noto Color Emoji" builds ship
        // alpha-only on certain OEM ROMs). If glyphs report Color
        // but pixels are still gray, the blit_color_rgba blend is
        // wrong (premultiplied vs straight alpha).
        let content_kind = match image.content {
            SwashContent::Mask => "Mask",
            SwashContent::SubpixelMask => "SubpixelMask",
            SwashContent::Color => "Color",
        };
        match image.content {
            SwashContent::Mask | SwashContent::SubpixelMask => {
                composed_pixels += blit_mask_white(
                    rgba,
                    width,
                    height,
                    &image.data,
                    image.placement.width,
                    image.placement.height,
                    dst_x_origin,
                    dst_y_origin,
                );
            }
            SwashContent::Color => {
                composed_pixels += blit_color_rgba(
                    rgba,
                    width,
                    height,
                    &image.data,
                    image.placement.width,
                    image.placement.height,
                    dst_x_origin,
                    dst_y_origin,
                );
            }
        }
        // Compact per-glyph trace; only emit at log level Debug to
        // avoid spamming logcat in production. M4-S14 retest will
        // grep for "swash_glyph" lines.
        log::debug!(
            target: "WarpFont",
            "swash_glyph content={} placement={}x{} bytes={}",
            content_kind,
            image.placement.width,
            image.placement.height,
            image.data.len()
        );
    }

    log::info!(
        target: "WarpFont",
        "compose_text_on_rgba: glyphs_total={} glyphs_missing={} composed_pixels={} via={} fonts={}",
        glyphs_total,
        glyphs_missing,
        composed_pixels,
        via,
        fonts_loaded
    );

    FontRenderStats {
        via,
        fonts_loaded,
        families_loaded,
        primary_family,
        cjk_family,
        glyphs_total,
        glyphs_missing,
        composed_pixels,
    }
}

/// Alpha-blend a swash A8 mask onto the RGBA dest using white text.
///
/// Returns the number of dest pixels actually touched (a > 0 in source).
fn blit_mask_white(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    src: &[u8],
    src_w: u32,
    src_h: u32,
    x0: i32,
    y0: i32,
) -> u64 {
    let mut touched: u64 = 0;
    for sy in 0..src_h as i32 {
        let dy = y0 + sy;
        if dy < 0 || dy as u32 >= dst_h {
            continue;
        }
        for sx in 0..src_w as i32 {
            let dx = x0 + sx;
            if dx < 0 || dx as u32 >= dst_w {
                continue;
            }
            let src_idx = (sy as usize) * (src_w as usize) + (sx as usize);
            let alpha = match src.get(src_idx) {
                Some(b) => *b,
                None => continue,
            };
            if alpha == 0 {
                continue;
            }
            let dst_idx = ((dy as usize) * (dst_w as usize) + (dx as usize)) * 4;
            if dst_idx + 3 >= dst.len() {
                continue;
            }
            // Linear-space alpha blend toward white:
            //   out = src.color * src.alpha + dst * (1 - src.alpha)
            // src.color = (255, 255, 255), so the math collapses to:
            //   out = src.alpha + dst * (1 - src.alpha)
            let a_n = alpha as u32;
            let inv = 255u32 - a_n;
            let r = dst[dst_idx] as u32;
            let g = dst[dst_idx + 1] as u32;
            let b = dst[dst_idx + 2] as u32;
            dst[dst_idx]     = ((255 * a_n + r * inv) / 255) as u8;
            dst[dst_idx + 1] = ((255 * a_n + g * inv) / 255) as u8;
            dst[dst_idx + 2] = ((255 * a_n + b * inv) / 255) as u8;
            // Leave dst alpha at 255 (opaque framebuffer).
            dst[dst_idx + 3] = 255;
            touched += 1;
        }
    }
    touched
}

/// Alpha-blend a swash color (RGBA premultiplied) bitmap onto the RGBA dest.
fn blit_color_rgba(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    src: &[u8],
    src_w: u32,
    src_h: u32,
    x0: i32,
    y0: i32,
) -> u64 {
    let mut touched: u64 = 0;
    for sy in 0..src_h as i32 {
        let dy = y0 + sy;
        if dy < 0 || dy as u32 >= dst_h {
            continue;
        }
        for sx in 0..src_w as i32 {
            let dx = x0 + sx;
            if dx < 0 || dx as u32 >= dst_w {
                continue;
            }
            let src_idx = ((sy as usize) * (src_w as usize) + (sx as usize)) * 4;
            if src_idx + 3 >= src.len() {
                continue;
            }
            let a = src[src_idx + 3] as u32;
            if a == 0 {
                continue;
            }
            let inv = 255u32 - a;
            let dst_idx = ((dy as usize) * (dst_w as usize) + (dx as usize)) * 4;
            if dst_idx + 3 >= dst.len() {
                continue;
            }
            // swash returns premultiplied RGBA in colour mode.
            let dr = dst[dst_idx] as u32;
            let dg = dst[dst_idx + 1] as u32;
            let db = dst[dst_idx + 2] as u32;
            dst[dst_idx]     = (src[src_idx] as u32 + dr * inv / 255).min(255) as u8;
            dst[dst_idx + 1] = (src[src_idx + 1] as u32 + dg * inv / 255).min(255) as u8;
            dst[dst_idx + 2] = (src[src_idx + 2] as u32 + db * inv / 255).min(255) as u8;
            dst[dst_idx + 3] = 255;
            touched += 1;
        }
    }
    touched
}
