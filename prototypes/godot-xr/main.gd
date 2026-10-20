extends Node3D
## Experimental native OpenXR viewer for the shared Rust scene; surfaces are samples.
var origin: XROrigin3D
var head: XRCamera3D
var frame_room := Node3D.new()
var frame_view := Node3D.new()
var trays := Node3D.new()
var request := HTTPRequest.new()
var xr: XRInterface
var snapshot: Dictionary = {}
var revision := -1
var refresh := 0.0
var base_url := OS.get_environment("DOME_URL")
var out := OS.get_environment("DOME_XR_OUTPUT")
var test_mode := "--test" in OS.get_cmdline_user_args()
var focused := false
var checks: Array[Dictionary] = []
var capture_effect: CompositorEffect
var finished := false

func _ready() -> void:
	if base_url.is_empty(): base_url = "http://127.0.0.1:49640"
	if out.is_empty(): out = ProjectSettings.globalize_path("res://../../artifacts/simulated-xr")
	DirAccess.make_dir_recursive_absolute(out)
	xr = XRServer.find_interface("OpenXR")
	if not xr or not xr.is_initialized():
		print("DOME_XR_ERROR: OpenXR did not initialize")
		get_tree().quit(2)
		return
	get_viewport().use_xr = true
	xr.set("render_target_size_multiplier", 0.65)
	if xr.has_signal("session_focussed"): xr.connect("session_focussed", func(): focused = true)
	if xr.has_signal("session_visible"): xr.connect("session_visible", func(): focused = false)
	if xr.has_signal("session_stopping"): xr.connect("session_stopping", func(): focused = false)
	origin = XROrigin3D.new()
	add_child(origin)
	head = XRCamera3D.new()
	head.current = true
	origin.add_child(head)
	add_child(frame_room)
	head.add_child(frame_view)
	add_child(trays)
	var environment := WorldEnvironment.new()
	var settings := Environment.new()
	settings.background_mode = Environment.BG_COLOR
	settings.background_color = Color("0b1822")
	settings.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	settings.ambient_light_color = Color.WHITE
	settings.ambient_light_energy = 0.8
	environment.environment = settings
	add_child(environment)
	add_child(request)
	request.timeout = 4.0
	request.request_completed.connect(received)
	request.request(base_url + "/api/xr-scene")
	print("DOME_XR_READY: ", xr.get_system_info())
	if test_mode: run_checks.call_deferred()

func _process(delta: float) -> void:
	refresh += delta
	if refresh >= 0.3 and request.get_http_client_status() == HTTPClient.STATUS_DISCONNECTED:
		refresh = 0.0
		request.request(base_url + "/api/xr-scene")

func received(result: int, status: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	if result != HTTPRequest.RESULT_SUCCESS or status != 200: return
	var data = JSON.parse_string(body.get_string_from_utf8())
	if not data is Dictionary: return
	if int(data.get("revision", -1)) == revision: return
	snapshot = data
	revision = int(data.revision)
	rebuild()

func clear(node: Node) -> void:
	for child in node.get_children(): node.remove_child(child); child.queue_free()

func material(color: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	m.cull_mode = BaseMaterial3D.CULL_DISABLED
	m.albedo_color = color
	if color.a < 0.99:
		m.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
		m.no_depth_test = false
	return m

func rebuild() -> void:
	clear(frame_room); clear(frame_view); clear(trays)
	var c: Dictionary = snapshot.config
	frame_room.position = Vector3(c.center_m[0], c.center_m[1], c.center_m[2])
	frame_room.rotation.y = -deg_to_rad(float(c.yaw_deg))
	var target: Node3D = frame_view if c.projection == "in_view" else frame_room
	for panel in snapshot.panels:
		var st := SurfaceTool.new()
		st.begin(Mesh.PRIMITIVE_TRIANGLES)
		var verts: Array = panel.vertices
		for i in range(0, verts.size() - 2, 2):
			for index in [i, i+1, i+2, i+1, i+3, i+2]:
				var v: Array = verts[index]
				st.add_vertex(Vector3(v[0], v[1], v[2]))
		var mesh := MeshInstance3D.new()
		mesh.mesh = st.commit()
		var palette := [Color("1d3d4b"), Color("29443d"), Color("3b3449")]
		var color: Color = palette[int(panel.id) % 3]
		color.a = float(panel.opacity)
		mesh.material_override = material(color)
		target.add_child(mesh)
		if int(panel.workspace) == int(snapshot.active_workspace):
			var label := Label3D.new()
			label.text = str(panel.title) + "\n\n" + str(panel.app_id) + "\nSAMPLE SURFACE"
			label.font_size = 30
			label.pixel_size = 0.0018
			label.modulate = Color("d8e9e8")
			label.no_depth_test = false
			var mid := int(verts.size()/4)*2
			var a: Array = verts[mid]
			var b: Array = verts[mid+1]
			label.position = (Vector3(a[0],a[1],a[2])+Vector3(b[0],b[1],b[2]))/2.0
			label.position *= 0.95
			label.billboard = BaseMaterial3D.BILLBOARD_ENABLED
			target.add_child(label)
	make_tray(true, c)
	make_tray(false, c)
	print("DOME_XR_SCENE: revision=", revision, " projection=", c.projection, " panels=", snapshot.panels.size())

func make_tray(upper: bool, _c: Dictionary) -> void:
	var bars: Array = snapshot.get("shell_bars", [])
	if bars.size() < 2: return
	var d: Dictionary = bars[0 if upper else 1].dome
	var q: Array = d.frame.orientation_xyzw
	var rotation := Quaternion(float(q[0]), float(q[1]), float(q[2]), float(q[3]))
	var pos: Array = d.frame.position_m
	var center := Vector3(float(pos[0]), float(pos[1]), float(pos[2]))
	var radius: float = d.radius
	var vertices: Array[Vector3] = []
	for i in range(33):
		var yaw: float = (float(i)/32.0-0.5)*float(d.horizontal)
		for y in [float(d.top), float(d.bottom)]:
			vertices.append(center + rotation*Vector3(radius*sin(yaw), radius*y, -radius*cos(yaw)))
	var st := SurfaceTool.new()
	st.begin(Mesh.PRIMITIVE_TRIANGLES)
	for i in range(0, vertices.size()-2, 2):
		for index in [i, i+1, i+2, i+1, i+3, i+2]: st.add_vertex(vertices[index])
	var plate := MeshInstance3D.new()
	plate.mesh = st.commit()
	plate.material_override = material(Color(0.25,0.34,0.38,0.82))
	trays.add_child(plate)
	var label := Label3D.new()
	label.text = "DOME    |    Window                             Tray pending" if upper else "Launch                         Tasks                         Desktops"
	label.position = center + rotation*Vector3(0, radius*(float(d.top)+float(d.bottom))/2.0, -radius+0.005)
	label.font_size = 24
	label.pixel_size = 0.0012
	label.no_depth_test = false
	trays.add_child(label)

func check(ok: bool, message: String) -> void:
	checks.append({"passed":ok,"check":message})
	print("PASS " if ok else "FAIL ", message)

func settle(frames: int) -> void:
	for _i in frames: await get_tree().process_frame

func change_projection(mode: String) -> bool:
	var config: Dictionary = snapshot.config.duplicate(true)
	config.projection = mode
	var command := HTTPRequest.new()
	add_child(command)
	var error := command.request(base_url+"/api/action", ["Content-Type: application/json"], HTTPClient.METHOD_POST, JSON.stringify({"op":"configure","config":config}))
	if error != OK: command.queue_free(); return false
	var response: Array = await command.request_completed
	command.queue_free()
	return int(response[1]) == 200

func capture(label: String) -> void:
	capture_effect.request_capture(label)
	for _i in 240:
		await get_tree().process_frame
		if capture_effect.completed == label: break
	check(capture_effect.completed == label and capture_effect.frames.size() == 2, "Two rendered eye buffers: " + label)
	if capture_effect.frames.size() == 2:
		check(capture_effect.frames[0].get_data() != capture_effect.frames[1].get_data(), "Stereo pixels differ: " + label)
		var colors := {}
		var sample: Image = capture_effect.frames[0]
		for y in range(0, sample.get_height(), maxi(1,sample.get_height()/12)):
			for x in range(0, sample.get_width(), maxi(1,sample.get_width()/12)):
				colors[sample.get_pixel(x,y).to_html()] = true
		check(colors.size() > 3, "Rendered scene is not blank: " + label)
	for eye in capture_effect.frames.size():
		var image: Image = capture_effect.frames[eye]
		image.convert(Image.FORMAT_RGBA8)
		image.linear_to_srgb()
		image.save_png(out.path_join("%s-eye%d.png" % [label, eye]))

func run_checks() -> void:
	for _i in 600:
		await get_tree().process_frame
		if not snapshot.is_empty() and focused: break
	check(xr.is_initialized(), "Native OpenXR initialized")
	check(get_viewport().use_xr and xr.get_view_count() == 2, "Native stereo presentation")
	check(focused, "Simulated runtime session focused")
	check(not snapshot.is_empty(), "Shared Rust scene received")
	if not snapshot.is_empty():
		var eyes: float = xr.get_transform_for_view(0,Transform3D.IDENTITY).origin.distance_to(xr.get_transform_for_view(1,Transform3D.IDENTITY).origin)
		check(eyes > 0.01, "Distinct runtime eye poses")
		capture_effect = load("res://stereo_capture.gd").new()
		var compositor := Compositor.new()
		compositor.compositor_effects = [capture_effect]
		head.compositor = compositor
		var initial: String = snapshot.config.projection
		for mode in ["room_dome", "in_view"]:
			check(await change_projection(mode), "Projection switch accepted: " + mode)
			for _i in 180:
				await get_tree().process_frame
				if str(snapshot.config.projection) == mode: break
			await settle(20)
			check(str(snapshot.config.projection) == mode and frame_room.get_child_count()+frame_view.get_child_count()>0, "Projection rendered: " + mode)
			await capture(mode)
		await change_projection(initial)
		await settle(20)
	finish()

func finish() -> void:
	if finished: return
	finished = true
	var failed := checks.filter(func(c): return not c.passed).size()
	var report := {"checks":checks,"failed":failed,"runtime_manifest":OS.get_environment("XR_RUNTIME_JSON"),"runtime":xr.get_system_info(),"simulated":OS.get_environment("SIMULATED_ENABLE"),"scene_source":snapshot.get("source","unavailable"),"limitations":["Sample surface content; client texture transport not implemented", "No WiVRn stream unless a WiVRn client is connected", "No physical headset or controller interaction validated"]}
	FileAccess.open(out.path_join("report.json"),FileAccess.WRITE).store_string(JSON.stringify(report,"  "))
	print("DOME_XR_RESULT: checks=",checks.size()," failed=",failed)
	head.compositor = null
	get_tree().quit(0 if failed == 0 else 1)
