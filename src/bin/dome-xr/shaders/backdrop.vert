#version 450
layout(location=0) in vec3 position;
layout(location=1) in vec2 uv_in;
layout(location=2) in vec3 lit_in;
layout(location=3) in vec3 emission_in;
layout(location=4) in float alpha_in;
layout(location=0) out vec2 uv;
layout(location=1) out vec3 lit;
layout(location=2) out vec3 emission;
layout(location=3) out float alpha;
layout(location=4) out vec3 local_position;
layout(push_constant) uniform Frame {
    mat4 mvp;
    vec4 eye_fog;
    vec4 fog_color;
    vec4 options; // cutoff, blend mode, sRGB target, unused
} p;
void main() {
    gl_Position = p.mvp * vec4(position, 1.0);
    uv = vec2(uv_in.x+p.fog_color.w, 1.0-uv_in.y-p.options.w);
    lit = lit_in;
    emission = emission_in;
    alpha = alpha_in;
    local_position = position;
}
