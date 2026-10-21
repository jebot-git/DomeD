#version 450
layout(set=0,binding=0) uniform sampler2D content;
layout(location=0) in vec2 uv;
layout(location=0) out vec4 color;
layout(push_constant) uniform Options {
    vec4 size; // logical width, output height, source height, Y inversion
    vec4 effects; // opacity, inset border, focused, stereo
    vec4 accent; // accent RGB, scroll band height
    vec4 scroll; // scroll offset, maximum offset, unused, unused
} p;
void main() {
    vec2 pos = uv * p.size.xy;
    float source_y = pos.y;
    float band = p.accent.w;
    if (band > 0.0) source_y = pos.y - band + p.scroll.x;
    vec2 sample_uv = vec2(uv.x, source_y / p.size.z);
    if (p.size.w != 0.0) sample_uv.y = 1.0 - sample_uv.y;
    color = texture(content, sample_uv);
    if (band > 0.0 && (pos.y < band || pos.y >= p.size.y-band)) {
        bool upper = pos.y < band;
        bool enabled = upper ? p.scroll.x > 0.0 : p.scroll.x < p.scroll.y;
        float cy = upper ? band/2.0 : p.size.y-band/2.0;
        float dy = (pos.y-cy) * (upper ? -1.0 : 1.0);
        color = vec4(vec3(31,37,45)/255.0,1.0);
        if (dy >= -4.0 && dy <= 4.0 && abs(pos.x-p.size.x/2.0) <= 4.0-dy)
            color = vec4(vec3(enabled ? 235.0 : 100.0)/255.0, 1.0);
    }
    float edge = min(min(pos.x, p.size.x-pos.x), min(pos.y,p.size.y-pos.y));
    if (p.effects.w == 0.0 && edge < p.effects.y)
        color = vec4(vec3(146,154,165)/255.0,1.0);
    if (p.effects.z != 0.0) {
        float eye_width = p.size.x / (p.effects.w != 0.0 ? 2.0 : 1.0);
        float x = mod(pos.x, eye_width);
        float eye_edge = min(min(x,eye_width-x), min(pos.y,p.size.y-pos.y));
        float alpha = eye_edge < 1.0 ? 0.60 : (eye_edge < 2.0 ? 0.16 : 0.0);
        color.rgb = mix(color.rgb, p.accent.rgb * color.a, alpha);
    }
    color *= p.effects.x;
    // Keep the byte-domain effects consistent with the CPU fallback. An sRGB
    // attachment encodes on write, so decode explicitly to avoid encoding twice.
    if (p.scroll.z != 0.0)
        color.rgb = mix(color.rgb / 12.92, pow((color.rgb + 0.055) / 1.055, vec3(2.4)), greaterThan(color.rgb, vec3(0.04045)));
}
