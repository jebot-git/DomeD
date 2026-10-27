#version 450
layout(location=0) in vec3 spawn;
layout(location=1) in vec3 velocity;
layout(location=2) in vec3 acceleration;
layout(location=3) in vec4 life_phase_size;
layout(location=0) out vec2 uv;
layout(location=1) out vec3 lit;
layout(location=2) out vec3 emission;
layout(location=3) out float alpha;
layout(location=4) out vec3 local_position;
layout(push_constant) uniform Frame {mat4 mvp;vec4 eye_fog;vec4 fog_color;vec4 options;vec4 tint;} p;
void main() {
    const vec2 corners[6]=vec2[](vec2(-.5,-.5),vec2(.5,-.5),vec2(.5,.5),vec2(-.5,-.5),vec2(.5,.5),vec2(-.5,.5));
    float life=max(life_phase_size.x,.01);
    float age=mod(p.options.w+life_phase_size.y,life);
    float phase=age/life;
    vec3 centre=spawn+velocity*age+.5*acceleration*age*age;
    vec3 facing=normalize(p.eye_fog.xyz-centre+vec3(.0001));
    vec3 right=normalize(cross(vec3(0,1,0),facing)+vec3(.0001,0,0));
    vec3 up=normalize(cross(facing,right));
    vec2 corner=corners[gl_VertexIndex];
    float grow=p.fog_color.w>0.5 ? 1.0+phase*2.0 : 1.0;
    local_position=centre+(right*corner.x*life_phase_size.z+up*corner.y*life_phase_size.w)*grow;
    gl_Position=p.mvp*vec4(local_position,1);
    uv=corner+vec2(.5);lit=vec3(0);emission=p.tint.rgb;
    alpha=p.tint.a*(p.fog_color.w < -.5 ? 1.0 : (p.fog_color.w>0.5 ? sin(phase*3.14159265) : smoothstep(0.,.03,phase)*(1.0-smoothstep(.92,1.,phase))));
}
