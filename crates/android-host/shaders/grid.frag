#version 450

// M2-S08 fragment shader for static glyph grid rendering.
//
// Samples the alpha-only glyph atlas (R8_UNORM, single channel = coverage)
// and outputs white text alpha-blended over the cleared magenta background.
// The blend mode is configured pipeline-side as standard alpha blending:
//   src.rgba = (1, 1, 1, sample.r)
//   dst = src * src.a + dst * (1 - src.a)

layout(set = 0, binding = 0) uniform sampler2D atlas_sampler;

layout(location = 0) in vec2 v_atlas_uv;

layout(location = 0) out vec4 out_color;

void main() {
    float alpha = texture(atlas_sampler, v_atlas_uv).r;
    if (alpha < 0.01) discard;
    out_color = vec4(1.0, 1.0, 1.0, alpha);
}
