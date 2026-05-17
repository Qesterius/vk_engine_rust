#version 450

// The texture of the current draw: each texture has its own descriptor set, bound per draw at set 1.
layout(set = 1, binding = 0) uniform sampler2D albedo;

layout(location = 0) in vec3 fragColor; // interpolated vertex color (unused when texturing)
layout(location = 1) in vec2 fragUV;    // interpolated UV from vertex shader
layout(location = 0) out vec4 outColor; // output color of the fragment

void main() {
    outColor = texture(albedo, fragUV);
}
