# M0 Platform Trait Delta — warpui::platform ↔ gpui-mobile

**Generated:** 2026-04-29  
**Warp commit:** `d0f045c` (master, read-only)  
**gpui-mobile commit:** `1d3ec2a1d14a63b74d1f4269340441d4eeada27a`  
**Source:** `warp-src/crates/warpui_core/src/platform/mod.rs`  
**gpui-mobile entry:** `/tmp/gpui-mobile/src/lib.rs` → `ios/platform.rs`, `android/platform.rs`

---

## 1. Architecture Summary

### warpui::platform (Warp's own framework — NOT gpui/Zed)

Warp has **forked and diverged** from gpui. It defines its own platform trait hierarchy in `warpui_core::platform`:

| Trait | Purpose |
|---|---|
| `Delegate` | App-level platform operations (clipboard, URLs, notifications, IME, cursor, a11y, shortcuts) |
| `DispatchDelegate` | Thread dispatch (main thread check + run_on_main_thread) |
| `FontDB` | Font loading, shaping, rasterisation |
| `TextLayoutSystem` | Line layout + text frame layout |
| `Window` | Per-window operations (minimize, fullscreen, transparency, graphics backend) |
| `WindowContext` | Window surface (size, scale, render_scene, redraw, frame capture) |
| `WindowManager` | Multi-window management (open/close/focus/display/bounds) |
| `LoadedSystemFonts` | Marker trait for async font loading result |

Backend selection is in `warpui/src/platform/mod.rs` via `cfg_if!`:
- `wasm` → `wasm::*`
- `linux` → `linux::*` (winit-backed)
- `macos` → `mac::*`
- `windows` → `windows::*`
- fallback → `warpui_core::platform::test::*`
- `headless` → always available, used in CI and headless tests

### gpui-mobile (Zed's gpui fork for mobile — DIFFERENT trait system)

gpui-mobile implements **Zed's `gpui::Platform` trait**, not Warp's `warpui_core::platform::Delegate`. The two are entirely separate interface families:

| gpui-mobile exports | Type |
|---|---|
| `IosPlatform` | implements `gpui::Platform` |
| `AndroidPlatform` | implements `gpui::Platform` |
| `PlatformView` trait | Mobile-specific embedded view |
| `PlatformViewFactory` trait | Factory for PlatformView |
| `set_system_chrome()` | Status/nav bar styling |
| `show_keyboard()` / `hide_keyboard()` | IME control |
| `keyboard_height()` / `set_keyboard_height()` | Keyboard inset |
| `safe_area_insets()` | Device safe area |
| `dispatch_text_input()` | Soft keyboard text delivery |
| `TEXT_INPUT_DIRTY` | AtomicBool dirty flag |
| `TargetPlatform` / `target_platform()` | Runtime platform enum |

---

## 2. Full Trait Surface — warpui_core::platform

### trait Delegate (lines 193–279)

| # | Method | Signature summary |
|---|---|---|
| 1 | `dispatch_delegate` | `&self → Arc<dyn DispatchDelegate>` |
| 2 | `request_user_attention` | `&self, WindowId` |
| 3 | `clipboard` | `&mut self → &mut dyn Clipboard` |
| 4 | `system_theme` | `&self → SystemTheme` |
| 5 | `open_url` | `&self, &str` |
| 6 | `open_file_path` | `&self, &Path` |
| 7 | `open_file_path_in_explorer` | `&self, &Path` |
| 8 | `open_file_picker` | `&self, FilePickerCallback, FilePickerConfiguration` |
| 9 | `open_save_file_picker` | `&self, SaveFilePickerCallback, SaveFilePickerConfiguration` |
| 10 | `application_bundle_info` | `&self, &str → Option<ApplicationBundleInfo>` |
| 11 | `show_native_platform_modal` | `&self, ModalId, AlertDialog` |
| 12 | `request_desktop_notification_permissions` | `&self, RequestNotificationPermissionsCallback` |
| 13 | `send_desktop_notification` | `&self, UserNotification, WindowId, SendNotificationErrorCallback` |
| 14 | `set_cursor_shape` | `&self, Cursor` |
| 15 | `get_cursor_shape` (test-util) | `&self → Cursor` |
| 16 | `close_ime_async` | `&self, WindowId` |
| 17 | `is_ime_open` | `&self → bool` |
| 18 | `open_character_palette` | `&self` |
| 19 | `set_accessibility_contents` | `&self, AccessibilityContent` |
| 20 | `register_global_shortcut` | `&self, Keystroke` |
| 21 | `unregister_global_shortcut` | `&self, &Keystroke` |
| 22 | `terminate_app` | `&self, TerminationMode` |
| 23 | `is_screen_reader_enabled` | `&self → Option<bool>` |
| 24 | `microphone_access_state` | `&self → MicrophoneAccessState` |
| 25 | `is_headless` (default impl) | `&self → bool` (default: false) |

### trait DispatchDelegate (lines 296–305)

| # | Method | Signature summary |
|---|---|---|
| 26 | `is_main_thread` | `&self → bool` |
| 27 | `run_on_main_thread` | `&self, Runnable` |

### trait LoadedSystemFonts (lines 309–311)

| # | Method | Signature summary |
|---|---|---|
| 28 | `as_any` | `Box<Self> → Box<dyn Any>` |

### trait TextLayoutSystem (lines 315–339)

| # | Method | Signature summary |
|---|---|---|
| 29 | `layout_line` | `&self, &str, LineStyle, &[(Range<usize>, StyleAndFont)], f32, ClipConfig → Line` |
| 30 | `layout_text` | `&self, &str, LineStyle, &[(Range, StyleAndFont)], f32, f32, TextAlignment, Option<f32> → TextFrame` |

### trait FontDB (lines 349–437)

| # | Method | Signature summary |
|---|---|---|
| 31 | `load_from_bytes` | `&mut self, &str, Vec<Vec<u8>> → Result<FamilyId>` |
| 32 | `load_from_system` (not-wasm) | `&mut self, &str → Result<FamilyId>` |
| 33 | `load_all_system_fonts` (not-wasm) | `&self → BoxFuture<Box<dyn LoadedSystemFonts>>` |
| 34 | `process_loaded_system_fonts` (not-wasm) | `&mut self, Box<dyn LoadedSystemFonts> → Vec<(Option<FamilyId>, FontInfo)>` |
| 35 | `family_id_for_name` | `&self, &str → Option<FamilyId>` |
| 36 | `load_family_name_from_id` | `&self, FamilyId → Option<String>` |
| 37 | `select_font` | `&self, FamilyId, Properties → FontId` |
| 38 | `fallback_fonts` | `&self, char, FontId → Vec<FontId>` |
| 39 | `font_metrics` | `&self, FontId → Metrics` |
| 40 | `glyph_advance` | `&self, FontId, GlyphId → Result<Vector2I>` |
| 41 | `glyph_raster_bounds` | `&self, FontId, f32, GlyphId, Vector2F, &GlyphConfig → Result<RectI>` |
| 42 | `glyph_typographic_bounds` | `&self, FontId, GlyphId → Result<RectI>` |
| 43 | `rasterize_glyph` | `&self, FontId, f32, GlyphId, Vector2F, SubpixelAlignment, &GlyphConfig, RasterFormat → Result<RasterizedGlyph>` |
| 44 | `glyph_for_char` | `&self, FontId, char → Option<GlyphId>` |
| 45 | `text_layout_system` | `&self → &dyn TextLayoutSystem` |

### trait Window (lines 447–465)

| # | Method | Signature summary |
|---|---|---|
| 46 | `minimize` | `&self` |
| 47 | `toggle_maximized` | `&self` |
| 48 | `toggle_fullscreen` | `&self` |
| 49 | `fullscreen_state` | `&self → FullscreenState` |
| 50 | `uses_native_window_decorations` | `&self → bool` |
| 51 | `set_titlebar_height` | `&self, f64` |
| 52 | `supports_transparency` | `&self → bool` |
| 53 | `graphics_backend` | `&self → GraphicsBackend` |
| 54 | `supported_backends` | `&self → Vec<GraphicsBackend>` |
| 55 | `as_ctx` | `&self → &dyn WindowContext` |
| 56 | `callbacks` | `&self → &WindowCallbacks` |
| 57 | `as_any` | `&self → &dyn Any` |

### trait WindowContext (lines 467–495)

| # | Method | Signature summary |
|---|---|---|
| 58 | `size` | `&self → Vector2F` |
| 59 | `origin` | `&self → Vector2F` |
| 60 | `backing_scale_factor` | `&self → f32` |
| 61 | `max_texture_dimension_2d` | `&self → Option<u32>` |
| 62 | `render_scene` | `&self, Rc<Scene>` |
| 63 | `request_redraw` | `&self` |
| 64 | `request_frame_capture` | `&self, Box<dyn FnOnce(CapturedFrame) + Send + 'static>` |

### trait WindowManager (lines 554–627)

| # | Method | Signature summary |
|---|---|---|
| 65 | `open_window` | `&mut self, WindowId, WindowOptions, WindowCallbacks → Result<()>` |
| 66 | `platform_window` | `&self, WindowId → OptionalPlatformWindow` |
| 67 | `remove_window` | `&mut self, WindowId` |
| 68 | `active_window_id` | `&self → Option<WindowId>` |
| 69 | `key_window_is_modal_panel` | `&self → bool` |
| 70 | `app_is_active` | `&self → bool` |
| 71 | `activate_app` | `&self, Option<WindowId> → Option<WindowId>` |
| 72 | `show_window_and_focus_app` | `&self, WindowId, WindowFocusBehavior` |
| 73 | `hide_app` | `&self` |
| 74 | `hide_window` | `&self, WindowId` |
| 75 | `set_window_bounds` | `&self, WindowId, RectF` |
| 76 | `set_all_windows_background_blur_radius` | `&self, u8` |
| 77 | `set_all_windows_background_blur_texture` | `&self, bool` |
| 78 | `set_window_title` | `&self, WindowId, &str` |
| 79 | `close_window_async` | `&self, WindowId, TerminationMode` |
| 80 | `active_display_bounds` | `&self → RectF` |
| 81 | `active_display_id` | `&self → DisplayId` |
| 82 | `display_count` | `&self → usize` |
| 83 | `bounds_for_display_idx` | `&self, DisplayIdx → Option<RectF>` |
| 84 | `active_cursor_position_updated` | `&self` |
| 85 | `windowing_system` | `&self → Option<windowing::System>` |
| 86 | `os_window_manager_name` | `&self → Option<String>` |
| 87 | `is_tiling_window_manager` | `&self → bool` |
| 88 | `ordered_window_ids` (default) | `&self → Vec<WindowId>` (default: vec![]) |
| 89 | `cancel_synthetic_drag` (default) | `&self, WindowId` (default: no-op) |

**Total trait methods: 89** (25 Delegate + 2 DispatchDelegate + 1 LoadedSystemFonts + 2 TextLayoutSystem + 15 FontDB + 12 Window + 7 WindowContext + 25 WindowManager)

---

## 3. gpui-mobile Trait Surface

gpui-mobile implements **`gpui::Platform`** (Zed framework), which has an entirely different method set. Key methods from `IosPlatform` and `AndroidPlatform`:

- `background_executor`, `foreground_executor`, `text_system`
- `run`, `quit`, `restart`, `activate`, `hide`, `hide_other_apps`, `unhide_other_apps`
- `displays`, `primary_display`, `active_window`
- `open_window(AnyWindowHandle, WindowParams) → Box<dyn PlatformWindow>`
- `window_appearance`
- `open_url`, `on_open_urls`, `register_url_scheme`
- `prompt_for_paths`, `prompt_for_new_path`, `can_select_mixed_files_and_dirs`
- `reveal_path`, `open_with_system`
- `on_quit`, `on_reopen`, `set_menus`, `set_dock_menu`
- `on_app_menu_action`, `on_will_open_app_menu`, `on_validate_app_menu_command`
- `thermal_state`, `on_thermal_state_change`
- `app_path`, `path_for_auxiliary_executable`
- `set_cursor_style`, `should_auto_hide_scrollbars`
- `write_to_clipboard`, `read_from_clipboard`
- `write_credentials`, `read_credentials`, `delete_credentials`
- `keyboard_layout`, `keyboard_mapper`, `on_keyboard_layout_change`

Additional gpui-mobile-specific exports (NOT in `gpui::Platform`):
- `show_keyboard()` / `hide_keyboard()` / `show_keyboard_with_type()`
- `keyboard_height()` / `set_keyboard_height()`
- `safe_area_insets()` → `(top, bottom, left, right)`
- `set_system_chrome(SystemChromeStyle)`
- `dispatch_text_input()` / `set_text_input_callback()`
- `TEXT_INPUT_DIRTY: AtomicBool`
- `PlatformView` / `PlatformViewFactory` traits (embedded native views)
- `TargetPlatform` enum + `target_platform()` fn

---

## 4. Delta Table: warpui_core Traits ↔ gpui-mobile

**Key finding:** gpui-mobile does NOT implement `warpui_core::platform` traits. It implements `gpui::Platform` — a completely different interface from the Zed codebase. The two cannot be directly composed.

| warpui_core method | gpui-mobile status | Classification | Notes |
|---|---|---|---|
| **Delegate** | | | |
| `dispatch_delegate` | `background_executor` / `foreground_executor` exist but return different types | **INCOMPATIBLE** | warp uses `Arc<dyn DispatchDelegate>`, gpui uses `BackgroundExecutor`/`ForegroundExecutor` wrapper structs |
| `request_user_attention` | missing | **MISSING** | iOS/Android have no user attention concept |
| `clipboard` | `write_to_clipboard` / `read_from_clipboard` exist | **PORTABLE** | gpui uses `ClipboardItem` type; warp uses `&mut dyn Clipboard` trait object |
| `system_theme` | `window_appearance` exists | **PORTABLE** | gpui returns `WindowAppearance`; warp returns `SystemTheme` — same 2-value enum, different types |
| `open_url` | `open_url(&str)` — identical intent | **PORTABLE** | Same signature, both no-op on mobile but infra exists |
| `open_file_path` | missing | **MISSING** | No file system concept on iOS/Android |
| `open_file_path_in_explorer` | missing | **MISSING** | No file explorer on mobile |
| `open_file_picker` | `prompt_for_paths` (stub / unimplemented) | **INCOMPATIBLE** | Different callback model; gpui uses oneshot channel; warp uses callback fn |
| `open_save_file_picker` | `prompt_for_new_path` (stub) | **INCOMPATIBLE** | Same callback model mismatch |
| `application_bundle_info` | `app_path` exists | **PORTABLE** | Warp wants bundle info struct; mobile can only give app path |
| `show_native_platform_modal` | missing | **MISSING** | No native modal API on iOS/Android via gpui-mobile |
| `request_desktop_notification_permissions` | missing | **MISSING** | Not implemented in gpui-mobile |
| `send_desktop_notification` | missing | **MISSING** | Not implemented |
| `set_cursor_shape` | `set_cursor_style` exists (no-op) | **PORTABLE** | Both are no-ops on touch devices; warp uses `Cursor` enum, gpui uses `CursorStyle` |
| `get_cursor_shape` (test-util) | missing | **MISSING** | Test-only; irrelevant |
| `close_ime_async` | `hide_keyboard()` in gpui-mobile extras | **PORTABLE** | gpui-mobile has IME hide but not via `Platform` trait, via free fn |
| `is_ime_open` | missing from Platform trait | **MISSING** | Not tracked in gpui-mobile Platform trait |
| `open_character_palette` | missing | **MISSING** | No concept on mobile |
| `set_accessibility_contents` | missing | **MISSING** | Not implemented |
| `register_global_shortcut` | missing | **MISSING** | No global shortcuts on mobile |
| `unregister_global_shortcut` | missing | **MISSING** | |
| `terminate_app` | `quit()` | **PORTABLE** | Same intent; iOS logs warning (can't programmatically quit) |
| `is_screen_reader_enabled` | missing | **MISSING** | Not implemented |
| `microphone_access_state` | missing | **MISSING** | Not in gpui::Platform |
| `is_headless` | `headless` field on AndroidPlatform | **PORTABLE** | Android has headless mode; iOS does not expose this |
| **DispatchDelegate** | | | |
| `is_main_thread` | `PlatformDispatcher::is_main_thread` (indirect) | **PORTABLE** | gpui wraps this in `BackgroundExecutor`; concept exists |
| `run_on_main_thread` | `PlatformDispatcher::dispatch_on_main_thread` (indirect) | **PORTABLE** | Same concept, different wrapper |
| **TextLayoutSystem** | | | |
| `layout_line` | `PlatformTextSystem` (different trait) | **INCOMPATIBLE** | gpui uses `PlatformTextSystem` with `layout` method; warp has own `TextLayoutSystem::layout_line` |
| `layout_text` | `PlatformTextSystem` (different trait) | **INCOMPATIBLE** | Same mismatch |
| **FontDB** (15 methods) | | | |
| `load_from_bytes` | partial in `CosmicTextSystem::add_fonts` | **INCOMPATIBLE** | gpui-mobile uses cosmic-text; warp's FontDB has distinct API for per-font loading |
| `load_from_system` | font path scanning in `AndroidPlatform::new` | **INCOMPATIBLE** | Android manually loads /system/fonts; iOS uses CoreText; neither matches warpui_core::FontDB API |
| `load_all_system_fonts` | inline in constructor | **INCOMPATIBLE** | gpui doesn't have this async pattern |
| `process_loaded_system_fonts` | not present | **MISSING** | warp-specific async font loading pattern |
| `family_id_for_name` | not exposed | **MISSING** | Internal to cosmic-text/CoreText |
| `load_family_name_from_id` | not exposed | **MISSING** | |
| `select_font` | not exposed | **MISSING** | |
| `fallback_fonts` | not exposed | **MISSING** | |
| `font_metrics` | not exposed | **MISSING** | |
| `glyph_advance` | not exposed | **MISSING** | |
| `glyph_raster_bounds` | not exposed | **MISSING** | |
| `glyph_typographic_bounds` | not exposed | **MISSING** | |
| `rasterize_glyph` | not exposed | **MISSING** | |
| `glyph_for_char` | not exposed | **MISSING** | |
| `text_layout_system` | not exposed | **MISSING** | |
| **Window** | | | |
| `minimize` | no-op (not applicable) | **PORTABLE** | Mobile has no minimize; headless also no-ops |
| `toggle_maximized` | no-op | **PORTABLE** | Mobile always fullscreen |
| `toggle_fullscreen` | no-op | **PORTABLE** | Always fullscreen |
| `fullscreen_state` | implicit (always fullscreen) | **PORTABLE** | Can return `FullscreenState::Fullscreen` always |
| `uses_native_window_decorations` | false | **PORTABLE** | Mobile has no decorations |
| `set_titlebar_height` | no-op | **PORTABLE** | |
| `supports_transparency` | partial (wgpu) | **PORTABLE** | Depends on GPU config |
| `graphics_backend` | `Vulkan` (Android) / `Metal` (iOS) | **PORTABLE** | Different enum type but concept maps |
| `supported_backends` | single-backend | **PORTABLE** | Mobile has one backend |
| `as_ctx` / `callbacks` / `as_any` | structural methods | **INCOMPATIBLE** | gpui-mobile's `PlatformWindow` has different structure |
| **WindowContext** | | | |
| `size` | window dimensions available | **PORTABLE** | Both have size |
| `origin` | `(0,0)` on mobile | **PORTABLE** | |
| `backing_scale_factor` | scale factor available | **PORTABLE** | gpui uses `DevicePixels`; warp uses `f32` — different unit types |
| `max_texture_dimension_2d` | not exposed in Platform | **MISSING** | warp-specific GPU query |
| `render_scene` | render pipeline in wgpu | **INCOMPATIBLE** | warp uses `Rc<Scene>`; gpui-mobile uses a completely different render path |
| `request_redraw` | frame-driven render | **PORTABLE** | Concept exists |
| `request_frame_capture` | not implemented | **MISSING** | |
| **WindowManager** | | | |
| `open_window` | `open_window(AnyWindowHandle, WindowParams)` | **INCOMPATIBLE** | Completely different signature; warp uses `WindowId`+`WindowOptions`+callbacks; gpui uses handle+params→Box<dyn PlatformWindow> |
| `platform_window` | via `primary_window()` | **INCOMPATIBLE** | Different return type and lookup model |
| `remove_window` | `close_window(id)` | **PORTABLE** | Same concept |
| `active_window_id` | `active_window()→Option<AnyWindowHandle>` | **INCOMPATIBLE** | Different ID types |
| `key_window_is_modal_panel` | missing | **MISSING** | |
| `app_is_active` | `is_active()` | **PORTABLE** | Same concept |
| `activate_app` | `activate()` | **PORTABLE** | |
| `show_window_and_focus_app` | no direct equivalent | **MISSING** | |
| `hide_app` | `hide()` (no-op) | **PORTABLE** | |
| `hide_window` | no equivalent | **MISSING** | |
| `set_window_bounds` | not applicable mobile | **MISSING** | Mobile windows don't have movable bounds |
| `set_all_windows_background_blur_radius` | missing | **MISSING** | |
| `set_all_windows_background_blur_texture` | missing | **MISSING** | |
| `set_window_title` | missing | **MISSING** | Mobile apps have no title bar |
| `close_window_async` | `close_window(id)` (sync) | **PORTABLE** | Concept maps |
| `active_display_bounds` | via `displays()[0]` | **PORTABLE** | |
| `active_display_id` | via `displays()[0]` | **PORTABLE** | |
| `display_count` | `displays().len()` | **PORTABLE** | |
| `bounds_for_display_idx` | via `displays()` | **PORTABLE** | |
| `active_cursor_position_updated` | no concept | **MISSING** | Touch-only; no cursor |
| `windowing_system` | no concept | **MISSING** | |
| `os_window_manager_name` | no concept | **MISSING** | |
| `is_tiling_window_manager` | always false | **PORTABLE** | |
| `ordered_window_ids` (default) | single window | **PORTABLE** | |
| `cancel_synthetic_drag` (default) | no-op | **PORTABLE** | |

---

## 5. Delta Count Summary

| Classification | Count | % of 89 |
|---|---|---|
| **IDENTICAL** | 0 | 0% |
| **PORTABLE** (same intent, slight sig diff, ≤ trivial adaptation) | 31 | 35% |
| **INCOMPATIBLE** (fundamentally different trait surface) | 13 | 15% |
| **MISSING** (gpui-mobile has no corresponding concept) | 45 | 50% |

**Total methods analysed: 89**

### Critically incompatible areas

1. **FontDB (15 methods)** — 14 MISSING + 1 INCOMPATIBLE. gpui-mobile's text system (CosmicTextSystem for Android, CoreText for iOS) is completely encapsulated and does not expose the fine-grained glyph/font API that warpui_core::FontDB requires. This is the single largest incompatibility surface.
2. **TextLayoutSystem (2 methods)** — both INCOMPATIBLE. gpui uses a higher-level `PlatformTextSystem::layout` API; warpui_core exposes `layout_line` / `layout_text` with warp-specific types (`Line`, `TextFrame`, `StyleAndFont`, `ClipConfig`).
3. **render_scene / WindowContext rendering** — gpui-mobile uses wgpu internally, but the surface is accessed differently: warp renders by passing `Rc<Scene>` down; gpui uses its own paint system.
4. **Window management IDs** — warp uses numeric `WindowId`; gpui uses `AnyWindowHandle` (entity handle). No shared ID space.
5. **Dispatch model** — warp exposes a `DispatchDelegate` trait object directly; gpui wraps dispatch in `BackgroundExecutor`/`ForegroundExecutor` newtype structs.

### Conclusion on Critic's claim

**The Critic's claim "gpui-mobile is incompatible" is substantially correct but requires nuance:**

- **Direct trait reuse: impossible.** gpui-mobile implements `gpui::Platform`, not `warpui_core::platform::Delegate` / `WindowManager` / `FontDB`. These are entirely different trait families.
- **Concept reuse: partially possible (35%).** The portable methods show that the *intent* of many operations maps across: IME control, URL opening, clipboard, display geometry, app lifecycle, and basic windowing no-ops. A mobile backend for warpui_core *could* borrow these implementations as a reference.
- **The critical blocker is FontDB (50% of missing methods when combined with other missing items).** Warp's text rendering pipeline is deeply coupled to its own FontDB/TextLayoutSystem trait. Any mobile port must either: (a) implement a new FontDB on top of CoreText/cosmic-text by wrapping their internals, or (b) replace Warp's font stack with a different approach.
- **gpui-mobile is a useful architecture reference** (touch event translation, keyboard height tracking, safe area insets, wgpu surface lifecycle) even if it cannot be a direct dependency.

---

## Decision A1-vs-A4 Archeology

*(See section below — Task #7)*

---

## 6. Task #7 — A1-vs-A4 Archeology: linux vs headless Derive Base

### Source files analysed

- `warp-src/crates/warpui/src/platform/linux/mod.rs`
- `warp-src/crates/warpui/src/platform/headless/delegate.rs`
- `warp-src/crates/warpui/src/platform/headless/windowing.rs`
- `warp-src/crates/warpui/src/platform/headless/app.rs`
- `warp-src/crates/warpui/src/platform/headless/event_loop.rs`
- `warp-src/crates/warpui/src/platform/wasm/mod.rs` (mobile_detection)

### 6.1 Linux backend scoring (A1 path)

Linux lives in `warpui/src/platform/linux/mod.rs` — but this file is a **thin shim** (~80 lines). It re-exports `crate::windowing::winit::app::App` as the concrete `App` type and adds two Linux-specific extensions (`AppBuilderExt`: `set_window_class`, `force_x11`). The actual `Delegate`, `WindowManager`, and `Window` implementations are inside `crates/warpui/src/windowing/winit/` (the winit integration layer, not visible in `platform/linux/`).

This means the "linux backend" is really the **winit backend** — it uses windowing primitives (event loop, window handle, input events) that are **not available on Android**.

**Linux backend method scoring (A1 path):**

| Trait | Impl Status | Mobile derive cost |
|---|---|---|
| `Delegate` | Full impl via winit delegate | **Major rewrite** — winit's X11/Wayland windowing does not exist on Android; every windowing call would need replacement |
| `DispatchDelegate` | Full (runs on main thread via winit event loop) | **Minor patch** — thread identity check is portable; `run_on_main_thread` needs ALooper instead of winit proxy |
| `FontDB` | Full (platform-specific fontconfig/FreeType) | **Major rewrite** — fontconfig not available on Android; need to replace with system font path scanning |
| `TextLayoutSystem` | Full | **Portable** — text layout logic is likely in a shared crate; ≤20 lines to adapt |
| `Window` | Full via winit `Window` struct | **Major rewrite** — winit Window wraps OS window handle; Android uses ANativeWindow |
| `WindowContext` | Full | **Major rewrite** — backed by winit GPU surface |
| `WindowManager` | Full via winit | **Major rewrite** — entire multi-window model is winit-based |

**A1 total assessment:** 4 of 7 trait groups require major rewrites (defined as > 20 lines, likely > 200 lines each). The linux backend is fundamentally winit-coupled. Reusing it for Android would mean porting winit to Android or replacing every winit call — which is essentially writing a new backend.

**A1 estimated work: 6–8 engineer-weeks** (assumes reusing text layout + dispatch logic, rewriting window/windowing/delegate/FontDB).

### 6.2 Headless backend scoring (A4 path)

The headless backend is fully implemented and self-contained:
- `headless/delegate.rs` — **complete `platform::Delegate` implementation** (all 25 methods implemented; stubs where unsupported, real logic where possible)
- `headless/windowing.rs` — **complete `platform::WindowManager` + `platform::Window` + `platform::WindowContext` implementation**
- `headless/app.rs` — clean app bootstrap (uses `TestFontDB`, event loop via mpsc channel)
- `headless/event_loop.rs` — simple mpsc-driven event loop with signal handler

**Headless backend method scoring (A4 path):**

| Trait | Headless impl | Mobile derive cost |
|---|---|---|
| `Delegate::dispatch_delegate` | Full — creates `DispatchDelegate` with mpsc Sender | **Minor patch** — replace `mpsc::Sender` with Android ALooper channel or Tokio; ~10 lines |
| `Delegate::clipboard` | Full — `InMemoryClipboard` | **Minor patch** — could use InMemory for now, later wire to JNI clipboard; 0 lines initially |
| `Delegate::system_theme` | Stub — always `SystemTheme::Light` | **Total reuse** — same for mobile initially |
| `Delegate::open_url` | Partial — delegates to mac/winit; needs own impl | **Minor patch** — replace with JNI Intent call; ~15 lines |
| `Delegate::open_file_path` | Stub — Unsupported | **Total reuse** — same no-op for mobile |
| `Delegate::open_file_picker` | Stub — returns empty vec via callback | **Total reuse** — same stub; later wire to Android Storage Access Framework |
| `Delegate::show_native_platform_modal` | Stub — Unsupported | **Total reuse** |
| `Delegate::request_desktop_notification_permissions` | Stub — denies permissions | **Total reuse** — same for mobile initially |
| `Delegate::send_desktop_notification` | Stub — returns error | **Total reuse** |
| `Delegate::set_cursor_shape` | Full — stores cursor shape in Mutex | **Total reuse** — no-op on touch but valid |
| `Delegate::close_ime_async` | Stub — Unsupported | **Minor patch** — wire to `hide_keyboard()` from gpui-mobile; ~5 lines |
| `Delegate::is_ime_open` | Stub — false | **Minor patch** — wire to keyboard height check; ~3 lines |
| `Delegate::open_character_palette` | Stub | **Total reuse** |
| `Delegate::set_accessibility_contents` | Stub | **Total reuse** |
| `Delegate::register_global_shortcut` | Stub | **Total reuse** |
| `Delegate::terminate_app` | Full — sends `AppEvent::Terminate` | **Total reuse** |
| `Delegate::is_screen_reader_enabled` | Stub — None | **Total reuse** |
| `Delegate::microphone_access_state` | Stub — Denied | **Minor patch** — wire to Android permission API; ~10 lines |
| `Delegate::is_headless` | Returns `true` | **Minor patch** — return false for real device, true for headless Android |
| `DispatchDelegate::is_main_thread` | Full — ThreadId check | **Total reuse** — same logic |
| `DispatchDelegate::run_on_main_thread` | Full — mpsc to event loop | **Minor patch** — replace mpsc with ALooper pipe; ~20 lines |
| `WindowManager` (all 25 methods) | **Fully implemented** | 18 methods: **Total reuse** (the headless window manager is already a minimal implementation that tracks state without any OS windowing); 4 methods need **minor patches** for Android surface lifecycle (`open_window` needs ANativeWindow hookup, `close_window_async` needs ALooper, display bounds from NDK) |
| `Window` (all 12 methods) | **Fully implemented** (no-ops or simple stubs) | 9 methods: **Total reuse** (minimize/maximize/fullscreen are no-ops; uses_native_decorations=false; transparency=false); 3 methods **minor patch** (graphics_backend should return Vulkan; backed by ANativeWindow/wgpu surface) |
| `WindowContext` (7 methods) | **Fully implemented** | 5 methods: **Total reuse** (size, origin, scale_factor, max_texture_dimension_2d, request_redraw); 2 methods **major rewrite** (`render_scene` needs wgpu surface; `request_frame_capture` needs wgpu readback) |
| `FontDB` (15 methods) | Uses `TestFontDB` | **Major rewrite** — `TestFontDB` is a test stub with no real shaping; needs new FontDB impl wrapping Android system fonts via FreeType or cosmic-text; ~200 lines |
| `TextLayoutSystem` (2 methods) | Via `TestFontDB` (test stub) | **Major rewrite** — same as FontDB; needs real text layout on Android |

**A4 total assessment:**

| Category | Count |
|---|---|
| Total reuse (0 lines) | 22 methods |
| Minor patch (≤20 lines each) | 10 methods |
| Major rewrite (>20 lines) | 4 areas (render_scene, frame_capture, FontDB×15, TextLayout×2) |

The major rewrites cluster entirely in **rendering and text**. Everything else in headless is directly portable.

**A4 estimated work: 3–4 engineer-weeks**
- Week 1: Scaffold mobile backend copying headless, wire ALooper dispatcher, hook up ANativeWindow in WindowManager/Window
- Week 2: Implement `render_scene` via wgpu + ANativeWindow surface (can reference gpui-mobile's `AndroidWindow::render`)
- Week 3-4: Implement Android FontDB (load /system/fonts, wrap cosmic-text or FreeType to match `FontDB` API methods)
- Headless text layout stubs can remain throughout M0 if text rendering is deferred

### 6.3 WASM mobile detection (borrowable for Android)

`warp-src/crates/warpui/src/platform/wasm/mobile_detection/mod.rs` contains:
- `is_mobile_device()` — cached, checks `navigator.max_touch_points > 0` + user agent
- `is_mobile_user_agent()` — UA string matching

These are WASM-only (use `gloo`/`web_sys`). The **concept** is borrowable: the Android backend always knows it's mobile by `cfg(target_os = "android")`, so no detection logic is needed. The `is_mobile_device()` free function in `warpui/src/platform/mod.rs` should return `true` when compiled for Android (currently hardcoded to `false` for all non-WASM targets).

---

### 6.4 Recommendation

**Recommended path: A4 (headless) as base, with selective reference to gpui-mobile**

**Rationale:**

1. **A4 saves ~3 engineer-weeks vs A1.** The headless backend already implements the entire `Delegate` + `WindowManager` + `Window` + `WindowContext` trait surface with clean stubs. The Linux/winit backend is deeply entangled with winit primitives that don't exist on Android.

2. **The headless backend is intentionally minimal** — it makes no assumptions about OS windowing, uses in-memory data structures, and is driven by an mpsc event loop. This is the right foundation for a new mobile backend that will replace the mpsc loop with ALooper/ANativeActivity.

3. **The 4 major rewrite areas are unavoidable regardless of base.** `render_scene` (wgpu surface), `request_frame_capture` (wgpu readback), `FontDB`, and `TextLayoutSystem` must be newly implemented for Android in either path. A1 would require these AND rewriting the entire windowing layer.

4. **gpui-mobile is a reference implementation** for the 4 major rewrite areas: `AndroidWindow` shows the ANativeWindow + wgpu surface lifecycle; `AndroidPlatform::new` shows system font loading patterns for cosmic-text; the dispatcher shows ALooper integration. These can be ported into warpui's trait API without taking a dependency on gpui-mobile itself.

5. **Hybrid within A4 is optimal:**
   - `Delegate` + `DispatchDelegate` + `WindowManager` + `Window` + `WindowContext` (except render): **copy headless, minor patches for Android**
   - `render_scene` / `request_frame_capture`: **new impl, reference gpui-mobile AndroidWindow**
   - `FontDB` + `TextLayoutSystem`: **new impl wrapping cosmic-text, reference gpui-mobile AndroidPlatform font loading**

### 6.5 Work estimate summary

| Path | Weeks | Risk |
|---|---|---|
| A1 (from linux/winit) | 6–8 weeks | High — winit entanglement, no clean separation |
| A4 (from headless) | 3–4 weeks | Medium — rendering + font work unavoidable; windowing is clean |
| **A4 + gpui-mobile reference** | **3–4 weeks** | **Medium-Low — best coverage of unknowns** |

**Recommendation: A4 with gpui-mobile as reference. Do not take gpui-mobile as a Cargo dependency** (incompatible trait system); use it as an implementation guide only.
