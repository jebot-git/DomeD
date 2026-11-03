//! Full-resolution GPU window snapshots, projected without resizing clients.
use super::DesktopElement;
use crate::Dome;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            damage::OutputDamageTracker,
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
                texture::{TextureBuffer, TextureRenderElement},
                Element, Id, Kind, RenderElement,
            },
            gles::{
                GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform,
                UniformName, UniformType,
            },
            utils::{CommitCounter, DamageSet, OpaqueRegions},
            Bind, Offscreen,
        },
    },
    desktop::Window,
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Transform},
};
use std::collections::HashMap;
type Signature = Vec<(Id, CommitCounter, Rectangle<i32, Physical>)>;
struct Snapshot {
    signature: Signature,
    key: String,
    buffer: TextureBuffer<GlesTexture>,
    bytes: usize,
}
#[derive(Default)]
pub struct GlassRenderer {
    shader: Option<GlesTexProgram>,
    snapshots: HashMap<u64, Snapshot>,
}
pub struct GlassElement {
    texture: TextureRenderElement<GlesTexture>,
    program: GlesTexProgram,
    uniforms: Vec<Uniform<'static>>,
}
impl Element for GlassElement {
    fn id(&self) -> &Id {
        self.texture.id()
    }
    fn current_commit(&self) -> CommitCounter {
        self.texture.current_commit()
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        self.texture.src()
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.texture.geometry(scale)
    }
    fn transform(&self) -> Transform {
        self.texture.transform()
    }
    fn alpha(&self) -> f32 {
        self.texture.alpha()
    }
    fn damage_since(
        &self,
        scale: Scale<f64>,
        _: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        DamageSet::from_slice(&[Rectangle::from_size(self.geometry(scale).size)])
    }
    fn opaque_regions(&self, _: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }
}
impl RenderElement<GlesRenderer> for GlassElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        frame.override_default_tex_program(self.program.clone(), self.uniforms.clone());
        let result = <TextureRenderElement<GlesTexture> as RenderElement<GlesRenderer>>::draw(
            &self.texture,
            frame,
            src,
            dst,
            damage,
            opaque,
        );
        frame.clear_tex_program_override();
        result
    }
}
const SHADER: &str = r#"#version 100
//_DEFINES_
#ifdef EXTERNAL
#extension GL_OES_EGL_image_external : require
#endif
precision highp float;
#ifdef EXTERNAL
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif
uniform float alpha;
uniform vec2 tilt;
uniform vec4 mapping;
varying vec2 v_coords;
void main(){
 vec2 p=mapping.xy+v_coords*mapping.zw;
 float denominator=tilt.x+p.x*tilt.y;
 if(abs(denominator)<0.000001)discard;
 float x=p.x/denominator;
 vec2 uv=vec2(x,p.y*(1.0-x*tilt.y))+vec2(0.5);
 if(uv.x<0.0||uv.x>1.0||uv.y<0.0||uv.y>1.0)discard;
 gl_FragColor=texture2D(tex,uv)*alpha;
}
"#;
impl GlassRenderer {
    pub fn retain(&mut self, ids: &[u64]) {
        self.snapshots.retain(|id, _| ids.contains(id));
    }
    pub fn element(
        &mut self,
        renderer: &mut GlesRenderer,
        state: &Dome,
        id: u64,
        w: &dome::Window,
        window: &Window,
        pos: Point<i32, Logical>,
        angle: f32,
    ) -> Result<GlassElement, Box<dyn std::error::Error>> {
        if self.shader.is_none() {
            self.shader = Some(renderer.compile_custom_texture_shader(
                SHADER,
                &[
                    UniformName::new("tilt", UniformType::_2f),
                    UniformName::new("mapping", UniformType::_4f),
                ],
            )?);
        }
        let geo = window.geometry();
        let caption = if w.maximized {
            0
        } else {
            state.appearance.caption_height()
        };
        let border = if w.maximized { 0 } else { 4 };
        let width = geo.size.w + border * 2;
        let height = geo.size.h + caption + border;
        if width < 1 || height < 1 || width > 8192 || height > 8192 {
            return Err("Window exceeds full-resolution tilt texture limits".into());
        }
        let surface = window.toplevel().unwrap().wl_surface();
        let clients = render_elements_from_surface_tree::<
            GlesRenderer,
            WaylandSurfaceRenderElement<GlesRenderer>,
        >(
            renderer,
            surface,
            (border - geo.loc.x, caption - geo.loc.y),
            1.,
            1.,
            Kind::Unspecified,
        );
        let signature = clients
            .iter()
            .map(|e| (e.id().clone(), e.current_commit(), e.geometry(1.0.into())))
            .collect::<Signature>();
        let focused = state.scene.lock().unwrap().focused == Some(id);
        let key = format!(
            "{width}:{height}:{}:{focused}:{:?}",
            w.title, state.appearance
        );
        if self
            .snapshots
            .get(&id)
            .is_none_or(|s| s.signature != signature || s.key != key)
        {
            let bytes = width as usize * height as usize * 4;
            if self.snapshots.values().map(|s| s.bytes).sum::<usize>() + bytes > 512 * 1024 * 1024 {
                self.snapshots.clear();
            }
            let mut elements: Vec<DesktopElement> = clients.into_iter().map(Into::into).collect();
            if caption > 0 {
                let pixels = crate::qt::paint(
                    serde_json::json!({"kind":"frame","width":width,"height":height,"title":w.title,"active":focused,"appearance":state.appearance}),
                );
                let decoration = MemoryRenderBuffer::from_slice(
                    &pixels,
                    Fourcc::Argb8888,
                    (width, height),
                    1,
                    Transform::Normal,
                    None,
                );
                elements.push(
                    MemoryRenderBufferRenderElement::from_buffer(
                        renderer,
                        (0., 0.),
                        &decoration,
                        None,
                        None,
                        None,
                        Kind::Unspecified,
                    )?
                    .into(),
                );
            }
            let mut texture: GlesTexture =
                renderer.create_buffer(Fourcc::Argb8888, (width, height).into())?;
            {
                let mut target = renderer.bind(&mut texture)?;
                let mut tracker = OutputDamageTracker::new((width, height), 1., Transform::Normal);
                let result =
                    tracker.render_output(renderer, &mut target, 0, &elements, [0., 0., 0., 0.])?;
                result.sync.wait()?;
            }
            let buffer = TextureBuffer::from_texture(renderer, texture, 1, Transform::Normal, None);
            self.snapshots.insert(
                id,
                Snapshot {
                    signature,
                    key,
                    buffer,
                    bytes,
                },
            );
        }
        let rect = dome::Rect {
            x: (pos.x - border) as f32,
            y: (pos.y - caption) as f32,
            w: width as f32,
            h: height as f32,
        };
        let bounds = dome::looking_glass::bounds(rect, angle);
        let a = angle.clamp(-89.9, 89.9).to_radians();
        let texture = TextureRenderElement::from_texture_buffer(
            (bounds.x as f64, bounds.y as f64),
            &self.snapshots[&id].buffer,
            Some(1. - 0.35 * (angle.abs() / 90.)),
            None,
            Some((bounds.w as i32, bounds.h as i32).into()),
            Kind::Unspecified,
        );
        Ok(GlassElement {
            texture,
            program: self.shader.as_ref().unwrap().clone(),
            uniforms: vec![
                Uniform::new("tilt", [a.cos(), a.sin() / 2.5]),
                Uniform::new(
                    "mapping",
                    [
                        (bounds.x - rect.x) / rect.w - 0.5,
                        (bounds.y - rect.y) / rect.h - 0.5,
                        bounds.w / rect.w,
                        bounds.h / rect.h,
                    ],
                ),
            ],
        })
    }
}
