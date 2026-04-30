#version 450

// M3-S08 vertex shader for the per-cell dynamic grid renderer
// (android-host runtime mirror; canonical lives in
// `warp-src/crates/warpui/shaders/android/dynamic_grid.vert`).
//
// Same quad/instance topology as the M2-S08 static grid shader (4-vertex
// triangle strip, gl_VertexIndex → corner lookup, per-instance dst quad +
// atlas UV bounds), with two extra per-instance vertex attributes carrying
// foreground / background RGBA colors. This way each cell can paint a
// different glyph color without having to swap pipelines or descriptors.
//
// Per-instance layout (matches `DynInstanceData` in `dynamic_grid.rs`):
//   loc 0: vec2 dst_origin   — top-left destination pixel of the cell quad
//   loc 1: vec2 dst_size     — destination quad size in pixels
//   loc 2: vec2 atlas_uv_min — atlas top-left UV (normalized 0..1)
//   loc 3: vec2 atlas_uv_max — atlas bottom-right UV (normalized 0..1)
//   loc 4: vec4 fg_rgba      — text foreground color (0..1, premultiplied)
//   loc 5: vec4 bg_rgba      — cell background color (0..1, premultiplied);
//                              alpha=0 means "transparent / no bg fill"
//   loc 6: vec2 cell_origin  — top-left of the cell rect (for bg quads)
//   loc 7: vec2 cell_size    — full cell size in pixels (for bg quads)
//
// `is_bg` is encoded as `cell_size.x < 0`: by convention we emit two
// instances per non-default cell — one bg quad (cell_size negated) and one
// glyph quad. The fragment shader sees a flag in `v_is_bg` and either
// outputs solid bg color or samples the alpha atlas with fg color.
//
// Push constants supply viewport extent (so we can do pixel→NDC conversion).

layout(push_constant) uniform PushConstants {
    vec2 viewport_size;  // (width, height) in pixels
} pc;

layout(location = 0) in vec2 in_dst_origin;
layout(location = 1) in vec2 in_dst_size;
layout(location = 2) in vec2 in_atlas_uv_min;
layout(location = 3) in vec2 in_atlas_uv_max;
layout(location = 4) in vec4 in_fg_rgba;
layout(location = 5) in vec4 in_bg_rgba;
layout(location = 6) in vec2 in_cell_origin;
layout(location = 7) in vec2 in_cell_size;

layout(location = 0) out vec2 v_atlas_uv;
layout(location = 1) out vec4 v_fg_rgba;
layout(location = 2) out vec4 v_bg_rgba;
layout(location = 3) flat out int v_is_bg;

void main() {
    vec2 corner;
    if (gl_VertexIndex == 0) corner = vec2(0.0, 0.0);
    else if (gl_VertexIndex == 1) corner = vec2(1.0, 0.0);
    else if (gl_VertexIndex == 2) corner = vec2(0.0, 1.0);
    else                          corner = vec2(1.0, 1.0);

    bool is_bg = in_cell_size.x < 0.0;
    v_is_bg = is_bg ? 1 : 0;

    vec2 origin = is_bg ? in_cell_origin : in_dst_origin;
    vec2 size   = is_bg ? vec2(-in_cell_size.x, in_cell_size.y) : in_dst_size;

    vec2 pixel = origin + corner * size;
    vec2 ndc = (pixel / pc.viewport_size) * 2.0 - 1.0;
    gl_Position = vec4(ndc.x, ndc.y, 0.0, 1.0);

    v_atlas_uv = mix(in_atlas_uv_min, in_atlas_uv_max, corner);

    v_fg_rgba = in_fg_rgba;
    v_bg_rgba = in_bg_rgba;
}
