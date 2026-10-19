extends CompositorEffect
## Test-only readback of the two actual OpenXR render layers.
var pending := ""
var completed := ""
var frames: Array[Image] = []
var mutex := Mutex.new()

func _init() -> void:
	effect_callback_type = EFFECT_CALLBACK_TYPE_POST_TRANSPARENT
	access_resolved_color = true

func request_capture(label: String) -> void:
	mutex.lock()
	pending = label
	mutex.unlock()

func _render_callback(_kind: int, render_data: RenderData) -> void:
	mutex.lock()
	var label := pending
	pending = ""
	mutex.unlock()
	if label.is_empty(): return
	var buffers: RenderSceneBuffersRD = render_data.get_render_scene_buffers()
	var device := RenderingServer.get_rendering_device()
	var texture := buffers.get_color_texture()
	var format := device.texture_get_format(texture)
	var source := RDShaderSource.new()
	source.source_compute = """#version 450
layout(local_size_x=8,local_size_y=8,local_size_z=1) in;
layout(set=0,binding=0) uniform sampler2DArray eyes;
layout(set=0,binding=1,std430) writeonly buffer Output { vec4 rgba[]; } dst;
void main() {
 ivec3 p=ivec3(gl_GlobalInvocationID), n=textureSize(eyes,0);
 if(any(greaterThanEqual(p,n))) return;
 dst.rgba[(p.z*n.y+p.y)*n.x+p.x]=texelFetch(eyes,p,0);
}
"""
	var shader := device.shader_create_from_spirv(device.shader_compile_spirv_from_source(source))
	var pipeline := device.compute_pipeline_create(shader)
	var sampler := device.sampler_create(RDSamplerState.new())
	var stride := format.width*format.height*16
	var storage := device.storage_buffer_create(stride*buffers.get_view_count())
	var input := RDUniform.new()
	input.uniform_type = RenderingDevice.UNIFORM_TYPE_SAMPLER_WITH_TEXTURE
	input.binding = 0
	input.add_id(sampler)
	input.add_id(texture)
	var output := RDUniform.new()
	output.uniform_type = RenderingDevice.UNIFORM_TYPE_STORAGE_BUFFER
	output.binding = 1
	output.add_id(storage)
	var bindings := device.uniform_set_create([input,output],shader,0)
	var list := device.compute_list_begin()
	device.compute_list_bind_compute_pipeline(list,pipeline)
	device.compute_list_bind_uniform_set(list,bindings,0)
	device.compute_list_dispatch(list,ceili(format.width/8.0),ceili(format.height/8.0),buffers.get_view_count())
	device.compute_list_end()
	var raw := device.buffer_get_data(storage)
	var images: Array[Image] = []
	for eye in buffers.get_view_count():
		images.append(Image.create_from_data(format.width,format.height,false,Image.FORMAT_RGBAF,raw.slice(eye*stride,(eye+1)*stride)))
	for resource in [bindings,pipeline,shader,sampler,storage]: device.free_rid(resource)
	mutex.lock()
	frames = images
	completed = label
	mutex.unlock()
