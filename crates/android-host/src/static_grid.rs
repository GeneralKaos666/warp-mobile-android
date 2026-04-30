//! M2-S08 — static glyph grid GPU rendering pipeline.
//!
//! Renders a fixed M×N grid of "Hello, World" (or any caller-supplied text)
//! at 60fps p95<16.6ms by:
//!
//! 1. **Pre-rasterizing every unique glyph** of the text via cosmic-text +
//!    swash into an alpha-only (R8_UNORM) atlas texture (one-shot at init).
//! 2. **Pre-building a per-instance vertex buffer** with one entry per glyph
//!    in the grid (rows × cols × glyphs_per_string). Each entry carries the
//!    destination quad (x,y,w,h) in pixel space + the atlas UV bounds.
//! 3. **Per-frame draw** = 1× `vkCmdDrawIndexed`-equivalent (we use
//!    `vkCmdDraw` with vertex_count=4 + instance_count=N because the quad
//!    geometry is corner-derived in the vertex shader from `gl_VertexIndex`,
//!    saving an index-buffer bind).
//!
//! Pipeline state:
//!   - Vertex stage reads instance data via vertex attribute bindings (rate
//!     = `VK_VERTEX_INPUT_RATE_INSTANCE`).
//!   - Fragment stage samples the alpha atlas and outputs (1,1,1, alpha).
//!   - Standard alpha blending: src.rgba * src.a + dst.rgba * (1 - src.a).
//!   - No depth buffer (text is single-layer over the clear color).
//!
//! Web-search refs (2026-04-30):
//! - cosmic-text + swash glyph atlas pattern (glyphon/wgpu reference):
//!   <https://github.com/grovesNL/glyphon>
//! - Modern bindless sprite-batch / instanced rendering:
//!   <https://jorenjoestar.github.io/post/modern_sprite_batch/>
//! - Vulkan tutorial — pipeline, descriptor sets, samplers:
//!   <https://kylemayes.github.io/vulkanalia/pipeline/shader_modules.html>
//! - SPIR-V shader compilation at build time:
//!   <https://falseidolfactory.mistodon.com/2018/06/23/compiling-glsl-to-spirv-at-build-time.html>
//! - SPIR-V (Khronos):
//!   <https://docs.vulkan.org/guide/latest/what_is_spirv.html>

#![cfg(target_os = "android")]

use std::collections::HashMap;
use std::ffi::CStr;

use ash::vk;
use cosmic_text::{
    Attrs, AttrsList, BidiParagraphs, CacheKey, CacheKeyFlags, FontSystem, ShapeLine, Shaping,
    SwashCache, SwashContent, Wrap,
};

/// SPIR-V bytecode embedded at build time. See `build.rs` for the compilation
/// pipeline (NDK glslc → OUT_DIR/*.spv).
const VERT_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/grid.vert.spv"));
const FRAG_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/grid.frag.spv"));

/// Atlas dimensions in pixels. 1024×1024 R8 = 1 MiB. Plenty of headroom for
/// the ~26-character Latin alphabet at 32-48px font size; we lay glyphs in
/// horizontal rows with shelf packing.
const ATLAS_W: u32 = 1024;
const ATLAS_H: u32 = 1024;

/// Per-instance vertex attributes (4 vec2s, layout matches grid.vert).
///
/// `repr(C)` for predictable field layout matching the SPIR-V vertex input
/// declarations.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct InstanceData {
    dst_origin_x: f32,
    dst_origin_y: f32,
    dst_size_w: f32,
    dst_size_h: f32,
    atlas_uv_min_u: f32,
    atlas_uv_min_v: f32,
    atlas_uv_max_u: f32,
    atlas_uv_max_v: f32,
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

/// All GPU resources owned by the static-grid renderer. One instance per
/// VulkanSurface; recreated only on full surface swap (NOT on swapchain
/// recreate — the pipeline + atlas + instance buffer are independent of the
/// swapchain images).
pub(crate) struct StaticGrid {
    // Atlas
    atlas_image: vk::Image,
    atlas_image_memory: vk::DeviceMemory,
    atlas_image_view: vk::ImageView,
    atlas_sampler: vk::Sampler,
    // Per-instance vertex buffer
    instance_buffer: vk::Buffer,
    instance_buffer_memory: vk::DeviceMemory,
    instance_count: u32,
    // Pipeline
    descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set: vk::DescriptorSet,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    /// Vertex/fragment shader modules (kept around for cleanup).
    vert_module: vk::ShaderModule,
    frag_module: vk::ShaderModule,
    // Diagnostics
    pub(crate) glyphs_per_frame: u32,
    pub(crate) atlas_glyph_count: u32,
    pub(crate) grid_rows: u32,
    pub(crate) grid_cols: u32,
    pub(crate) text_per_cell: String,
}

impl StaticGrid {
    /// Builds a StaticGrid from the given text + grid dims. Performs all
    /// expensive work synchronously: cosmic-text shaping, swash rasterization,
    /// atlas packing, atlas + instance buffer GPU upload, pipeline creation.
    ///
    /// # Safety
    ///
    /// Caller owns `device` + must keep it alive longer than the returned
    /// StaticGrid. `render_pass` must be compatible with the runtime
    /// VulkanSurface render pass (same color attachment format).
    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe fn new(
        instance: &ash::Instance,
        device: &ash::Device,
        phys_device: vk::PhysicalDevice,
        graphics_queue: vk::Queue,
        command_pool: vk::CommandPool,
        render_pass: vk::RenderPass,
        text_per_cell: &str,
        font_size_px: f32,
        rows: u32,
        cols: u32,
        cell_w_px: f32,
        cell_h_px: f32,
    ) -> Result<Self, String> {
        log::info!(
            target: "WarpStaticGrid",
            "init begin text={:?} rows={} cols={} cell={}x{}px font_size_px={}",
            text_per_cell, rows, cols, cell_w_px, cell_h_px, font_size_px
        );

        // ── 1. Shape one row + rasterize unique glyphs into atlas. ──────────
        let (atlas_pixels, glyph_entries, layout_glyphs, ascent_px) =
            shape_and_rasterize(text_per_cell, font_size_px)?;

        // ── 2. Build per-instance vertex data for rows*cols cells. ──────────
        let instances =
            build_instances(&layout_glyphs, &glyph_entries, rows, cols, cell_w_px, cell_h_px, ascent_px);
        let instance_count = instances.len() as u32;
        let glyphs_per_cell = layout_glyphs.len();
        log::info!(
            target: "WarpStaticGrid",
            "atlas glyphs={} per_cell_glyphs={} grid={}x{} total_instances={}",
            glyph_entries.len(), glyphs_per_cell, rows, cols, instance_count
        );

        // ── 3. Upload atlas + instance buffer. ──────────────────────────────
        log::info!(target: "WarpStaticGrid", "step3a: create_atlas_image begin");
        let (atlas_image, atlas_image_memory, atlas_image_view) =
            create_atlas_image(instance, device, phys_device, graphics_queue, command_pool, &atlas_pixels)?;
        log::info!(target: "WarpStaticGrid", "step3a: create_atlas_image ok");
        let atlas_sampler = create_atlas_sampler(device)?;
        log::info!(target: "WarpStaticGrid", "step3b: create_atlas_sampler ok");

        let (instance_buffer, instance_buffer_memory) =
            create_instance_buffer(instance, device, phys_device, &instances)?;
        log::info!(target: "WarpStaticGrid", "step3c: create_instance_buffer ok");

        // ── 4. Pipeline + descriptors. ──────────────────────────────────────
        let descriptor_set_layout = create_descriptor_set_layout(device)?;
        log::info!(target: "WarpStaticGrid", "step4a: descriptor_set_layout ok");
        let pipeline_layout = create_pipeline_layout(device, descriptor_set_layout)?;
        log::info!(target: "WarpStaticGrid", "step4b: pipeline_layout ok");
        let (descriptor_pool, descriptor_set) =
            create_descriptor_set(device, descriptor_set_layout, atlas_image_view, atlas_sampler)?;
        log::info!(target: "WarpStaticGrid", "step4c: descriptor_set ok");
        let (pipeline, vert_module, frag_module) =
            create_pipeline(device, pipeline_layout, render_pass)?;
        log::info!(target: "WarpStaticGrid", "step4d: pipeline ok");

        log::info!(
            target: "WarpStaticGrid",
            "init ok atlas_glyphs={} instances={} pipeline=created",
            glyph_entries.len(), instance_count
        );

        Ok(Self {
            atlas_image,
            atlas_image_memory,
            atlas_image_view,
            atlas_sampler,
            instance_buffer,
            instance_buffer_memory,
            instance_count,
            descriptor_set_layout,
            descriptor_pool,
            descriptor_set,
            pipeline_layout,
            pipeline,
            vert_module,
            frag_module,
            glyphs_per_frame: instance_count,
            atlas_glyph_count: glyph_entries.len() as u32,
            grid_rows: rows,
            grid_cols: cols,
            text_per_cell: text_per_cell.to_string(),
        })
    }

    /// Records the draw call into `cmd_buf` (assumed to be inside a render
    /// pass already begun by the caller). Caller is responsible for begin
    /// command buffer + begin render pass + end render pass + end command
    /// buffer.
    pub(crate) unsafe fn record_draw(
        &self,
        device: &ash::Device,
        cmd_buf: vk::CommandBuffer,
        viewport_w: f32,
        viewport_h: f32,
    ) {
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

        // Bind descriptor set (atlas sampler).
        device.cmd_bind_descriptor_sets(
            cmd_buf,
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline_layout,
            0,
            &[self.descriptor_set],
            &[],
        );

        // Bind per-instance buffer at binding=0.
        let offsets = [0u64];
        device.cmd_bind_vertex_buffers(cmd_buf, 0, &[self.instance_buffer], &offsets);

        // Push constants — viewport size for NDC conversion.
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

        // Draw: 4 verts (quad) × N instances (one per glyph). The vertex
        // shader generates corner positions from gl_VertexIndex, so no
        // explicit vertex buffer for geometry is needed.
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
// Phase 1 — shape text + rasterize glyphs into atlas
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct LayoutGlyph {
    /// Map key into the atlas entry table — composed of (font_id, glyph_id).
    /// We use font_id+glyph_id because all glyphs in our static grid use the
    /// same font_size_px, so we don't need it in the key.
    font_id: cosmic_text::fontdb::ID,
    glyph_id: u16,
    /// Pen position relative to the line's start (baseline_y assumed at 0).
    /// `pen_y` matches the cosmic-text vertical offset (always 0 for a single
    /// line).
    pen_x: f32,
    pen_y: f32,
}

/// Shape `text` with cosmic-text, then rasterize each unique glyph into a
/// shelf-packed atlas. Returns:
///   - 1024×1024 R8 atlas pixel buffer
///   - HashMap<(font_id, glyph_id) → AtlasEntry>
///   - Vec<LayoutGlyph> for one full text line
///   - ascent_px for vertical positioning
fn shape_and_rasterize(
    text: &str,
    font_size_px: f32,
) -> Result<
    (
        Vec<u8>,
        HashMap<(cosmic_text::fontdb::ID, u16), AtlasEntry>,
        Vec<LayoutGlyph>,
        f32,
    ),
    String,
> {
    // Build a FontSystem populated from /system/fonts via ASystemFontIterator
    // (or directory scan fallback). We reuse the helper from font_render.rs.
    let mut system = build_font_system_for_grid()?;

    // Shape one line.
    let combined = BidiParagraphs::new(text)
        .collect::<Vec<&str>>()
        .join("\u{200B}");
    // Use a Roboto/sans-serif default (cosmic-text's Family::SansSerif).
    let attrs = Attrs::new().family(cosmic_text::Family::SansSerif);
    let attrs_list = AttrsList::new(attrs);
    let shape_line = ShapeLine::new(&mut system, combined.as_str(), &attrs_list, Shaping::Advanced, 4);
    let max_width = ATLAS_W as f32 * 4.0;
    let layout = shape_line.layout(font_size_px, Some(max_width), Wrap::None, Some(cosmic_text::Align::Left), None, None);
    let first_line = layout
        .into_iter()
        .next()
        .ok_or_else(|| "ShapeLine produced no LayoutLines".to_string())?;

    let glyphs_n = first_line.glyphs.len();
    if glyphs_n == 0 {
        return Err(format!("text {:?} produced 0 glyphs", text));
    }

    // Rasterize each unique (font_id, glyph_id) once via swash, pack into the
    // atlas via shelf-packing.
    let mut atlas = vec![0u8; (ATLAS_W * ATLAS_H) as usize];
    let mut entries: HashMap<(cosmic_text::fontdb::ID, u16), AtlasEntry> = HashMap::new();
    let mut shelf_x: u32 = 0;
    let mut shelf_y: u32 = 0;
    let mut shelf_height: u32 = 0;

    let mut swash_cache = SwashCache::new();
    let mut layout_glyphs: Vec<LayoutGlyph> = Vec::with_capacity(glyphs_n);
    let mut ascent_max: f32 = 0.0;
    for glyph in &first_line.glyphs {
        let key = (glyph.font_id, glyph.glyph_id);
        layout_glyphs.push(LayoutGlyph {
            font_id: glyph.font_id,
            glyph_id: glyph.glyph_id,
            pen_x: glyph.x,
            pen_y: glyph.y,
        });
        if entries.contains_key(&key) {
            continue;
        }
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
        if image.placement.width == 0 || image.placement.height == 0 {
            // Skip whitespace / zero-area glyphs — they advance pen but draw
            // nothing. Insert a zero-area entry so layout still increments.
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
            continue;
        }
        // Only support Mask + SubpixelMask atlas entries (alpha). Color emoji
        // would need a separate atlas + pipeline; M2-S08 acceptance text is
        // pure ASCII so this is safe.
        if !matches!(image.content, SwashContent::Mask | SwashContent::SubpixelMask) {
            continue;
        }
        let gw = image.placement.width;
        let gh = image.placement.height;
        if gw > ATLAS_W || gh > ATLAS_H {
            return Err(format!(
                "glyph {} too large for atlas ({}x{} > {}x{})",
                glyph.glyph_id, gw, gh, ATLAS_W, ATLAS_H
            ));
        }
        if shelf_x + gw > ATLAS_W {
            // Wrap to next shelf.
            shelf_x = 0;
            shelf_y += shelf_height + 1;
            shelf_height = 0;
        }
        if shelf_y + gh > ATLAS_H {
            return Err(format!(
                "atlas full at glyph {} (need {}x{} at shelf {},{}, shelf height {}, total atlas {}x{})",
                glyph.glyph_id, gw, gh, shelf_x, shelf_y, shelf_height, ATLAS_W, ATLAS_H
            ));
        }
        // Blit grayscale alpha bytes into atlas.
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
        shelf_x += gw + 1; // 1-pixel padding
        if gh > shelf_height {
            shelf_height = gh;
        }
        // Track max ascent for vertical centering of text within cells.
        if image.placement.top as f32 > ascent_max {
            ascent_max = image.placement.top as f32;
        }
    }

    Ok((atlas, entries, layout_glyphs, ascent_max))
}

/// Build a cosmic-text FontSystem populated from /system/fonts. Mirrors the
/// font_render::build_font_system but stripped of CJK fallback discovery —
/// the M2-S08 acceptance text is pure ASCII "Hello, World".
fn build_font_system_for_grid() -> Result<FontSystem, String> {
    let mut db = cosmic_text::fontdb::Database::new();
    let mut total = 0usize;
    let mut loaded = 0usize;

    // Try ASystemFontIterator first.
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
        if let Ok(entries) = std::fs::read_dir(dir) {
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

    // Set sans-serif default to "Roboto" if loaded — Android system default
    // since API 21+. cosmic-text's Family::SansSerif resolves through this.
    let mut roboto_seen = false;
    for face in db.faces() {
        for (name, _lang) in &face.families {
            if name.eq_ignore_ascii_case("Roboto") {
                roboto_seen = true;
                break;
            }
        }
        if roboto_seen {
            break;
        }
    }
    if roboto_seen {
        db.set_sans_serif_family("Roboto");
    }

    log::info!(
        target: "WarpStaticGrid",
        "font_system built used_iter={} total={} loaded={} sans={:?}",
        used_iter, total, loaded, if roboto_seen { Some("Roboto") } else { None }
    );

    Ok(FontSystem::new_with_locale_and_db("en-US".to_string(), db))
}

// ---------------------------------------------------------------------------
// Phase 2 — build per-instance vertex buffer
// ---------------------------------------------------------------------------

fn build_instances(
    layout: &[LayoutGlyph],
    entries: &HashMap<(cosmic_text::fontdb::ID, u16), AtlasEntry>,
    rows: u32,
    cols: u32,
    cell_w_px: f32,
    cell_h_px: f32,
    ascent_px: f32,
) -> Vec<InstanceData> {
    let mut out: Vec<InstanceData> =
        Vec::with_capacity((rows as usize) * (cols as usize) * layout.len());

    // Vertical baseline within each cell — center the line vertically. The
    // layout gives glyphs in pen-space with y=0 at the baseline; we offset
    // by `cell_h_px / 2 + ascent_px / 2` so the text's vertical mid is at the
    // cell's vertical mid.
    let baseline_within_cell = cell_h_px * 0.5 + ascent_px * 0.5;

    for r in 0..rows {
        let cell_y = r as f32 * cell_h_px;
        for c in 0..cols {
            let cell_x = c as f32 * cell_w_px;
            for g in layout {
                let entry = match entries.get(&(g.font_id, g.glyph_id)) {
                    Some(e) => e,
                    None => continue,
                };
                if entry.width == 0 || entry.height == 0 {
                    continue;
                }
                // Pixel-space top-left of this glyph's quad.
                //   pen_x + placement_left  = top-left X in pen space
                //   pen_y - placement_top   = top-left Y in pen space (Y down)
                // pen origin within the cell = (cell_x, cell_y + baseline_within_cell).
                let dst_x = cell_x + g.pen_x + entry.placement_left as f32;
                let dst_y =
                    cell_y + baseline_within_cell + g.pen_y - entry.placement_top as f32;
                let dst_w = entry.width as f32;
                let dst_h = entry.height as f32;
                let (uvmin_u, uvmin_v) = entry.uv_min();
                let (uvmax_u, uvmax_v) = entry.uv_max();
                out.push(InstanceData {
                    dst_origin_x: dst_x,
                    dst_origin_y: dst_y,
                    dst_size_w: dst_w,
                    dst_size_h: dst_h,
                    atlas_uv_min_u: uvmin_u,
                    atlas_uv_min_v: uvmin_v,
                    atlas_uv_max_u: uvmax_u,
                    atlas_uv_max_v: uvmax_v,
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Phase 3 — Vulkan resource creation
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

    // Create the GPU-only R8_UNORM atlas image.
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

    // Record a one-shot command buffer:
    //   UNDEFINED -> TRANSFER_DST_OPTIMAL
    //   vkCmdCopyBufferToImage
    //   TRANSFER_DST_OPTIMAL -> SHADER_READ_ONLY_OPTIMAL
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

    // Free staging.
    device.destroy_buffer(staging_buffer, None);
    device.free_memory(staging_mem, None);

    // Image view for descriptor.
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
    // Linear filtering on alpha mask + clamp-to-edge so neighboring atlas
    // entries don't bleed.
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
    instances: &[InstanceData],
) -> Result<(vk::Buffer, vk::DeviceMemory), String> {
    if instances.is_empty() {
        return Err("instance buffer would be empty".into());
    }
    let buffer_size = (instances.len() * std::mem::size_of::<InstanceData>()) as vk::DeviceSize;
    let info = vk::BufferCreateInfo::default()
        .size(buffer_size)
        .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = device
        .create_buffer(&info, None)
        .map_err(|e| format!("create instance buffer: {:?}", e))?;
    let mem_req = device.get_buffer_memory_requirements(buffer);
    // Use HOST_VISIBLE|HOST_COHERENT — the instance buffer is uploaded once;
    // on Adreno mobile these flags map to LPDDR-cached and have similar perf
    // to DEVICE_LOCAL for read-only access. Keeps the code path simple
    // (no second staging copy).
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
        instances.len() * std::mem::size_of::<InstanceData>(),
    );
    std::ptr::copy_nonoverlapping(src_bytes.as_ptr(), mapped, src_bytes.len());
    device.unmap_memory(memory);

    Ok((buffer, memory))
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
    // The bytes loaded via include_bytes! may not be 4-byte aligned in the
    // binary (and Rust 1.83+ enforces this with a runtime precondition check
    // in `slice::from_raw_parts`). Copy into a freshly-allocated Vec<u32>
    // which is guaranteed 4-byte aligned. This is a one-shot copy at init,
    // not on the per-frame hot path.
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

    // Vertex input — one per-instance binding, four vec2 attributes.
    let binding_desc = [vk::VertexInputBindingDescription {
        binding: 0,
        stride: std::mem::size_of::<InstanceData>() as u32,
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
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&binding_desc)
        .vertex_attribute_descriptions(&attr_desc);

    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_STRIP)
        .primitive_restart_enable(false);

    // Dynamic viewport + scissor — set per-frame in record_draw.
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

    // Placeholder — filled in by dynamic state at draw time.
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

    // Standard alpha blending: src.a * src.rgb + (1 - src.a) * dst.rgb.
    let attach = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
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
