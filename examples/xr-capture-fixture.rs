//! Deterministic spatial screenshot for independent glTF validation/rendering.
//! cargo run --example xr-capture-fixture -- /tmp/dome-capture.glb
use dome::{screenshot::{self, Eye, Layer, Pixels}, xr_input::{DomePatch, Quad}, Pose};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output=std::env::args_os().nth(1).ok_or("Supply an output GLB path")?;
    let head=Pose {position_m:[0.,1.6,0.],..Default::default()};
    let texture=|stereo:bool|Pixels {width:if stereo {128}else{64},height:48,
        rgba:(0..48).flat_map(|y|(0..if stereo {128}else{64}).flat_map(move |x| {
            let right=stereo && x>=64;let x=x%64;
            if (x/8+y/8)%2==0 {if right {[30,180,240,255]}else{[230,160,30,255]}}
            else {[30+x as u8,40+y as u8,80,255]}
        })).collect()};
    let mut layers=vec![Layer {quad:Quad {id:1,pose:head,size:[4.,2.],dome:Some(DomePatch {
        frame:head,radius:2.5,horizontal:1.6,top:0.4,bottom:-0.4})},pixels:texture(false),stereo:false,opacity:1.}];
    layers.push(Layer {quad:Quad {id:2,pose:Pose {position_m:[0.25,1.4,-1.8],..Default::default()},
        size:[1.2,0.9],dome:None},pixels:texture(true),stereo:true,opacity:1.});
    let eyes=[-0.032,0.032].map(|x|Eye {pose:Pose {position_m:[x,1.6,0.],..Default::default()},fov:[-0.7,0.8,-0.6,0.7]});
    let bytes=screenshot::glb(&layers,eyes,head)?;
    screenshot::validate_glb(&bytes)?;
    std::fs::write(&output,bytes)?;
    println!("{}",std::path::Path::new(&output).display());
    Ok(())
}
