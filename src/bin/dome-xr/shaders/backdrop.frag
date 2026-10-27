#version 450
layout(set=0,binding=0) uniform sampler2D artwork;
layout(location=0) in vec2 uv;
layout(location=1) in vec3 lit;
layout(location=2) in vec3 emission;
layout(location=3) in float alpha;
layout(location=4) in vec3 local_position;
layout(location=0) out vec4 color;
layout(push_constant) uniform Frame {
    mat4 mvp;
    vec4 eye_fog;
    vec4 fog_color;
    vec4 options;
} p;
void main() {
    vec4 texel = texture(artwork, uv);
    float opacity = texel.a*alpha;
    if (opacity < p.options.x) discard;
    vec3 value = texel.rgb*(lit+emission);
    float fog = exp(-length(local_position-p.eye_fog.xyz)*p.eye_fog.w);
    value = mix(p.options.y == 2.0 ? vec3(0.0) : p.fog_color.rgb, value, fog);
    // OpenXR swapchain formats may be sRGB or UNORM. Keep their appearance equal.
    if (p.options.z == 0.0) value = pow(max(value,vec3(0.0)),vec3(1.0/2.2));
    color = vec4(value, p.options.y == 0.0 ? 1.0 : opacity);
}
