#version 450

// M3-S08 fragment shader for the per-cell dynamic grid renderer
// (android-host runtime mirror; canonical lives in
// `warp-src/crates/warpui/shaders/android/dynamic_grid.frag`).
//
// Two instance flavours are dispatched through this shader, distinguished by
// `v_is_bg` set in the vertex shader:
//
//   * Background quads (`v_is_bg == 1`): emit solid `v_bg_rgba`. We only
//     emit these for cells whose bg is not the default black (0x000000FF) so
//     the cleared frame already covers the common case (one instance per
//     cell instead of two).
//   * Glyph quads (`v_is_bg == 0`): sample the alpha atlas, modulate
//     `v_fg_rgba` by the sampled coverage, and output the result.
//
// Standard alpha blending is configured pipeline-side, same as static_grid:
//   src = (rgb, a)
//   dst = src * src.a + dst * (1 - src.a)

layout(set = 0, binding = 0) uniform sampler2D atlas_sampler;

layout(location = 0) in vec2 v_atlas_uv;
layout(location = 1) in vec4 v_fg_rgba;
layout(location = 2) in vec4 v_bg_rgba;
layout(location = 3) flat in int v_is_bg;

layout(location = 0) out vec4 out_color;

void main() {
    if (v_is_bg == 1) {
        if (v_bg_rgba.a < 0.001) discard;
        out_color = v_bg_rgba;
        return;
    }

    float coverage = texture(atlas_sampler, v_atlas_uv).r;
    if (coverage < 0.01) discard;
    float a = v_fg_rgba.a * coverage;
    out_color = vec4(v_fg_rgba.rgb * a, a);
}
