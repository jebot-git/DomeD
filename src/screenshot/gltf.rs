//! Self-contained glTF 2.0 snapshot of DomeD's submitted spatial surfaces.
//! Geometry comes from the compositor, not inferred from the screenshot pixels.
use super::{Eye, Layer, Pixels, MAX_CAPTURE_BYTES, MAX_PIXELS};
use crate::Pose;
use glam::{Quat, Vec3};
use serde_json::{json, Value};

const JSON_CHUNK: u32 = 0x4e4f534a;
const BIN_CHUNK: u32 = 0x004e4942;
const MAX_LAYERS: usize = 256;

/// Check the bounded GLB container before accepting a renderer's result.
/// Full schema/geometry validation is performed by the export regression tests.
pub fn validate_glb(bytes: &[u8]) -> Result<(), String> {
    let bad = || "Invalid XR glTF capture".to_string();
    if bytes.len() < 28 || bytes.len() > MAX_CAPTURE_BYTES || bytes.len() % 4 != 0 {
        return Err(bad());
    }
    let word = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    if &bytes[..4] != b"glTF" || word(4) != 2 || word(8) != bytes.len()
        || word(16) != JSON_CHUNK as usize { return Err(bad()); }
    let end = 20usize.checked_add(word(12)).filter(|n| *n <= bytes.len()).ok_or_else(bad)?;
    if word(12) % 4 != 0 { return Err(bad()); }
    let document: Value = serde_json::from_slice(&bytes[20..end]).map_err(|_| bad())?;
    if document["asset"]["version"] != "2.0" || end + 8 > bytes.len()
        || word(end + 4) != BIN_CHUNK as usize || word(end) % 4 != 0
        || end + 8 + word(end) != bytes.len() { return Err(bad()); }
    let length = document["buffers"][0]["byteLength"].as_u64().ok_or_else(bad)? as usize;
    if document["buffers"].as_array().map(Vec::len) != Some(1)
        || document["buffers"][0].get("uri").is_some()
        || length > word(end) || word(end) - length > 3 { return Err(bad()); }
    Ok(())
}

struct Builder {
    document: Value,
    binary: Vec<u8>,
}
impl Builder {
    fn push(&mut self, key: &str, value: Value) -> usize {
        let values = self.document[key].as_array_mut().unwrap();
        let index = values.len(); values.push(value); index
    }
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> Result<usize, String> {
        while self.binary.len() % 4 != 0 { self.binary.push(0); }
        if self.binary.len().checked_add(bytes.len()).is_none_or(|n| n > MAX_CAPTURE_BYTES) {
            return Err("XR glTF capture exceeds 128 MiB".into());
        }
        let offset = self.binary.len(); self.binary.extend_from_slice(bytes);
        let mut view = json!({"buffer":0,"byteOffset":offset,"byteLength":bytes.len()});
        if let Some(target) = target { view["target"] = json!(target); }
        Ok(self.push("bufferViews", view))
    }
    fn floats(&mut self, values: &[f32], width: usize, bounds: bool) -> Result<usize, String> {
        let bytes: Vec<_> = values.iter().flat_map(|x| x.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34962))?;
        let mut accessor = json!({"bufferView":view,"componentType":5126,
            "count":values.len()/width,"type":if width==3 {"VEC3"} else {"VEC2"}});
        if bounds {
            let mut min = vec![f32::INFINITY; width]; let mut max = vec![f32::NEG_INFINITY; width];
            for v in values.chunks_exact(width) {
                for i in 0..width { min[i] = min[i].min(v[i]); max[i] = max[i].max(v[i]); }
            }
            accessor["min"] = json!(min); accessor["max"] = json!(max);
        }
        Ok(self.push("accessors", accessor))
    }
    fn texture(&mut self, layer: &Layer, eye: usize) -> Result<usize, String> {
        let p = &layer.pixels;
        let stereo = layer.stereo && p.width >= 2;
        let width = if stereo { p.width / 2 } else { p.width };
        let start = if stereo && eye == 1 { p.width - width } else { 0 };
        let mut rgba = Vec::with_capacity(width as usize * p.height as usize * 4);
        for y in 0..p.height {
            let row = ((y * p.width + start) * 4) as usize;
            for pixel in p.rgba[row..row + width as usize * 4].chunks_exact(4) {
                // Wayland/OpenXR uses premultiplied alpha; glTF base color is straight.
                let a = pixel[3] as u32;
                for c in &pixel[..3] { rgba.push(if a == 0 { 0 } else { ((*c as u32 * 255 + a/2)/a).min(255) as u8 }); }
                rgba.push(pixel[3]);
            }
        }
        let png = Pixels { width, height: p.height, rgba }.png()?;
        let view = self.view(&png, None)?;
        let image = self.push("images", json!({"bufferView":view,"mimeType":"image/png"}));
        let texture = self.push("textures", json!({"source":image,"sampler":0}));
        Ok(self.push("materials", json!({
            "pbrMetallicRoughness":{"baseColorTexture":{"index":texture},
                "baseColorFactor":[1,1,1,layer.opacity],"metallicFactor":0,"roughnessFactor":1},
            "alphaMode":"BLEND","doubleSided":true,"extensions":{"KHR_materials_unlit":{}}
        })))
    }
    fn finish(mut self) -> Result<Vec<u8>, String> {
        self.document["buffers"] = json!([{"byteLength":self.binary.len()}]);
        let mut json = serde_json::to_vec(&self.document).map_err(|e| e.to_string())?;
        while json.len() % 4 != 0 { json.push(b' '); }
        while self.binary.len() % 4 != 0 { self.binary.push(0); }
        let length = 28 + json.len() + self.binary.len();
        if length > MAX_CAPTURE_BYTES { return Err("XR glTF capture exceeds 128 MiB".into()); }
        let mut result = Vec::with_capacity(length);
        result.extend_from_slice(b"glTF");
        for word in [2, length as u32, json.len() as u32, JSON_CHUNK] { result.extend_from_slice(&word.to_le_bytes()); }
        result.extend_from_slice(&json);
        for word in [self.binary.len() as u32, BIN_CHUNK] { result.extend_from_slice(&word.to_le_bytes()); }
        result.extend_from_slice(&self.binary);
        Ok(result)
    }
}

fn valid_eye(eye: Eye) -> Result<(), String> {
    eye.pose.validate()?;
    let [l,r,d,u] = eye.fov;
    if eye.fov.iter().any(|v| !v.is_finite() || v.abs() >= std::f32::consts::FRAC_PI_2)
        || l >= r || d >= u { return Err("Invalid XR capture eye FOV".into()); }
    Ok(())
}

/// Export metres, Y-up and -Z-forward, relative to the captured upright head pose.
/// Stereo media gets separate standard glTF scenes: no viewer-specific eye shader
/// is required, and neither eye's pixels are silently discarded.
pub fn glb(layers: &[Layer], eyes: [Eye; 2], head: Pose) -> Result<Vec<u8>, String> {
    head.validate()?;
    for eye in eyes { valid_eye(eye)?; }
    if layers.len() > MAX_LAYERS { return Err("Too many XR capture surfaces".into()); }
    let mut pixels = 0usize;
    for layer in layers {
        if !layer.pixels.valid() || !layer.opacity.is_finite() || !(0. ..=1.).contains(&layer.opacity) {
            return Err("Invalid XR capture pixels or opacity".into());
        }
        pixels += layer.pixels.width as usize * layer.pixels.height as usize;
        if pixels > MAX_PIXELS { return Err("XR capture textures exceed 32 megapixels".into()); }
        let q = &layer.quad; q.pose.validate()?;
        if q.size.iter().any(|n| !n.is_finite() || *n <= 0.) { return Err("Invalid XR capture surface size".into()); }
        if let Some(d) = &q.dome {
            d.frame.validate()?;
            if ![d.radius,d.horizontal,d.top,d.bottom].iter().all(|n| n.is_finite())
                || d.radius <= 0. || d.horizontal <= 0. || d.horizontal > std::f32::consts::TAU
                || d.top <= d.bottom { return Err("Invalid XR capture cylinder".into()); }
        }
    }
    if layers.is_empty() || layers.iter().all(|l| l.opacity == 0.) { return Err("No DomeD surfaces to capture".into()); }
    let origin = head.upright();
    let inverse = Quat::from_array(origin.orientation_xyzw).conjugate();
    let position = Vec3::from_array(origin.position_m);
    let stereo = layers.iter().any(|l| l.stereo && l.pixels.width >= 2);
    let mut b = Builder { binary: vec![], document: json!({
        "asset":{"version":"2.0","generator":"DomeD XR screenshot"},
        "scene":0,"scenes":[],"nodes":[],"meshes":[],"accessors":[],"bufferViews":[],
        "buffers":[],"images":[],"textures":[],"materials":[],"cameras":[],
        "samplers":[{"magFilter":9729,"minFilter":9729,"wrapS":33071,"wrapT":33071}],
        "extensionsUsed":["KHR_materials_unlit"],
        "extras":{"DomeD":{"version":1,"units":"metres","stageOrigin":origin,
            "eyes":eyes.map(|e|json!({"pose":e.pose,"fov":e.fov})),
            "stereoTextures":"Separate left/right scenes; media depth is not reconstructed",
            "composition":"Spatial surfaces; glTF depth/alpha sorting may differ from OpenXR layer order",
            "excluded":"Other OpenXR applications, runtime passthrough and immersive WebXR"}}
    }) };
    let mut scenes: [Vec<usize>; 2] = Default::default();
    for (order, layer) in layers.iter().enumerate() {
        if layer.opacity == 0. { continue; }
        let q = &layer.quad;
        // At most one degree per chord, bounded even for a full cylinder.
        let segments = q.dome.as_ref().map_or(1, |d| (d.horizontal.to_degrees().ceil() as usize).clamp(1,360));
        let mut positions = vec![]; let mut normals = vec![]; let mut uv = vec![];
        for y in 0..=1 {
            for x in 0..=segments {
                let u = x as f32 / segments as f32; let v = y as f32;
                let p = inverse * (q.point([u,v]) - position);
                let n = if let Some(d) = &q.dome {
                    let angle = (u-0.5)*d.horizontal;
                    Quat::from_array(d.frame.orientation_xyzw) * Vec3::new(-angle.sin(),0.,angle.cos())
                } else { Quat::from_array(q.pose.orientation_xyzw) * Vec3::Z };
                if !p.is_finite() { return Err("XR capture geometry is out of range".into()); }
                positions.extend_from_slice(&p.to_array());
                normals.extend_from_slice(&(inverse*n).normalize().to_array());
                uv.extend_from_slice(&[u,v]);
            }
        }
        let pos = b.floats(&positions,3,true)?; let normal = b.floats(&normals,3,false)?; let tex = b.floats(&uv,2,false)?;
        let mut indices: Vec<u32> = vec![];
        for x in 0..segments as u32 { let row = segments as u32+1; indices.extend_from_slice(&[x,x+row,x+1,x+1,x+row,x+row+1]); }
        let index_bytes: Vec<_> = indices.iter().flat_map(|n|n.to_le_bytes()).collect();
        let view = b.view(&index_bytes,Some(34963))?;
        let index = b.push("accessors",json!({"bufferView":view,"componentType":5125,"count":indices.len(),"type":"SCALAR"}));
        let eye_count = if layer.stereo && layer.pixels.width >= 2 {2} else {1};
        for eye in 0..eye_count {
            let material = b.texture(layer,eye)?;
            let mesh = b.push("meshes",json!({"primitives":[{"attributes":{"POSITION":pos,"NORMAL":normal,"TEXCOORD_0":tex},"indices":index,"material":material}]}));
            let node = b.push("nodes",json!({"name":format!("Surface {}{}",q.id,if eye_count==2 {if eye==0 {" left"}else{" right"}}else{""}),
                "mesh":mesh,"extras":{"DomeD":{"surfaceId":q.id.to_string(),"layerOrder":order,
                    "geometry":if q.dome.is_some() {"cylinder"}else{"quad"},"eye":if eye_count==1 {"both"}else if eye==0 {"left"}else{"right"}}}}));
            if eye_count==1 { scenes[0].push(node);scenes[1].push(node); } else { scenes[eye].push(node); }
        }
    }
    for (i,eye) in eyes.into_iter().enumerate() {
        let [l,r,d,u]=eye.fov.map(f32::tan);
        // glTF perspective is symmetric; keep the exact asymmetric FOV in extras.
        let yfov=eye.fov[3]-eye.fov[2];
        let camera=b.push("cameras",json!({"name":if i==0 {"Left eye"}else{"Right eye"},"type":"perspective",
            "perspective":{"yfov":yfov,"aspectRatio":(r-l)/(u-d),"znear":0.01},"extras":{"DomeD":{"fov":eye.fov,"projection":"symmetric preview; exact runtime FOV in extras"}}}));
        let node=b.push("nodes",json!({"name":if i==0 {"Left capture camera"}else{"Right capture camera"},"camera":camera,
            "translation":(inverse*(Vec3::from_array(eye.pose.position_m)-position)).to_array(),
            "rotation":(inverse*Quat::from_array(eye.pose.orientation_xyzw)).to_array()}));
        scenes[0].push(node);scenes[1].push(node);
    }
    b.push("scenes",json!({"name":if stereo {"DomeD capture (left eye textures)"}else{"DomeD XR capture"},"nodes":scenes[0]}));
    if stereo { b.push("scenes",json!({"name":"DomeD capture (right eye textures)","nodes":scenes[1]})); }
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Layer, [Eye; 2], Pose) {
        let head = Pose { position_m:[3.,1.6,4.], orientation_xyzw:Quat::from_rotation_y(0.4).to_array() };
        let quad = crate::xr_input::Quad { id:42, pose:Pose { position_m:head.transform([0.,0.,-2.]), ..head },size:[2.,1.],dome:None };
        let layer=Layer { quad, pixels:Pixels { width:2,height:1,rgba:vec![128,0,0,128,0,0,255,255] },stereo:true,opacity:0.5 };
        let eyes=[-0.032,0.032].map(|x|Eye {pose:Pose {position_m:head.transform([x,0.,0.]),..head},fov:[-0.7,0.8,-0.6,0.7]});
        (layer,eyes,head)
    }
    fn unpack(bytes:&[u8]) -> (Value,&[u8]) {
        validate_glb(bytes).unwrap();
        let n=u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        (serde_json::from_slice(&bytes[20..20+n]).unwrap(),&bytes[28+n..])
    }
    fn view<'a>(doc:&Value,bin:&'a [u8],index:usize) -> &'a [u8] {
        let v=&doc["bufferViews"][index];let at=v["byteOffset"].as_u64().unwrap() as usize;let n=v["byteLength"].as_u64().unwrap() as usize;
        &bin[at..at+n]
    }
    fn floats(doc:&Value,bin:&[u8],accessor:usize) -> Vec<f32> {
        view(doc,bin,doc["accessors"][accessor]["bufferView"].as_u64().unwrap() as usize)
            .chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect()
    }
    #[test]
    fn gltf_preserves_geometry_head_frame_eyes_and_both_stereo_textures() {
        let (layer,eyes,head)=fixture();let bytes=glb(&[layer],eyes,head).unwrap();let (doc,bin)=unpack(&bytes);
        assert_eq!(doc["scenes"].as_array().unwrap().len(),2);
        assert_eq!(doc["scene"],0);
        assert_eq!(doc["nodes"][0]["extras"]["DomeD"]["surfaceId"],"42");
        let p=floats(&doc,bin,0);
        for (a,b) in p.iter().zip([-1.,0.5,-2.,1.,0.5,-2.,-1.,-0.5,-2.,1.,-0.5,-2.]) {assert!((a-b).abs()<1e-5);}
        for n in floats(&doc,bin,1).chunks_exact(3) {assert!(Vec3::from_slice(n).distance(Vec3::Z)<1e-5);}
        let first=view(&doc,bin,doc["images"][0]["bufferView"].as_u64().unwrap() as usize);
        let second=view(&doc,bin,doc["images"][1]["bufferView"].as_u64().unwrap() as usize);
        assert_eq!(image::load_from_memory(first).unwrap().into_rgba8().as_raw(),&[255,0,0,128]);
        assert_eq!(image::load_from_memory(second).unwrap().into_rgba8().as_raw(),&[0,0,255,255]);
        assert_eq!(doc["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"][3],0.5);
        assert_eq!(doc["materials"][0]["extensions"]["KHR_materials_unlit"],json!({}));
        assert_eq!(doc["cameras"][0]["extras"]["DomeD"]["fov"],json!(eyes[0].fov));
        assert!((doc["nodes"][2]["translation"][0].as_f64().unwrap()+0.032).abs()<1e-5);
        assert!((doc["nodes"][3]["translation"][0].as_f64().unwrap()-0.032).abs()<1e-5);
        assert_eq!(Capture::from_xr("model/gltf-binary",bytes).unwrap().mime_type(),"model/gltf-binary");
    }
    use super::super::Capture;
    #[test]
    fn curved_mesh_matches_capture_surface_with_inward_normals_and_winding() {
        let (mut layer,eyes,head)=fixture();layer.stereo=false;
        layer.quad.dome=Some(crate::xr_input::DomePatch {frame:head,radius:2.,horizontal:1.5,top:0.4,bottom:-0.3});
        let bytes=glb(&[layer.clone()],eyes,head).unwrap();let (doc,bin)=unpack(&bytes);
        assert_eq!(doc["scenes"].as_array().unwrap().len(),1);
        let points=floats(&doc,bin,0);let normals=floats(&doc,bin,1);let uv=floats(&doc,bin,2);
        assert!(points.len()>12);
        let inv=Quat::from_array(head.orientation_xyzw).conjugate();
        for (p,t) in points.chunks_exact(3).zip(uv.chunks_exact(2)) {
            let expected=inv*(layer.quad.point([t[0],t[1]])-Vec3::from_array(head.position_m));
            assert!(expected.distance(Vec3::from_slice(p))<1e-5);
        }
        let indices:Vec<_>=view(&doc,bin,doc["accessors"][3]["bufferView"].as_u64().unwrap() as usize).chunks_exact(4)
            .map(|v|u32::from_le_bytes(v.try_into().unwrap()) as usize).collect();
        for triangle in indices.chunks_exact(3) {
            let p=triangle.iter().map(|i|Vec3::from_slice(&points[i*3..i*3+3])).collect::<Vec<_>>();
            assert!((p[1]-p[0]).cross(p[2]-p[0]).dot(Vec3::from_slice(&normals[triangle[0]*3..]))>0.);
        }
    }
    #[test]
    fn odd_width_stereo_matches_runtime_regions_and_ignores_unused_center_pixel() {
        let (mut layer,eyes,head)=fixture();
        layer.pixels=Pixels{width:5,height:1,rgba:[10,20,30,40,50].into_iter().flat_map(|v|[v,0,0,255]).collect()};
        let bytes=glb(&[layer],eyes,head).unwrap();let (doc,bin)=unpack(&bytes);
        for (index,wanted) in [[10,20],[40,50]].into_iter().enumerate() {
            let png=view(&doc,bin,doc["images"][index]["bufferView"].as_u64().unwrap() as usize);
            let image=image::load_from_memory(png).unwrap().into_rgba8();assert_eq!(image.dimensions(),(2,1));
            assert_eq!(image.as_raw().chunks_exact(4).map(|p|p[0]).collect::<Vec<_>>(),wanted);
        }
    }
    #[test]
    fn rejects_malformed_containers_invalid_geometry_and_unbounded_resources() {
        let (mut layer,eyes,head)=fixture();let valid=glb(&[layer.clone()],eyes,head).unwrap();
        for n in [0,1,12,20,valid.len()-1] {assert!(validate_glb(&valid[..n]).is_err());}
        for at in [0,4,8,12,16,20] {let mut bad=valid.clone();bad[at]^=255;assert!(validate_glb(&bad).is_err());}
        assert!(glb(&[],eyes,head).is_err());
        assert!(glb(&vec![layer.clone();MAX_LAYERS+1],eyes,head).is_err());
        let mut eye=eyes;eye[0].fov[0]=f32::NAN;assert!(glb(&[layer.clone()],eye,head).is_err());
        layer.opacity=0.;assert!(glb(&[layer.clone()],eyes,head).is_err());layer.opacity=1.;
        layer.quad.size[0]=f32::INFINITY;assert!(glb(&[layer.clone()],eyes,head).is_err());layer.quad.size[0]=2.;
        layer.quad.pose.orientation_xyzw=[0.;4];assert!(glb(&[layer],eyes,head).is_err());
        assert!(Capture::from_xr("image/png",b"glTF".to_vec()).is_err());
        assert!(Capture::from_xr("model/gltf-binary",b"\x89PNG\r\n\x1a\n".to_vec()).is_err());
    }
}
