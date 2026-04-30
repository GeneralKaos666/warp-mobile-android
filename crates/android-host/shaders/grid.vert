#version 450

// M2-S08 vertex shader for static glyph grid rendering.
//
// Per-vertex (4 vertices = quad corners, repeated as triangle list with index
// buffer or as triangle strip via vertex_index lookup). We use vertex_index
// directly so the input vertex buffer is empty — saves one bind.
//
// Per-instance (one instance per glyph in the grid):
//   loc 0: vec2 dst_origin   — top-left destination pixel
//   loc 1: vec2 dst_size     — destination quad size in pixels
//   loc 2: vec2 atlas_uv_min — atlas top-left UV (normalized 0..1)
//   loc 3: vec2 atlas_uv_max — atlas bottom-right UV (normalized 0..1)
//
// Push constants supply viewport extent (so we can do pixel→NDC conversion).

layout(push_constant) uniform PushConstants {
    vec2 viewport_size;  // (width, height) in pixels
} pc;

layout(location = 0) in vec2 in_dst_origin;
layout(location = 1) in vec2 in_dst_size;
layout(location = 2) in vec2 in_atlas_uv_min;
layout(location = 3) in vec2 in_atlas_uv_max;

layout(location = 0) out vec2 v_atlas_uv;

void main() {
    // 4 corners of the quad as triangle strip:
    //   0 = (0,0)
    //   1 = (1,0)
    //   2 = (0,1)
    //   3 = (1,1)
    vec2 corner;
    if (gl_VertexIndex == 0) corner = vec2(0.0, 0.0);
    else if (gl_VertexIndex == 1) corner = vec2(1.0, 0.0);
    else if (gl_VertexIndex == 2) corner = vec2(0.0, 1.0);
    else                          corner = vec2(1.0, 1.0);

    // Pixel space → clip space ([0,W] -> [-1,1], [0,H] -> [-1,1] flipped Y).
    vec2 pixel = in_dst_origin + corner * in_dst_size;
    vec2 ndc = (pixel / pc.viewport_size) * 2.0 - 1.0;
    gl_Position = vec4(ndc.x, ndc.y, 0.0, 1.0);

    // Linearly interpolate UV between atlas_uv_min and atlas_uv_max.
    v_atlas_uv = mix(in_atlas_uv_min, in_atlas_uv_max, corner);
}
