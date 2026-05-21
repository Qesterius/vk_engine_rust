#version 450

// Bindless texture and sampler arrays — bound once per frame, set 1.
// Array sizes are compile-time constants. 1024 must be less than Device::max_texture_slots cap in device.rs.
// texture2D + sampler are kept separate so any image can be sampled with any sampler at draw time.
layout(set = 1, binding = 0) uniform texture2D textures[1024];
layout(set = 1, binding = 1) uniform sampler samplers[3];

// Per-object data read directly from the command stream.
// Mirrors MeshPushConstants struct in src/rendering/vertex.rs
// model is unused here but must match the vertex stage declaration.
layout(push_constant) uniform constants {
    mat4 model; // unused
    uint texture_index;
    uint sampler_index;
} PushVars;

layout(location = 0) in vec3 fragColor; // interpolated vertex color (unused when texturing)
layout(location = 1) in vec2 fragUV;    // interpolated UV from vertex shader
layout(location = 0) out vec4 outColor; // output color of the fragment

void main() {
    outColor = texture(
        sampler2D(textures[PushVars.texture_index], samplers[PushVars.sampler_index]),
        fragUV
    );
}
