#version 450

// 1. The "Global" data (Camera) stays in the UBO
layout(binding = 0) uniform UniformBufferObject {
    mat4 view;
    mat4 proj;
} ubo;

// 2. The "Per-Object" data (Model Matrix) moves here
// This block matches MeshPushConstants struct
layout(push_constant) uniform constants{
    mat4 model;
} PushVars; // uniform as its same for each vertex in this draw call

layout(location = 0) in vec3 inPosition; // One vertex's position
layout(location = 1) in vec3 inColor;    // One vertex's color
layout(location = 2) in vec2 inUV;       // One vertex's texture coordinates

layout(location = 0) out vec3 fragColor;
layout(location = 1) out vec2 fragUV;

void main() {
    gl_Position = ubo.proj * ubo.view * PushVars.model * vec4(inPosition, 1.0);
    fragColor = inColor;
    fragUV = inUV;
}