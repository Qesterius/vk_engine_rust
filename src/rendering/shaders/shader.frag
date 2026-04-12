#version 450

layout(set = 1, binding = 0) uniform sampler2D myTexture;
layout(location = 0) in vec3 fragColor;
layout(location = 1) in vec2 fragUV;
layout(location = 0) out vec4 outColor;

void main(){
    outColor = texture(myTexture, fragUV);
}
