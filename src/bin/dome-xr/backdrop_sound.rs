//! Bounded-memory native audio streaming through PulseAudio/PipeWire.
//! Sound files are pitch/reverse baked Opus; no player subprocess is required.
use dome::xr_backdrop::{Motion, Sound};
use glam::{Mat4, Vec3};
use libloading::Library;
use std::{
    ffi::{c_char, c_int, c_void, CString},
    path::Path,
    sync::{Arc, Condvar, Mutex},
    thread,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const RATE: i32 = 48000;
const FRAMES: usize = 960;
#[repr(C)]
#[derive(Default)]
struct Info {
    frames: i64,
    samplerate: i32,
    channels: i32,
    format: i32,
    sections: i32,
    seekable: i32,
}
struct Decoder {
    _library: Library,
    file: *mut c_void,
    info: Info,
    read: unsafe extern "C" fn(*mut c_void, *mut f32, i64) -> i64,
    seek: unsafe extern "C" fn(*mut c_void, i64, c_int) -> i64,
    close: unsafe extern "C" fn(*mut c_void) -> c_int,
    position: i64,
}
impl Decoder {
    fn open(path: &Path) -> Result<Self> {
        unsafe {
            let library = Library::new("libsndfile.so.1")?;
            let open =
                *library
                    .get::<unsafe extern "C" fn(*const c_char, c_int, *mut Info) -> *mut c_void>(
                        b"sf_open\0",
                    )?;
            let read = *library.get(b"sf_readf_float\0")?;
            let seek = *library.get(b"sf_seek\0")?;
            let close = *library.get(b"sf_close\0")?;
            let mut info = Info::default();
            let name = CString::new(path.as_os_str().as_encoded_bytes())?;
            let file = open(name.as_ptr(), 0x10, &mut info);
            if file.is_null() {
                return Err(format!("Cannot decode backdrop audio {}", path.display()).into());
            }
            let decoder = Self {
                _library: library,
                file,
                info,
                read,
                seek,
                close,
                position: 0,
            };
            if decoder.info.samplerate != RATE
                || !(1..=2).contains(&decoder.info.channels)
                || decoder.info.frames <= 0
                || decoder.info.seekable == 0
            {
                return Err("Backdrop audio must be seekable mono/stereo 48 kHz".into());
            }
            Ok(decoder)
        }
    }
    fn block(&mut self, time: f64, looping: bool, frames: usize) -> Vec<f32> {
        unsafe {
            let expected = (time * RATE as f64).max(0.) as i64;
            let expected = if looping {
                expected % self.info.frames
            } else {
                expected.min(self.info.frames)
            };
            // Small scheduling jitter is absorbed by the stream. Seek only on a
            // real clock discontinuity; pause keeps both clocks stationary.
            if (self.position - expected).abs() > RATE as i64 / 8 {
                self.position = (self.seek)(self.file, expected, 0);
            }
            let channels = self.info.channels as usize;
            let mut output = vec![0.; frames * channels];
            let mut cursor = 0;
            while cursor < frames {
                let got = (self.read)(
                    self.file,
                    output[cursor * channels..].as_mut_ptr(),
                    (frames - cursor) as i64,
                )
                .max(0) as usize;
                self.position += got as i64;
                cursor += got;
                if got == 0 {
                    if !looping {
                        break;
                    }
                    self.position = (self.seek)(self.file, 0, 0);
                    if self.position < 0 {
                        break;
                    }
                }
            }
            output
        }
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            (self.close)(self.file);
        }
    }
}
#[derive(Clone, Default)]
pub struct State {
    pub active: bool,
    pub volume: f32,
    pub seconds: f64,
    pub position: [f32; 3],
    pub right: [f32; 3],
    pub stop: bool,
    pub updated: Option<std::time::Instant>,
}
impl State {
    fn audible(&self) -> bool {
        self.active
            && self.volume > 0.
            && self
                .updated
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(250))
    }
}
struct Stream {
    sound: Sound,
    decoder: Decoder,
}
struct Mixer {
    streams: Vec<Stream>,
    motions: Vec<Motion>,
}
impl Mixer {
    fn load(directory: &Path, sounds: &[Sound], motions: &[Motion]) -> Result<Self> {
        let mut streams = Vec::new();
        for sound in sounds {
            streams.push(Stream {
                sound: sound.clone(),
                decoder: Decoder::open(&directory.join(&sound.file))?,
            });
        }
        Ok(Self {
            streams,
            motions: motions.to_vec(),
        })
    }
    fn mix(&mut self, state: &State) -> Vec<f32> {
        let mut output = vec![0.; FRAMES * 2];
        let listener = Vec3::from_array(state.position);
        let right = Vec3::from_array(state.right).normalize_or_zero();
        for stream in &mut self.streams {
            let sound = &stream.sound;
            let transform = sound.motions.iter().fold(Mat4::IDENTITY, |m, id| {
                m * self.motions[*id].matrix(state.seconds)
            });
            let position = transform.transform_point3(Vec3::from_array(sound.position));
            let delta = position - listener;
            let distance = delta.length();
            let attenuation = if sound.attached {
                1.
            } else if distance >= sound.max_distance {
                0.
            } else {
                sound.min_distance / distance.max(sound.min_distance)
            };
            let spatial = sound.spatial;
            let gain = state.volume * sound.gain * ((1. - spatial) + spatial * attenuation);
            let pan = delta.normalize_or_zero().dot(right).clamp(-1., 1.) * spatial;
            let left = (1. - pan).sqrt();
            let right = (1. + pan).sqrt();
            if gain < 0.000001 {
                continue;
            }
            let data = stream.decoder.block(state.seconds, sound.looping, FRAMES);
            let channels = stream.decoder.info.channels as usize;
            for (i, pair) in output.chunks_exact_mut(2).enumerate() {
                let l = data[i * channels];
                let r = data[i * channels + channels - 1];
                pair[0] += l * gain * left;
                pair[1] += r * gain * right;
            }
        }
        for x in &mut output {
            *x = x.clamp(-1., 1.);
        }
        output
    }
}
#[repr(C)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}
#[repr(C)]
struct BufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}
struct Sink {
    _library: Library,
    handle: *mut c_void,
    write: unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int,
    flush: unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int,
    free: unsafe extern "C" fn(*mut c_void),
}
impl Sink {
    fn open() -> Result<Self> {
        unsafe {
            let library = Library::new("libpulse-simple.so.0")?;
            let new = *library.get::<unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                c_int,
                *const c_char,
                *const c_char,
                *const SampleSpec,
                *const c_void,
                *const BufferAttr,
                *mut c_int,
            ) -> *mut c_void>(b"pa_simple_new\0")?;
            let write = *library.get(b"pa_simple_write\0")?;
            let flush = *library.get(b"pa_simple_flush\0")?;
            let free = *library.get(b"pa_simple_free\0")?;
            let spec = SampleSpec {
                format: 5,
                rate: RATE as u32,
                channels: 2,
            }; // PA_SAMPLE_FLOAT32LE
            let attr = BufferAttr {
                maxlength: u32::MAX,
                tlength: (RATE * 8 / 12) as u32,
                prebuf: 0,
                minreq: (FRAMES * 8) as u32,
                fragsize: u32::MAX,
            };
            let mut error = 0;
            let handle = new(
                std::ptr::null(),
                c"DomeD XR backdrop".as_ptr(),
                1,
                std::ptr::null(),
                c"Precipice ambience and traffic".as_ptr(),
                &spec,
                std::ptr::null(),
                &attr,
                &mut error,
            );
            if handle.is_null() {
                return Err(format!("PulseAudio/PipeWire backdrop stream failed ({error})").into());
            }
            Ok(Self {
                _library: library,
                handle,
                write,
                flush,
                free,
            })
        }
    }
    fn write(&mut self, data: &[f32]) -> bool {
        unsafe {
            let mut error = 0;
            (self.write)(
                self.handle,
                data.as_ptr().cast(),
                std::mem::size_of_val(data),
                &mut error,
            ) >= 0
        }
    }
    fn flush(&mut self) {
        unsafe {
            let mut error = 0;
            (self.flush)(self.handle, &mut error);
        }
    }
}
impl Drop for Sink {
    fn drop(&mut self) {
        unsafe {
            (self.free)(self.handle);
        }
    }
}

pub struct Player {
    state: Arc<(Mutex<State>, Condvar)>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Player {
    pub fn new(directory: &Path, sounds: &[Sound], motions: &[Motion]) -> Self {
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let shared = state.clone();
        let directory = directory.to_owned();
        let sounds = sounds.to_vec();
        let motions = motions.to_vec();
        let worker = thread::spawn(move || {
            let mut mixer = match Mixer::load(&directory, &sounds, &motions) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("XR backdrop sound unavailable: {e}");
                    return;
                }
            };
            let mut sink: Option<Sink> = None;
            let mut playing = false;
            loop {
                let snapshot = {
                    let (lock, changed) = &*shared;
                    let mut value = lock.lock().unwrap();
                    while !value.audible() && !value.stop {
                        if playing {
                            drop(value);
                            if let Some(s) = sink.as_mut() {
                                s.flush();
                            }
                            playing = false;
                            value = lock.lock().unwrap();
                            continue;
                        }
                        value = changed.wait(value).unwrap();
                    }
                    if value.stop {
                        break;
                    }
                    value.clone()
                };
                if sink.is_none() {
                    match Sink::open() {
                        Ok(s) => {
                            sink = Some(s);
                            eprintln!(
                                "XR backdrop audio streaming: {} original sources",
                                sounds.len()
                            );
                        }
                        Err(e) => {
                            eprintln!("XR backdrop sound unavailable: {e}");
                            return;
                        }
                    }
                }
                let data = mixer.mix(&snapshot);
                if !sink.as_mut().unwrap().write(&data) {
                    eprintln!("XR backdrop audio stream disconnected");
                    break;
                }
                playing = true;
            }
            if let Some(mut sink) = sink {
                sink.flush();
            }
        });
        Self {
            state,
            worker: Some(worker),
        }
    }
    pub fn update(
        &self,
        active: bool,
        seconds: f64,
        position: [f32; 3],
        right: [f32; 3],
        volume: f32,
    ) {
        let (lock, changed) = &*self.state;
        let mut state = lock.lock().unwrap();
        state.active = active;
        state.volume = volume.clamp(0., 1.);
        state.updated = Some(std::time::Instant::now());
        state.seconds = seconds;
        state.position = position;
        state.right = right;
        changed.notify_one();
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        let (lock, changed) = &*self.state;
        lock.lock().unwrap().stop = true;
        changed.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_tracking_and_fullscreen_suspension_mute_audio() {
        let mut state = State {
            active: true,
            volume: 1.,
            updated: Some(std::time::Instant::now()),
            ..Default::default()
        };
        assert!(state.audible());
        state.volume = 0.;
        assert!(!state.audible());
        state.volume = 1.;
        state.active = false;
        assert!(!state.audible());
        state.active = true;
        state.updated = Some(std::time::Instant::now() - std::time::Duration::from_millis(300));
        assert!(!state.audible());
    }
    #[test]
    #[ignore = "requires imported original audio; offline decoding only"]
    fn original_audio_decodes_and_mixes_without_opening_a_sink() -> Result<()> {
        let path = dome::xr_backdrop::directory().ok_or("Missing assets")?;
        let (asset, _) = dome::xr_backdrop::Asset::load(&path)?;
        assert!(!asset.sounds.is_empty());
        let mut mixer = Mixer::load(&path, &asset.sounds, &asset.motions)?;
        let state = State {
            active: true,
            volume: 1.,
            seconds: 3.,
            right: [1., 0., 0.],
            ..Default::default()
        };
        let audio = mixer.mix(&state);
        let mut quiet = Mixer::load(&path, &asset.sounds, &asset.motions)?;
        let low = quiet.mix(&State {
            volume: 0.5,
            ..state.clone()
        });
        assert!(
            audio
                .iter()
                .zip(low)
                .all(|(full, half)| (full * 0.5 - half).abs() < 1e-5),
            "Backdrop volume must scale all original sources"
        );
        let rms = (audio.iter().map(|v| v * v).sum::<f32>() / audio.len() as f32).sqrt();
        assert!(rms > 0.00001 && rms < 1.);
        assert!(audio.iter().all(|v| v.is_finite()));
        println!(
            "Original audio: {} active sources, RMS {rms:.6}, {} float samples",
            mixer.streams.len(),
            audio.len()
        );
        Ok(())
    }
}
