//! Private stdin-driven fixture, compiled only with input-test-fixture and
//! accepted only in a nested compositor. Normal builds have no input endpoint.
use serde::Deserialize;
use smithay::{backend::input::TouchSlot,reexports::calloop::channel};
use std::io::{self,BufRead,Write};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    token:u64,
    action:String,
    slot:Option<u32>,
    point:Option<[f64;2]>,
    device:Option<u32>,
    pressure:Option<f64>,
    distance:Option<f64>,
    tilt:Option<[f64;2]>,
    rotation:Option<f64>,
    slider:Option<f64>,
    wheel:Option<(f64,i32)>,
    button:Option<u32>,
    pressed:Option<bool>,
    eraser:Option<bool>,
    capabilities:Option<u32>,
}
fn reply(value:serde_json::Value) {
    println!("DOME_TOUCH_ACK {}",value);
    let _=io::stdout().flush();
}
pub fn receiver() -> channel::Channel<Command> {
    let (sender,receiver)=channel::channel();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line.ok().and_then(|line|serde_json::from_str::<Command>(&line).ok()) {
                Some(command)=>if sender.send(command).is_err() {break;},
                None=>reply(serde_json::json!({"error":"Invalid fixture command"})),
            }
        }
    });
    receiver
}
impl Command {
    pub fn apply(self, state:&mut crate::Dome) {
        let time=state.start_time.elapsed().as_millis() as u32;
        let result=(|| -> Result<(),&'static str> {
            if self.action.starts_with("tablet-") {
                use smithay::{backend::input::{ButtonState,TabletToolDescriptor,TabletToolType,TabletToolCapabilities},wayland::tablet_manager::TabletDescriptor};
                let descriptor=TabletToolDescriptor {tool_type:if self.eraser.unwrap_or(false) {TabletToolType::Eraser}else{TabletToolType::Pen},hardware_serial:self.slot.unwrap_or(0) as u64+1,hardware_id_wacom:0,
                    capabilities:self.capabilities.map(TabletToolCapabilities::from_bits_truncate).unwrap_or(TabletToolCapabilities::PRESSURE|TabletToolCapabilities::TILT|TabletToolCapabilities::DISTANCE|TabletToolCapabilities::ROTATION|TabletToolCapabilities::SLIDER|TabletToolCapabilities::WHEEL)};
                let device=TabletDescriptor {name:format!("Fixture tablet {}",self.device.unwrap_or(0)),usb_id:None,syspath:None};
                let axes=crate::tablet::Axes {pressure:self.pressure,distance:self.distance,tilt:self.tilt.map(|[x,y]|(x,y)),rotation:self.rotation,slider:self.slider,wheel:self.wheel};
                let point=||->Result<_,&'static str>{let [x,y]=self.point.ok_or("point required")?;if !x.is_finite()||!y.is_finite(){return Err("finite point required");}Ok((x,y).into())};
                match self.action.as_str() {
                    "tablet-in"=>state.tablet_in(device,descriptor,point()?,axes,time),
                    "tablet-move"=>state.tablet_motion(&descriptor,point()?,axes,time),
                    "tablet-down"|"tablet-up"=>{
                        if self.point.is_some(){state.tablet_motion(&descriptor,point()?,axes,time);}
                        state.tablet_tip(&descriptor,self.action=="tablet-down",time);
                    },
                    "tablet-out"=>state.tablet_out(&descriptor,time),
                    "tablet-remove"=>state.tablet_remove(&device,time),
                    "tablet-button"=>state.tablet_button(&descriptor,self.button.ok_or("button required")?,if self.pressed.ok_or("pressed required")? {ButtonState::Pressed}else{ButtonState::Released},time),
                    _=>return Err("Unknown tablet fixture action"),
                }
                return Ok(());
            }
            match self.action.as_str() {
                "down"|"move"=>{
                    let slot=TouchSlot::from(Some(self.slot.ok_or("slot required")?));
                    let [x,y]=self.point.ok_or("point required")?;
                    if !x.is_finite() || !y.is_finite() {return Err("finite point required");}
                    if self.action=="down" {state.touch_down(slot,(x,y).into(),time);} else {state.touch_motion(slot,(x,y).into(),time);}
                },
                "up"=>state.touch_up(TouchSlot::from(Some(self.slot.ok_or("slot required")?)),time),
                "frame"=>state.touch_frame(),
                "cancel"=>state.cancel_touches(),
                "capability-off"=>{state.cancel_touches();state.seat.remove_touch();},
                "capability-on"=>{if state.seat.get_touch().is_none() {state.seat.add_touch();}},
                _=>return Err("Unknown fixture action"),
            }
            Ok(())
        })();
        reply(match result {Ok(())=>serde_json::json!({"token":self.token,"handled":true}),Err(error)=>serde_json::json!({"token":self.token,"error":error})});
    }
}
