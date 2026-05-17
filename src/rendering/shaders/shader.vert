#version 450

// Camera matrices — shared across all draw calls this frame.
layout(set = 0, binding = 0) uniform UniformBufferObject {
    mat4 view;
    mat4 proj;
} ubo;

// Per-object data pushed directly into the command stream, no descriptor needed.
// Mirrors MeshPushConstants struct
// texture_index and sampler_index are unused here but must match the fragment stage declaration.
layout(push_constant) uniform constants {
    mat4 model;
    uint texture_index; //unused
    uint sampler_index; //unused
} PushVars;

layout(location = 0) in vec3 inPosition; // one vertex's position
layout(location = 1) in vec3 inColor;    // one vertex's color
layout(location = 2) in vec2 inUV;       // one vertex's UV coordinates

layout(location = 0) out vec3 fragColor;
layout(location = 1) out vec2 fragUV;

void main() {
    gl_Position = ubo.proj * ubo.view * PushVars.model * vec4(inPosition, 1.0);
    fragColor = inColor;
    fragUV = inUV;
}
