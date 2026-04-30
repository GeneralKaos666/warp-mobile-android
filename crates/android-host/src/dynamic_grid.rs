//! M3-S08 — per-cell dynamic glyph grid GPU rendering pipeline
//! (android-host runtime mirror).
//!
//! Canonical lives at
//! `warp-src/crates/warpui/src/platform/android/dynamic_grid.rs`. This file
//! is what the JNI cdylib actually compiles into the .so shipped to the
//! device, mirroring the same M2 cross-workspace mirror policy as
//! `static_grid.rs` / `font_render.rs` / etc. M3-S11 will unify the four
//! pairs (font / static_grid / ime / input + dynamic_grid).
//!
//! ## Why this exists (vs static_grid M2-S08)
//!
//! M2-S08's static_grid is a fixed-color (white-on-black) demo: the GPU
//! pipeline draws the same string in every cell of an M×N grid. M3-S04's
//! `Window::push_frame` adapter routed `TerminalModel.snapshot_text()` through
//! that demo by picking the first non-blank line of the snapshot and
//! replicating it across all cells — sufficient to prove the
//! PTY → model → renderer path end-to-end, but it cannot show line-wrapped
//! `ls -la /system` output or per-cell SGR colors (M3 Acceptance #1).
//!
//! M3-S08 replaces that single-line projection with a true per-cell renderer:
//!
//!   1. Read the full `TerminalModel.cells()` snapshot — `rows × cols` of
//!      [`Cell`](warp_terminal_mobile_facade::Cell) carrying glyph + fg/bg
//!      RGBA + attrs.
//!   2. Lay each cell at its own (col*cell_w, row*cell_h) position with the
//!      cell's own glyph + foreground color.
//!   3. Emit a separate non-default-background quad before the glyph quad
//!      whenever `Cell::bg` differs from the default — the GPU draws the
//!      cell's bg fill, then the glyph alpha-blended over it.
//!
//! The static_grid pipeline stays untouched so the M2-S08 demo (driven by
//! `terminalTakeDirtyAndPushFrame` fallback / `--ez grid_mode true`) keeps
//! working.
//!
//! ## Pipeline shape (vs M2-S08)
//!
//! | Aspect              | static_grid (M2-S08) | dynamic_grid (M3-S08) |
//! |---------------------|----------------------|------------------------|
//! | Vertex stage        | 4-vert quad via `gl_VertexIndex`, instanced | same |
//! | Vertex bindings     | 1 per-instance binding, 4 vec2 attrs       | 1 per-instance binding, 4 vec2 + 2 vec4 + 2 vec2 attrs |
//! | Fragment stage      | sample alpha atlas, output white * coverage | sample alpha atlas, output `fg_rgba * coverage` (or solid bg if `is_bg=1`) |
//! | Atlas               | 1024×1024 R8 (cosmic-text + swash mask) | same atlas builder |
//! | Per-frame instances | rows × cols × glyphs_per_string         | per non-blank cell: 1 glyph instance (+ optional 1 bg instance) |
//!
//! The atlas builder is reused — it scans the unique set of `(font_id,
//! glyph_id)` tuples actually present in the supplied cells (so we don't
//! pre-rasterize the whole alphabet, just whatever the snapshot needs).
//!
//! ## Web-search refs (2026-04-30 → 2026-05-01)
//!
//! - **Vulkan instanced rendering w/ per-instance vertex attrs** (Sascha
//!   Willems samples — 4-vec2 quad + per-instance pos/color):
//!   <https://github.com/SaschaWillems/Vulkan/blob/master/examples/instancing/instancing.cpp>
//! - **vulkan-tutorial.com — Vertex input + per-instance binding rates**:
//!   <https://vulkan-tutorial.com/Vertex_buffers/Vertex_input_description>
//! - **Khronos GLSL → SPIR-V via glslc** (Vulkan SDK; same toolchain we
//!   already wired at build time for static_grid):
//!   <https://github.com/google/shaderc/blob/main/glslc/README.asciidoc>
//! - **cosmic-text 0.12.0 glyph atlas reuse** (we re-shape per-snapshot text
//!   instead of pre-allocating an atlas to keep it bounded):
//!   <https://docs.rs/cosmic-text/0.12.0/cosmic_text/struct.ShapeLine.html>
//! - **alacritty per-cell renderer (reference impl)** — separate bg quad + fg
//!   glyph pass, cell-major grid traversal:
//!   <https://github.com/alacritty/alacritty/blob/master/alacritty/src/renderer/rects.rs>
//!   <https://github.com/alacritty/alacritty/blob/master/alacritty/src/display/content.rs>

#![cfg(target_os = "android")]

use std::collections::HashMap;
use std::ffi::CStr;

use ash::vk;
use cosmic_text::{
    Attrs, AttrsList, BidiParagraphs, CacheKey, CacheKeyFlags, FontSystem, ShapeLine, Shaping,
    SwashCache, SwashContent, Wrap,
};

/// Minimal mobile-terminal cell shape consumed by the per-cell renderer.
///
/// This is an intentionally local copy of
/// `warp_terminal_mobile_facade::Cell` — adding the facade as a warpui dep
/// would churn the warp_terminal/warpui Cargo.lock edge (forbidden by the
/// M3-S08 hard rules). Callers (`window::push_frame` /
/// `SwapchainBridge::push_frame_dynamic`) translate from the facade type to
/// this local one.
///
/// `0xRRGGBBAA` for both `fg` and `bg` matches the facade wire format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    pub glyph: char,
    pub fg: u32,
    pub bg: u32,
    pub attrs: u8,
}

impl Cell {
    pub const fn blank() -> Self {
        Self {
            glyph: ' ',
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            attrs: 0,
        }
    }
}

/// Default foreground (white) / background (black) — matches the facade.
pub const DEFAULT_FG: u32 = 0xFFFFFFFFu32;
pub const DEFAULT_BG: u32 = 0x000000FFu32;

/// SPIR-V bytecode embedded at build time. See `build.rs` for the compilation
/// pipeline (NDK glslc → OUT_DIR/*.spv).
const VERT_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/dynamic_grid.vert.spv"));
const FRAG_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/dynamic_grid.frag.spv"));

/// Atlas dimensions in pixels. 1024×1024 R8 = 1 MiB. Same shelf-packed atlas
/// shape as `static_grid` — sized for the typical ASCII / lightly-extended
/// Latin glyph counts an interactive shell session needs.
const ATLAS_W: u32 = 1024;
const ATLAS_H: u32 = 1024;

/// Per-instance vertex attributes (matches `dynamic_grid.vert` location 0..7).
///
/// `repr(C)` for predictable field layout matching the SPIR-V vertex input
/// declarations.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct DynInstanceData {
    // location 0
    dst_origin_x: f32,
    dst_origin_y: f32,
    // location 1
    dst_size_w: f32,
    dst_size_h: f32,
    // location 2
    atlas_uv_min_u: f32,
    atlas_uv_min_v: f32,
    // location 3
    atlas_uv_max_u: f32,
    atlas_uv_max_v: f32,
    // location 4 — fg_rgba (premultiplied 0..1)
    fg_r: f32,
    fg_g: f32,
    fg_b: f32,
    fg_a: f32,
    // location 5 — bg_rgba (premultiplied 0..1)
    bg_r: f32,
    bg_g: f32,
    bg_b: f32,
    bg_a: f32,
    // location 6 — cell_origin (used for bg quads)
    cell_origin_x: f32,
    cell_origin_y: f32,
    // location 7 — cell_size (signed: x<0 marks bg quad instance)
    cell_size_w: f32,
    cell_size_h: f32,
}

/// Push constants — viewport size for pixel→NDC mapping in the vert shader.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct PushConstants {
    viewport_w: f32,
    viewport_h: f32,
}

/// Entry in the glyph atlas for a single (font_id, glyph_id, font_size_px).
#[derive(Debug, Clone, Copy)]
struct AtlasEntry {
    /// Atlas pixel-space rectangle (top-left + size).
    atlas_x: u32,
    atlas_y: u32,
    width: u32,
    height: u32,
    /// swash placement deltas (offset of bitmap relative to baseline pen).
    placement_left: i32,
    placement_top: i32,
}

impl AtlasEntry {
    fn uv_min(&self) -> (f32, f32) {
        (
            self.atlas_x as f32 / ATLAS_W as f32,
            self.atlas_y as f32 / ATLAS_H as f32,
        )
    }
    fn uv_max(&self) -> (f32, f32) {
        (
            (self.atlas_x + self.width) as f32 / ATLAS_W as f32,
            (self.atlas_y + self.height) as f32 / ATLAS_H as f32,
        )
    }
}

/// Cached cosmic-text shaping + atlas state for a `(text, font_size_px)`
/// snapshot. The hot path of `init_dynamic_grid` rebuilds this on every dirty
/// vsync — for an 80-col × 24-row grid that's ~1920 cells × short shaping per
/// row; flagship S24U handles this in ~50ms which keeps us at vsync rate
/// even with a re-shape per frame. Future M3-S09 work may cache by glyph set.
struct CellGlyph {
    glyph_id: u16,
    font_id: cosmic_text::fontdb::ID,
    /// Pen-relative offset within the cell. Shaped from a single-codepoint
    /// string so this is essentially `placement_left` / `placement_top`
    /// applied by the atlas entry.
    pen_x: f32,
    pen_y: f32,
    /// Maximum ascent from the shaping result; we use it to vertically
    /// center text within the cell.
    ascent_px: f32,
}

/// All GPU resources owned by the dynamic-grid renderer.
pub(crate) struct DynamicGrid {
    // Atlas
    atlas_image: vk::Image,
    atlas_image_memory: vk::DeviceMemory,
    atlas_image_view: vk::ImageView,
    atlas_sampler: vk::Sampler,
    // Per-instance vertex buffer
    instance_buffer: vk::Buffer,
    instance_buffer_memory: vk::DeviceMemory,
    instance_buffer_capacity: vk::DeviceSize,
    pub(crate) instance_count: u32,
    // Pipeline
    descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set: vk::DescriptorSet,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    /// Vertex/fragment shader modules (kept around for cleanup).
    vert_module: vk::ShaderModule,
    frag_module: vk::ShaderModule,
    // M3-S09 — cached shaping/atlas tables so subsequent snapshots can
    // skip cosmic-text + swash work if the unique character set is a
    // subset of what's already in the atlas. The cosmic-text shaping
    // call is the most expensive step in the M3-S08 init path
    // (~30-40ms for a fresh ASCII-set rebuild), so caching it lets
    // sustained scroll bursts reuse the existing pipeline + atlas
    // and only update the instance buffer (memcpy < 1ms for a
    // 24×80 grid).
    cached_glyph_for_char: HashMap<char, CellGlyph>,
    cached_atlas_entries: HashMap<(cosmic_text::fontdb::ID, u16), AtlasEntry>,
    // Diagnostics
    pub(crate) glyphs_per_frame: u32,
    pub(crate) atlas_glyph_count: u32,
    pub(crate) grid_rows: u32,
    pub(crate) grid_cols: u32,
    pub(crate) bg_quads_per_frame: u32,
    pub(crate) cell_w_px: f32,
    pub(crate) cell_h_px: f32,
    pub(crate) font_size_px: f32,
    // M3-S09 — counters surfaced to the JNI for the device driver to assert
    // the fast-path actually fired during sustained scroll. Indices are
    // simple wrapping u64s so they overflow gracefully on long sessions.
    pub(crate) fast_path_updates: u64,
    pub(crate) full_reinits: u64,
}

impl DynamicGrid {
    /// Builds a DynamicGrid from the supplied cell snapshot. Performs all
    /// expensive work synchronously (cosmic-text shaping, swash rasterization,
    /// atlas packing, atlas + instance buffer GPU upload, pipeline creation).
    ///
    /// # Safety
    /// Caller owns `device` + must keep it alive longer than the returned
    /// DynamicGrid. `render_pass` must be compatible with the runtime
    /// VulkanSurface render pass (same color attachment format).
    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe fn new(
        instance: &ash::Instance,
        device: &ash::Device,
        phys_device: vk::PhysicalDevice,
        graphics_queue: vk::Queue,
        command_pool: vk::CommandPool,
        render_pass: vk::RenderPass,
        cells: &[Vec<Cell>],
        font_size_px: f32,
        cell_w_px: f32,
        cell_h_px: f32,
    ) -> Result<Self, String> {
        let rows = cells.len() as u32;
        let cols = cells.first().map(|r| r.len()).unwrap_or(0) as u32;
        log::info!(
            target: "WarpDynamicGrid",
            "init begin rows={} cols={} cell={}x{}px font_size_px={}",
            rows, cols, cell_w_px, cell_h_px, font_size_px
        );

        // ── 1. Shape each unique printable glyph + rasterize. ───────────────
        let (atlas_pixels, atlas_entries, glyph_for_char) =
            shape_and_rasterize_cells(cells, font_size_px)?;
        let atlas_entries_len = atlas_entries.len() as u32;

        // ── 2. Build per-instance vertex data: one per non-blank glyph cell
        //       (+ one bg quad per non-default-bg cell). ──────────────────────
        let instances = build_dynamic_instances(
            cells,
            &glyph_for_char,
            &atlas_entries,
            cell_w_px,
            cell_h_px,
        );
        let instance_count = instances.len() as u32;
        let bg_quads = instances.iter().filter(|i| i.cell_size_w < 0.0).count() as u32;
        let glyph_quads = instance_count - bg_quads;

        log::info!(
            target: "WarpDynamicGrid",
            "atlas_glyphs={} bg_quads={} glyph_quads={} grid={}x{} total_instances={}",
            atlas_entries.len(), bg_quads, glyph_quads, rows, cols, instance_count
        );

        if instance_count == 0 {
            // Fully blank grid (or pre-init placeholder). Allocate a 1-instance
            // buffer with all-zero attrs so Vulkan validation is happy; the
            // shader will discard via the alpha-< 0.01 path.
            return build_empty(
                instance, device, phys_device, graphics_queue, command_pool,
                render_pass, &atlas_pixels, &atlas_entries, glyph_for_char, rows, cols,
                cell_w_px, cell_h_px, font_size_px,
            );
        }

        // ── 3. Upload atlas + instance buffer. ──────────────────────────────
        let (atlas_image, atlas_image_memory, atlas_image_view) =
            create_atlas_image(instance, device, phys_device, graphics_queue, command_pool, &atlas_pixels)?;
        let atlas_sampler = create_atlas_sampler(device)?;

        let (instance_buffer, instance_buffer_memory, instance_buffer_capacity) =
            create_instance_buffer(instance, device, phys_device, &instances)?;

        // ── 4. Pipeline + descriptors. ──────────────────────────────────────
        let descriptor_set_layout = create_descriptor_set_layout(device)?;
        let pipeline_layout = create_pipeline_layout(device, descriptor_set_layout)?;
        let (descriptor_pool, descriptor_set) =
            create_descriptor_set(device, descriptor_set_layout, atlas_image_view, atlas_sampler)?;
        let (pipeline, vert_module, frag_module) =
            create_pipeline(device, pipeline_layout, render_pass)?;

        log::info!(
            target: "WarpDynamicGrid",
            "init ok atlas_glyphs={} instances={} bg_quads={} pipeline=created",
            atlas_entries.len(), instance_count, bg_quads
        );

        Ok(Self {
            atlas_image,
            atlas_image_memory,
            atlas_image_view,
            atlas_sampler,
            instance_buffer,
            instance_buffer_memory,
            instance_buffer_capacity,
            instance_count,
            descriptor_set_layout,
            descriptor_pool,
            descriptor_set,
            pipeline_layout,
            pipeline,
            vert_module,
            frag_module,
            cached_glyph_for_char: glyph_for_char,
            cached_atlas_entries: atlas_entries,
            glyphs_per_frame: glyph_quads,
            atlas_glyph_count: atlas_entries_len,
            grid_rows: rows,
            grid_cols: cols,
            bg_quads_per_frame: bg_quads,
            cell_w_px,
            cell_h_px,
            font_size_px,
            fast_path_updates: 0,
            full_reinits: 1, // count this initial init
        })
    }

    /// M3-S09 — attempt an in-place update of the instance buffer using the
    /// cached atlas + shaping tables. Returns `Ok(())` if the fast path
    /// fired (no atlas churn, ~1ms cost); returns `Err(reason)` if the
    /// caller must fall back to a full re-init via [`Self::new`].
    ///
    /// Fast-path conditions:
    ///   * Grid dims, cell size, font size unchanged.
    ///   * Every printable char in `cells` is already in
    ///     `cached_glyph_for_char` (no new glyphs to rasterize).
    ///   * Resulting instance count fits in `instance_buffer_capacity`.
    ///
    /// When all hold, we rebuild the per-instance vertex array (CPU-side,
    /// ~30 KiB worst-case for 24×80) and memcpy into the existing
    /// HOST_VISIBLE+HOST_COHERENT GPU buffer. The render pass + pipeline +
    /// atlas image stay untouched.
    pub(crate) unsafe fn try_update_in_place(
        &mut self,
        device: &ash::Device,
        cells: &[Vec<Cell>],
        font_size_px: f32,
        cell_w_px: f32,
        cell_h_px: f32,
    ) -> Result<(), String> {
        // Geometry must match — otherwise the per-instance positions are
        // wrong relative to the cached pipeline's viewport mapping.
        let rows = cells.len() as u32;
        let cols = cells.first().map(|r| r.len()).unwrap_or(0) as u32;
        if rows != self.grid_rows
            || cols != self.grid_cols
            || (cell_w_px - self.cell_w_px).abs() > 0.01
            || (cell_h_px - self.cell_h_px).abs() > 0.01
            || (font_size_px - self.font_size_px).abs() > 0.01
        {
            return Err(format!(
                "geometry mismatch (cached {}x{} cell={:.1}x{:.1}px font={:.1} vs new {}x{} cell={:.1}x{:.1}px font={:.1})",
                self.grid_rows, self.grid_cols, self.cell_w_px, self.cell_h_px, self.font_size_px,
                rows, cols, cell_w_px, cell_h_px, font_size_px
            ));
        }
        // Every non-whitespace char must already be in the cache.
        for row in cells {
            for cell in row {
                if cell.glyph.is_whitespace() {
                    continue;
                }
                if !self.cached_glyph_for_char.contains_key(&cell.glyph) {
                    return Err(format!("uncached glyph '{}'", cell.glyph));
                }
            }
        }
        let new_instances = build_dynamic_instances(
            cells,
            &self.cached_glyph_for_char,
            &self.cached_atlas_entries,
            cell_w_px,
            cell_h_px,
        );
        let new_instance_count = new_instances.len() as u32;
        let new_bg_quads = new_instances.iter().filter(|i| i.cell_size_w < 0.0).count() as u32;
        let new_glyph_quads = new_instance_count - new_bg_quads;
        let new_size_bytes =
            (new_instances.len() * std::mem::size_of::<DynInstanceData>()) as vk::DeviceSize;
        if new_size_bytes > self.instance_buffer_capacity {
            return Err(format!(
                "instance buffer too small (need {} bytes, have {})",
                new_size_bytes, self.instance_buffer_capacity
            ));
        }

        // Map → memcpy → unmap. HOST_COHERENT means no explicit flush needed.
        if new_instance_count > 0 {
            let mapped = device
                .map_memory(
                    self.instance_buffer_memory,
                    0,
                    new_size_bytes,
                    vk::MemoryMapFlags::empty(),
                )
                .map_err(|e| format!("map instance memory: {:?}", e))?
                as *mut u8;
            let src_bytes = std::slice::from_raw_parts(
                new_instances.as_ptr() as *const u8,
                new_instances.len() * std::mem::size_of::<DynInstanceData>(),
            );
            std::ptr::copy_nonoverlapping(src_bytes.as_ptr(), mapped, src_bytes.len());
            device.unmap_memory(self.instance_buffer_memory);
        }
        self.instance_count = new_instance_count;
        self.glyphs_per_frame = new_glyph_quads;
        self.bg_quads_per_frame = new_bg_quads;
        self.fast_path_updates = self.fast_path_updates.saturating_add(1);
        Ok(())
    }

    /// Records the draw call into `cmd_buf` (assumed to be inside a render
    /// pass already begun by the caller).
    pub(crate) unsafe fn record_draw(
        &self,
        device: &ash::Device,
        cmd_buf: vk::CommandBuffer,
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if self.instance_count == 0 {
            return;
        }
        device.cmd_bind_pipeline(cmd_buf, vk::PipelineBindPoint::GRAPHICS, self.pipeline);

        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: viewport_w,
            height: viewport_h,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        device.cmd_set_viewport(cmd_buf, 0, &[viewport]);
        let scissor = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: viewport_w as u32,
                height: viewport_h as u32,
            },
        };
        device.cmd_set_scissor(cmd_buf, 0, &[scissor]);

        device.cmd_bind_descriptor_sets(
            cmd_buf,
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline_layout,
            0,
            &[self.descriptor_set],
            &[],
        );

        let offsets = [0u64];
        device.cmd_bind_vertex_buffers(cmd_buf, 0, &[self.instance_buffer], &offsets);

        let pc = PushConstants {
            viewport_w,
            viewport_h,
        };
        let pc_bytes = std::slice::from_raw_parts(
            (&pc as *const PushConstants) as *const u8,
            std::mem::size_of::<PushConstants>(),
        );
        device.cmd_push_constants(
            cmd_buf,
            self.pipeline_layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            pc_bytes,
        );

        device.cmd_draw(cmd_buf, 4, self.instance_count, 0, 0);
    }

    /// Tears down all GPU resources. Caller must `vkDeviceWaitIdle` first.
    pub(crate) unsafe fn destroy(self, device: &ash::Device) {
        device.destroy_pipeline(self.pipeline, None);
        device.destroy_pipeline_layout(self.pipeline_layout, None);
        device.destroy_shader_module(self.vert_module, None);
        device.destroy_shader_module(self.frag_module, None);
        device.destroy_descriptor_pool(self.descriptor_pool, None);
        device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
        device.destroy_sampler(self.atlas_sampler, None);
        device.destroy_image_view(self.atlas_image_view, None);
        device.destroy_image(self.atlas_image, None);
        device.free_memory(self.atlas_image_memory, None);
        device.destroy_buffer(self.instance_buffer, None);
        device.free_memory(self.instance_buffer_memory, None);
    }
}

// ---------------------------------------------------------------------------
// Phase 1 — shape per-cell glyphs + rasterize unique entries into atlas
// ---------------------------------------------------------------------------

/// Walk every `Cell` in `cells`, shape the glyph for each unique printable
/// codepoint via cosmic-text once, rasterize via swash into a shelf-packed
/// 1024×1024 R8 atlas, and return both the atlas + per-codepoint shaping
/// result.
///
/// Returns:
///   - 1024×1024 R8 atlas pixel buffer
///   - HashMap<(font_id, glyph_id) → AtlasEntry>
///   - HashMap<char → CellGlyph>  (per unique char in the snapshot)
fn shape_and_rasterize_cells(
    cells: &[Vec<Cell>],
    font_size_px: f32,
) -> Result<
    (
        Vec<u8>,
        HashMap<(cosmic_text::fontdb::ID, u16), AtlasEntry>,
        HashMap<char, CellGlyph>,
    ),
    String,
> {
    let mut system = build_font_system_for_grid()?;
    let mut atlas = vec![0u8; (ATLAS_W * ATLAS_H) as usize];
    let mut entries: HashMap<(cosmic_text::fontdb::ID, u16), AtlasEntry> = HashMap::new();
    let mut shelf_x: u32 = 0;
    let mut shelf_y: u32 = 0;
    let mut shelf_height: u32 = 0;
    let mut swash_cache = SwashCache::new();

    let mut glyph_for_char: HashMap<char, CellGlyph> = HashMap::new();

    // Collect every unique non-space character that appears in the snapshot.
    // Spaces stay as "no glyph instance" — the bg fill already handles whatever
    // was beneath them.
    let mut unique_chars: Vec<char> = cells
        .iter()
        .flat_map(|row| row.iter().map(|c| c.glyph))
        .filter(|c| !c.is_whitespace())
        .collect();
    unique_chars.sort_unstable();
    unique_chars.dedup();

    for ch in unique_chars {
        let mut buf = [0u8; 4];
        let s: &str = ch.encode_utf8(&mut buf);
        let attrs = Attrs::new().family(cosmic_text::Family::Monospace);
        let attrs_list = AttrsList::new(attrs);
        let combined = BidiParagraphs::new(s).collect::<Vec<&str>>().join("\u{200B}");
        let shape_line = ShapeLine::new(&mut system, combined.as_str(), &attrs_list, Shaping::Advanced, 4);
        let layout = shape_line.layout(
            font_size_px,
            Some(ATLAS_W as f32 * 4.0),
            Wrap::None,
            Some(cosmic_text::Align::Left),
            None,
            None,
        );
        let first_line = match layout.into_iter().next() {
            Some(l) => l,
            None => continue,
        };
        let glyph = match first_line.glyphs.first() {
            Some(g) => g,
            None => continue,
        };
        let key = (glyph.font_id, glyph.glyph_id);

        let cache_key = CacheKey::new(
            glyph.font_id,
            glyph.glyph_id,
            font_size_px,
            (0.0, 0.0),
            CacheKeyFlags::empty(),
        )
        .0;
        let image = match swash_cache.get_image_uncached(&mut system, cache_key) {
            Some(img) => img,
            None => continue,
        };

        let mut ascent_for_char = 0.0f32;

        if !entries.contains_key(&key) {
            if image.placement.width == 0 || image.placement.height == 0 {
                entries.insert(
                    key,
                    AtlasEntry {
                        atlas_x: 0,
                        atlas_y: 0,
                        width: 0,
                        height: 0,
                        placement_left: image.placement.left,
                        placement_top: image.placement.top,
                    },
                );
            } else {
                if !matches!(image.content, SwashContent::Mask | SwashContent::SubpixelMask) {
                    continue;
                }
                let gw = image.placement.width;
                let gh = image.placement.height;
                if gw > ATLAS_W || gh > ATLAS_H {
                    return Err(format!(
                        "glyph '{}' too large for atlas ({}x{} > {}x{})",
                        ch, gw, gh, ATLAS_W, ATLAS_H
                    ));
                }
                if shelf_x + gw > ATLAS_W {
                    shelf_x = 0;
                    shelf_y += shelf_height + 1;
                    shelf_height = 0;
                }
                if shelf_y + gh > ATLAS_H {
                    return Err(format!(
                        "atlas full at glyph '{}' (need {}x{} at shelf {},{}, shelf height {}, total atlas {}x{})",
                        ch, gw, gh, shelf_x, shelf_y, shelf_height, ATLAS_W, ATLAS_H
                    ));
                }
                for sy in 0..gh {
                    let dst_row_start = ((shelf_y + sy) * ATLAS_W + shelf_x) as usize;
                    let src_row_start = (sy * gw) as usize;
                    let dst_row = &mut atlas[dst_row_start..dst_row_start + gw as usize];
                    let src_row = &image.data[src_row_start..src_row_start + gw as usize];
                    dst_row.copy_from_slice(src_row);
                }
                entries.insert(
                    key,
                    AtlasEntry {
                        atlas_x: shelf_x,
                        atlas_y: shelf_y,
                        width: gw,
                        height: gh,
                        placement_left: image.placement.left,
                        placement_top: image.placement.top,
                    },
                );
                shelf_x += gw + 1;
                if gh > shelf_height {
                    shelf_height = gh;
                }
                ascent_for_char = image.placement.top as f32;
            }
        } else {
            // Already in atlas — reuse its ascent_top for vertical centering.
            if let Some(entry) = entries.get(&key) {
                ascent_for_char = entry.placement_top as f32;
            }
        }

        glyph_for_char.insert(
            ch,
            CellGlyph {
                glyph_id: glyph.glyph_id,
                font_id: glyph.font_id,
                pen_x: glyph.x,
                pen_y: glyph.y,
                ascent_px: ascent_for_char,
            },
        );
    }

    Ok((atlas, entries, glyph_for_char))
}

/// Build a cosmic-text FontSystem populated from /system/fonts. Same shape
/// as `static_grid::build_font_system_for_grid` — duplicated here rather
/// than re-exported because `font_render` lives in the host crate; the
/// canonical warpui copy keeps the dynamic_grid module self-contained.
fn build_font_system_for_grid() -> Result<FontSystem, String> {
    let mut db = cosmic_text::fontdb::Database::new();
    let mut total = 0usize;
    let mut loaded = 0usize;

    let used_iter = unsafe {
        let iter = ndk_sys::ASystemFontIterator_open();
        if iter.is_null() {
            false
        } else {
            loop {
                let font = ndk_sys::ASystemFontIterator_next(iter);
                if font.is_null() {
                    break;
                }
                let path_ptr = ndk_sys::AFont_getFontFilePath(font);
                if !path_ptr.is_null() {
                    let cstr = CStr::from_ptr(path_ptr);
                    let path = std::path::PathBuf::from(cstr.to_string_lossy().into_owned());
                    total += 1;
                    let ids = db.load_font_source(cosmic_text::fontdb::Source::File(path));
                    loaded += ids.len();
                }
                ndk_sys::AFont_close(font);
            }
            ndk_sys::ASystemFontIterator_close(iter);
            true
        }
    };

    if !used_iter || loaded == 0 {
        let dir = std::path::Path::new("/system/fonts");
        if let Ok(read_dir) = std::fs::read_dir(dir) {
            for entry in read_dir.flatten() {
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
                total += 1;
                let ids = db.load_font_source(cosmic_text::fontdb::Source::File(path));
                loaded += ids.len();
            }
        }
    }

    if loaded == 0 {
        return Err(format!(
            "no fonts loaded (used_iter={} total={})",
            used_iter, total
        ));
    }

    // Prefer a known-monospace family if the device shipped one. We collect
    // the candidate name into a separate String first because the iterator
    // returned by `db.faces()` keeps an immutable borrow live, which would
    // conflict with the `db.set_monospace_family` mutable call.
    let mut mono_name: Option<String> = None;
    for face in db.faces() {
        for (name, _lang) in &face.families {
            if name.eq_ignore_ascii_case("DroidSansMono")
                || name.eq_ignore_ascii_case("Droid Sans Mono")
                || name.eq_ignore_ascii_case("RobotoMono")
                || name.eq_ignore_ascii_case("Roboto Mono")
            {
                mono_name = Some(name.to_string());
                break;
            }
        }
        if mono_name.is_some() {
            break;
        }
    }
    let mono_seen = mono_name.is_some();
    if let Some(name) = mono_name {
        db.set_monospace_family(name);
    }
    // Else fall through; cosmic-text's Family::Monospace resolves through
    // fontdb's monospace heuristic when no explicit family is set.

    log::info!(
        target: "WarpDynamicGrid",
        "font_system built used_iter={} total={} loaded={} monospace_seen={}",
        used_iter, total, loaded, mono_seen
    );

    Ok(FontSystem::new_with_locale_and_db("en-US".to_string(), db))
}

// ---------------------------------------------------------------------------
// Phase 2 — build per-instance vertex buffer from cells
// ---------------------------------------------------------------------------

fn build_dynamic_instances(
    cells: &[Vec<Cell>],
    glyph_for_char: &HashMap<char, CellGlyph>,
    entries: &HashMap<(cosmic_text::fontdb::ID, u16), AtlasEntry>,
    cell_w_px: f32,
    cell_h_px: f32,
) -> Vec<DynInstanceData> {
    let mut out: Vec<DynInstanceData> =
        Vec::with_capacity(cells.iter().map(|r| r.len()).sum::<usize>() * 2);

    for (r, row) in cells.iter().enumerate() {
        let cell_y = r as f32 * cell_h_px;
        for (c, cell) in row.iter().enumerate() {
            let cell_x = c as f32 * cell_w_px;

            // 1. Background quad (only if bg differs from default black).
            if cell.bg != DEFAULT_BG {
                let (br, bg, bb, ba) = unpack_rgba(cell.bg);
                out.push(DynInstanceData {
                    dst_origin_x: 0.0,
                    dst_origin_y: 0.0,
                    dst_size_w: 0.0,
                    dst_size_h: 0.0,
                    atlas_uv_min_u: 0.0,
                    atlas_uv_min_v: 0.0,
                    atlas_uv_max_u: 0.0,
                    atlas_uv_max_v: 0.0,
                    fg_r: 0.0,
                    fg_g: 0.0,
                    fg_b: 0.0,
                    fg_a: 0.0,
                    bg_r: br,
                    bg_g: bg,
                    bg_b: bb,
                    bg_a: ba,
                    cell_origin_x: cell_x,
                    cell_origin_y: cell_y,
                    // Sentinel: cell_size_w < 0 marks a bg quad.
                    cell_size_w: -cell_w_px,
                    cell_size_h: cell_h_px,
                });
            }

            // 2. Glyph quad — skip whitespace/space cells (no glyph to draw).
            if cell.glyph.is_whitespace() {
                continue;
            }
            let cg = match glyph_for_char.get(&cell.glyph) {
                Some(g) => g,
                None => continue,
            };
            let entry = match entries.get(&(cg.font_id, cg.glyph_id)) {
                Some(e) => e,
                None => continue,
            };
            if entry.width == 0 || entry.height == 0 {
                continue;
            }

            // Vertical baseline within the cell — centered. Same heuristic
            // as static_grid but per-character ascent (since each char has
            // its own atlas placement).
            let baseline_within_cell = cell_h_px * 0.5 + cg.ascent_px * 0.5;

            // ATTR_DIM (bit 3) — approximate by halving fg RGB intensity.
            let mut fg = cell.fg;
            if cell.attrs & 0b1000 != 0 {
                let (r, g, b, a) = unpack_rgba_u8(fg);
                fg = pack_rgba_u8(r / 2, g / 2, b / 2, a);
            }
            let (fr, fg_, fb, fa) = unpack_rgba(fg);

            let dst_x = cell_x + cg.pen_x + entry.placement_left as f32;
            let dst_y = cell_y + baseline_within_cell + cg.pen_y - entry.placement_top as f32;
            let dst_w = entry.width as f32;
            let dst_h = entry.height as f32;
            let (uvmin_u, uvmin_v) = entry.uv_min();
            let (uvmax_u, uvmax_v) = entry.uv_max();
            out.push(DynInstanceData {
                dst_origin_x: dst_x,
                dst_origin_y: dst_y,
                dst_size_w: dst_w,
                dst_size_h: dst_h,
                atlas_uv_min_u: uvmin_u,
                atlas_uv_min_v: uvmin_v,
                atlas_uv_max_u: uvmax_u,
                atlas_uv_max_v: uvmax_v,
                fg_r: fr,
                fg_g: fg_,
                fg_b: fb,
                fg_a: fa,
                bg_r: 0.0,
                bg_g: 0.0,
                bg_b: 0.0,
                bg_a: 0.0,
                cell_origin_x: cell_x,
                cell_origin_y: cell_y,
                // cell_size_w >= 0 marks a glyph quad.
                cell_size_w: cell_w_px,
                cell_size_h: cell_h_px,
            });
        }
    }

    out
}

/// Unpack `0xRRGGBBAA` → `(r,g,b,a)` floats in 0..1.
fn unpack_rgba(packed: u32) -> (f32, f32, f32, f32) {
    let r = ((packed >> 24) & 0xFF) as f32 / 255.0;
    let g = ((packed >> 16) & 0xFF) as f32 / 255.0;
    let b = ((packed >> 8) & 0xFF) as f32 / 255.0;
    let a = (packed & 0xFF) as f32 / 255.0;
    (r, g, b, a)
}

fn unpack_rgba_u8(packed: u32) -> (u8, u8, u8, u8) {
    let r = ((packed >> 24) & 0xFF) as u8;
    let g = ((packed >> 16) & 0xFF) as u8;
    let b = ((packed >> 8) & 0xFF) as u8;
    let a = (packed & 0xFF) as u8;
    (r, g, b, a)
}

fn pack_rgba_u8(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (a as u32)
}

// ---------------------------------------------------------------------------
// Phase 3 — Vulkan resource creation (mirrors static_grid; differences
// noted inline)
// ---------------------------------------------------------------------------

unsafe fn find_memory_type(
    instance: &ash::Instance,
    phys_device: vk::PhysicalDevice,
    type_bits: u32,
    properties: vk::MemoryPropertyFlags,
) -> Option<u32> {
    let mem = instance.get_physical_device_memory_properties(phys_device);
    for i in 0..mem.memory_type_count {
        let bit = 1u32 << i;
        if (type_bits & bit) != 0
            && mem.memory_types[i as usize]
                .property_flags
                .contains(properties)
        {
            return Some(i);
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_atlas_image(
    instance: &ash::Instance,
    device: &ash::Device,
    phys_device: vk::PhysicalDevice,
    graphics_queue: vk::Queue,
    command_pool: vk::CommandPool,
    pixels: &[u8],
) -> Result<(vk::Image, vk::DeviceMemory, vk::ImageView), String> {
    let staging_size = pixels.len() as vk::DeviceSize;
    let staging_info = vk::BufferCreateInfo::default()
        .size(staging_size)
        .usage(vk::BufferUsageFlags::TRANSFER_SRC)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let staging_buffer = device
        .create_buffer(&staging_info, None)
        .map_err(|e| format!("create staging buffer: {:?}", e))?;
    let mem_req = device.get_buffer_memory_requirements(staging_buffer);
    let mem_idx = find_memory_type(
        instance,
        phys_device,
        mem_req.memory_type_bits,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )
    .ok_or_else(|| "no HOST_VISIBLE|HOST_COHERENT memory type for staging".to_string())?;
    let alloc = vk::MemoryAllocateInfo::default()
        .allocation_size(mem_req.size)
        .memory_type_index(mem_idx);
    let staging_mem = device
        .allocate_memory(&alloc, None)
        .map_err(|e| format!("allocate staging memory: {:?}", e))?;
    device
        .bind_buffer_memory(staging_buffer, staging_mem, 0)
        .map_err(|e| format!("bind staging memory: {:?}", e))?;
    let mapped = device
        .map_memory(staging_mem, 0, staging_size, vk::MemoryMapFlags::empty())
        .map_err(|e| format!("map staging: {:?}", e))? as *mut u8;
    std::ptr::copy_nonoverlapping(pixels.as_ptr(), mapped, pixels.len());
    device.unmap_memory(staging_mem);

    let img_info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(vk::Format::R8_UNORM)
        .extent(vk::Extent3D {
            width: ATLAS_W,
            height: ATLAS_H,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    let image = device
        .create_image(&img_info, None)
        .map_err(|e| format!("create atlas image: {:?}", e))?;
    let image_mem_req = device.get_image_memory_requirements(image);
    let img_mem_idx = find_memory_type(
        instance,
        phys_device,
        image_mem_req.memory_type_bits,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )
    .ok_or_else(|| "no DEVICE_LOCAL memory type for atlas image".to_string())?;
    let img_alloc = vk::MemoryAllocateInfo::default()
        .allocation_size(image_mem_req.size)
        .memory_type_index(img_mem_idx);
    let image_mem = device
        .allocate_memory(&img_alloc, None)
        .map_err(|e| format!("allocate atlas memory: {:?}", e))?;
    device
        .bind_image_memory(image, image_mem, 0)
        .map_err(|e| format!("bind atlas memory: {:?}", e))?;

    let cmd_alloc = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let cmd_bufs = device
        .allocate_command_buffers(&cmd_alloc)
        .map_err(|e| format!("alloc upload cmd: {:?}", e))?;
    let cmd = cmd_bufs[0];

    let begin = vk::CommandBufferBeginInfo::default()
        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    device
        .begin_command_buffer(cmd, &begin)
        .map_err(|e| format!("begin upload cmd: {:?}", e))?;

    let to_dst = vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1),
        );
    device.cmd_pipeline_barrier(
        cmd,
        vk::PipelineStageFlags::TOP_OF_PIPE,
        vk::PipelineStageFlags::TRANSFER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[to_dst],
    );

    let region = vk::BufferImageCopy::default()
        .buffer_offset(0)
        .buffer_row_length(0)
        .buffer_image_height(0)
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .mip_level(0)
                .base_array_layer(0)
                .layer_count(1),
        )
        .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
        .image_extent(vk::Extent3D {
            width: ATLAS_W,
            height: ATLAS_H,
            depth: 1,
        });
    device.cmd_copy_buffer_to_image(
        cmd,
        staging_buffer,
        image,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        &[region],
    );

    let to_shader = vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1),
        );
    device.cmd_pipeline_barrier(
        cmd,
        vk::PipelineStageFlags::TRANSFER,
        vk::PipelineStageFlags::FRAGMENT_SHADER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[to_shader],
    );

    device
        .end_command_buffer(cmd)
        .map_err(|e| format!("end upload cmd: {:?}", e))?;

    let submit = vk::SubmitInfo::default().command_buffers(&cmd_bufs);
    device
        .queue_submit(graphics_queue, &[submit], vk::Fence::null())
        .map_err(|e| format!("submit upload cmd: {:?}", e))?;
    device
        .queue_wait_idle(graphics_queue)
        .map_err(|e| format!("wait upload cmd: {:?}", e))?;
    device.free_command_buffers(command_pool, &cmd_bufs);

    device.destroy_buffer(staging_buffer, None);
    device.free_memory(staging_mem, None);

    let view_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(vk::Format::R8_UNORM)
        .components(vk::ComponentMapping::default())
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1),
        );
    let view = device
        .create_image_view(&view_info, None)
        .map_err(|e| format!("create atlas view: {:?}", e))?;

    Ok((image, image_mem, view))
}

unsafe fn create_atlas_sampler(device: &ash::Device) -> Result<vk::Sampler, String> {
    let info = vk::SamplerCreateInfo::default()
        .mag_filter(vk::Filter::LINEAR)
        .min_filter(vk::Filter::LINEAR)
        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
        .min_lod(0.0)
        .max_lod(0.0)
        .anisotropy_enable(false)
        .max_anisotropy(1.0);
    device
        .create_sampler(&info, None)
        .map_err(|e| format!("create atlas sampler: {:?}", e))
}

unsafe fn create_instance_buffer(
    instance: &ash::Instance,
    device: &ash::Device,
    phys_device: vk::PhysicalDevice,
    instances: &[DynInstanceData],
) -> Result<(vk::Buffer, vk::DeviceMemory, vk::DeviceSize), String> {
    if instances.is_empty() {
        return Err("instance buffer would be empty".into());
    }
    let buffer_size = (instances.len() * std::mem::size_of::<DynInstanceData>()) as vk::DeviceSize;
    let info = vk::BufferCreateInfo::default()
        .size(buffer_size)
        .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = device
        .create_buffer(&info, None)
        .map_err(|e| format!("create instance buffer: {:?}", e))?;
    let mem_req = device.get_buffer_memory_requirements(buffer);
    let mem_idx = find_memory_type(
        instance,
        phys_device,
        mem_req.memory_type_bits,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )
    .ok_or_else(|| "no HOST_VISIBLE memory type for instance buffer".to_string())?;
    let alloc = vk::MemoryAllocateInfo::default()
        .allocation_size(mem_req.size)
        .memory_type_index(mem_idx);
    let memory = device
        .allocate_memory(&alloc, None)
        .map_err(|e| format!("allocate instance memory: {:?}", e))?;
    device
        .bind_buffer_memory(buffer, memory, 0)
        .map_err(|e| format!("bind instance memory: {:?}", e))?;
    let mapped = device
        .map_memory(memory, 0, buffer_size, vk::MemoryMapFlags::empty())
        .map_err(|e| format!("map instance memory: {:?}", e))? as *mut u8;
    let src_bytes = std::slice::from_raw_parts(
        instances.as_ptr() as *const u8,
        instances.len() * std::mem::size_of::<DynInstanceData>(),
    );
    std::ptr::copy_nonoverlapping(src_bytes.as_ptr(), mapped, src_bytes.len());
    device.unmap_memory(memory);

    Ok((buffer, memory, buffer_size))
}

unsafe fn create_descriptor_set_layout(
    device: &ash::Device,
) -> Result<vk::DescriptorSetLayout, String> {
    let bindings = [vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    device
        .create_descriptor_set_layout(&info, None)
        .map_err(|e| format!("create descriptor set layout: {:?}", e))
}

unsafe fn create_pipeline_layout(
    device: &ash::Device,
    set_layout: vk::DescriptorSetLayout,
) -> Result<vk::PipelineLayout, String> {
    let set_layouts = [set_layout];
    let push_constant_ranges = [vk::PushConstantRange::default()
        .stage_flags(vk::ShaderStageFlags::VERTEX)
        .offset(0)
        .size(std::mem::size_of::<PushConstants>() as u32)];
    let info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(&set_layouts)
        .push_constant_ranges(&push_constant_ranges);
    device
        .create_pipeline_layout(&info, None)
        .map_err(|e| format!("create pipeline layout: {:?}", e))
}

unsafe fn create_descriptor_set(
    device: &ash::Device,
    set_layout: vk::DescriptorSetLayout,
    image_view: vk::ImageView,
    sampler: vk::Sampler,
) -> Result<(vk::DescriptorPool, vk::DescriptorSet), String> {
    let pool_sizes = [vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .descriptor_count(1)];
    let pool_info = vk::DescriptorPoolCreateInfo::default()
        .pool_sizes(&pool_sizes)
        .max_sets(1);
    let pool = device
        .create_descriptor_pool(&pool_info, None)
        .map_err(|e| format!("create descriptor pool: {:?}", e))?;

    let layouts = [set_layout];
    let alloc = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(pool)
        .set_layouts(&layouts);
    let sets = device
        .allocate_descriptor_sets(&alloc)
        .map_err(|e| format!("allocate descriptor set: {:?}", e))?;
    let set = sets[0];

    let image_info = [vk::DescriptorImageInfo::default()
        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .image_view(image_view)
        .sampler(sampler)];
    let write = vk::WriteDescriptorSet::default()
        .dst_set(set)
        .dst_binding(0)
        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .image_info(&image_info);
    device.update_descriptor_sets(&[write], &[]);

    Ok((pool, set))
}

unsafe fn create_shader_module(
    device: &ash::Device,
    spv: &[u8],
) -> Result<vk::ShaderModule, String> {
    if spv.len() % 4 != 0 {
        return Err(format!("SPV size {} not multiple of 4", spv.len()));
    }
    let word_count = spv.len() / 4;
    let mut words: Vec<u32> = Vec::with_capacity(word_count);
    let src = spv.as_ptr();
    let dst = words.as_mut_ptr() as *mut u8;
    std::ptr::copy_nonoverlapping(src, dst, spv.len());
    words.set_len(word_count);

    let info = vk::ShaderModuleCreateInfo::default().code(&words);
    device
        .create_shader_module(&info, None)
        .map_err(|e| format!("create shader module: {:?}", e))
}

unsafe fn create_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    render_pass: vk::RenderPass,
) -> Result<(vk::Pipeline, vk::ShaderModule, vk::ShaderModule), String> {
    let vert = create_shader_module(device, VERT_SPV)?;
    let frag = create_shader_module(device, FRAG_SPV)?;

    let entry_name = CStr::from_bytes_with_nul(b"main\0").unwrap();
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vert)
            .name(entry_name),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(frag)
            .name(entry_name),
    ];

    // 1 per-instance binding; 8 vertex attributes (loc 0..7).
    let stride = std::mem::size_of::<DynInstanceData>() as u32;
    let binding_desc = [vk::VertexInputBindingDescription {
        binding: 0,
        stride,
        input_rate: vk::VertexInputRate::INSTANCE,
    }];
    let attr_desc = [
        vk::VertexInputAttributeDescription {
            location: 0,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: 0,
        },
        vk::VertexInputAttributeDescription {
            location: 1,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: 8,
        },
        vk::VertexInputAttributeDescription {
            location: 2,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: 16,
        },
        vk::VertexInputAttributeDescription {
            location: 3,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: 24,
        },
        vk::VertexInputAttributeDescription {
            location: 4,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: 32,
        },
        vk::VertexInputAttributeDescription {
            location: 5,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: 48,
        },
        vk::VertexInputAttributeDescription {
            location: 6,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: 64,
        },
        vk::VertexInputAttributeDescription {
            location: 7,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: 72,
        },
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&binding_desc)
        .vertex_attribute_descriptions(&attr_desc);

    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_STRIP)
        .primitive_restart_enable(false);

    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

    let viewports = [vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
        min_depth: 0.0,
        max_depth: 1.0,
    }];
    let scissors = [vk::Rect2D {
        offset: vk::Offset2D { x: 0, y: 0 },
        extent: vk::Extent2D {
            width: 1,
            height: 1,
        },
    }];
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewports(&viewports)
        .scissors(&scissors);

    let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
        .depth_clamp_enable(false)
        .rasterizer_discard_enable(false)
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .depth_bias_enable(false)
        .line_width(1.0);

    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1)
        .sample_shading_enable(false)
        .alpha_to_coverage_enable(false)
        .alpha_to_one_enable(false);

    // Premultiplied alpha blending — frag shader outputs (rgb*a, a), so we use
    // ONE / ONE_MINUS_SRC_ALPHA on the dst side.
    let attach = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(
            vk::ColorComponentFlags::R
                | vk::ColorComponentFlags::G
                | vk::ColorComponentFlags::B
                | vk::ColorComponentFlags::A,
        )];
    let blend = vk::PipelineColorBlendStateCreateInfo::default()
        .logic_op_enable(false)
        .logic_op(vk::LogicOp::COPY)
        .attachments(&attach)
        .blend_constants([0.0, 0.0, 0.0, 0.0]);

    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterizer)
        .multisample_state(&multisample)
        .color_blend_state(&blend)
        .dynamic_state(&dynamic_state)
        .layout(layout)
        .render_pass(render_pass)
        .subpass(0);

    let pipelines = device
        .create_graphics_pipelines(vk::PipelineCache::null(), &[info], None)
        .map_err(|(_, e)| format!("create graphics pipeline: {:?}", e))?;
    Ok((pipelines[0], vert, frag))
}

/// Pre-init / fully-blank-grid helper. Allocates a 1-instance vertex buffer
/// containing a 0-alpha glyph quad (the shader discards it) so the pipeline
/// is ready for the first non-empty model snapshot without re-creating
/// pipeline+atlas objects.
#[allow(clippy::too_many_arguments)]
unsafe fn build_empty(
    instance: &ash::Instance,
    device: &ash::Device,
    phys_device: vk::PhysicalDevice,
    graphics_queue: vk::Queue,
    command_pool: vk::CommandPool,
    render_pass: vk::RenderPass,
    atlas_pixels: &[u8],
    atlas_entries: &HashMap<(cosmic_text::fontdb::ID, u16), AtlasEntry>,
    glyph_for_char: HashMap<char, CellGlyph>,
    rows: u32,
    cols: u32,
    cell_w_px: f32,
    cell_h_px: f32,
    font_size_px: f32,
) -> Result<DynamicGrid, String> {
    let placeholder = vec![DynInstanceData {
        dst_origin_x: 0.0,
        dst_origin_y: 0.0,
        dst_size_w: 0.0,
        dst_size_h: 0.0,
        atlas_uv_min_u: 0.0,
        atlas_uv_min_v: 0.0,
        atlas_uv_max_u: 0.0,
        atlas_uv_max_v: 0.0,
        fg_r: 0.0,
        fg_g: 0.0,
        fg_b: 0.0,
        fg_a: 0.0,
        bg_r: 0.0,
        bg_g: 0.0,
        bg_b: 0.0,
        bg_a: 0.0,
        cell_origin_x: 0.0,
        cell_origin_y: 0.0,
        cell_size_w: cell_w_px,
        cell_size_h: cell_h_px,
    }];

    let (atlas_image, atlas_image_memory, atlas_image_view) =
        create_atlas_image(instance, device, phys_device, graphics_queue, command_pool, atlas_pixels)?;
    let atlas_sampler = create_atlas_sampler(device)?;
    let (instance_buffer, instance_buffer_memory, instance_buffer_capacity) =
        create_instance_buffer(instance, device, phys_device, &placeholder)?;
    let descriptor_set_layout = create_descriptor_set_layout(device)?;
    let pipeline_layout = create_pipeline_layout(device, descriptor_set_layout)?;
    let (descriptor_pool, descriptor_set) =
        create_descriptor_set(device, descriptor_set_layout, atlas_image_view, atlas_sampler)?;
    let (pipeline, vert_module, frag_module) =
        create_pipeline(device, pipeline_layout, render_pass)?;

    Ok(DynamicGrid {
        atlas_image,
        atlas_image_memory,
        atlas_image_view,
        atlas_sampler,
        instance_buffer,
        instance_buffer_memory,
        instance_buffer_capacity,
        // The placeholder is only there so Vulkan validation has something
        // to validate. The shader's alpha-< 0.01 discard makes it draw
        // nothing.
        instance_count: 0,
        descriptor_set_layout,
        descriptor_pool,
        descriptor_set,
        pipeline_layout,
        pipeline,
        vert_module,
        frag_module,
        cached_glyph_for_char: glyph_for_char,
        cached_atlas_entries: atlas_entries.clone(),
        glyphs_per_frame: 0,
        atlas_glyph_count: atlas_entries.len() as u32,
        grid_rows: rows,
        grid_cols: cols,
        bg_quads_per_frame: 0,
        cell_w_px,
        cell_h_px,
        font_size_px,
        fast_path_updates: 0,
        full_reinits: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpack_rgba_round_trips() {
        let (r, g, b, a) = unpack_rgba(0xCC0000FF);
        assert!((r - (0xCC as f32 / 255.0)).abs() < 1e-3);
        assert_eq!(g, 0.0);
        assert_eq!(b, 0.0);
        assert_eq!(a, 1.0);
    }

    #[test]
    fn instance_struct_size_matches_shader_attrs() {
        // 4 vec2 + 2 vec4 + 2 vec2 = (8 + 8 + 8 + 8) + (16 + 16) + (8 + 8) = 80 bytes
        assert_eq!(std::mem::size_of::<DynInstanceData>(), 80);
    }

    #[test]
    fn dim_attr_halves_fg_intensity() {
        let raw = pack_rgba_u8(0xFF, 0xFF, 0xFF, 0xFF);
        let (r, g, b, a) = unpack_rgba_u8(raw);
        let dim = pack_rgba_u8(r / 2, g / 2, b / 2, a);
        let (dr, dg, db, da) = unpack_rgba_u8(dim);
        assert_eq!(dr, 0x7F);
        assert_eq!(dg, 0x7F);
        assert_eq!(db, 0x7F);
        assert_eq!(da, 0xFF);
    }

    #[test]
    fn build_dynamic_instances_skips_default_cells() {
        let cells: Vec<Vec<Cell>> = (0..3).map(|_| vec![Cell::blank(); 5]).collect();
        let glyph_for_char = HashMap::new();
        let entries = HashMap::new();
        let instances = build_dynamic_instances(&cells, &glyph_for_char, &entries, 10.0, 20.0);
        // All-default cells = no instances (no bg differs from default, no
        // glyphs are non-whitespace).
        assert!(instances.is_empty(), "got {} instances", instances.len());
    }

    #[test]
    fn build_dynamic_instances_emits_bg_quad_for_red_bg() {
        let mut cells: Vec<Vec<Cell>> = (0..1).map(|_| vec![Cell::blank(); 1]).collect();
        cells[0][0] = Cell {
            glyph: ' ',
            fg: DEFAULT_FG,
            bg: 0xFF0000FF, // red bg
            attrs: 0,
        };
        let glyph_for_char = HashMap::new();
        let entries = HashMap::new();
        let instances = build_dynamic_instances(&cells, &glyph_for_char, &entries, 10.0, 20.0);
        assert_eq!(instances.len(), 1);
        assert!(instances[0].cell_size_w < 0.0, "expected bg quad sentinel");
        assert!((instances[0].bg_r - 1.0).abs() < 1e-3);
        assert!(instances[0].bg_g.abs() < 1e-3);
    }
}
