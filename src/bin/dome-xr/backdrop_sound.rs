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
const SFX_GAIN: f32 = 0.5;
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
    loop_start: i64,
    loop_end: i64,
    loop_head: Vec<f32>,
    synced: bool,
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
                read,
                seek,
                close,
                position: 0,
                loop_start: 0,
                loop_end: info.frames,
                loop_head: Vec::new(),
                synced: false,
                info,
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
    /// Remove authored intro/outro fades from the rain ambience. Inspect only
    /// bounded edge windows, then retain a two-second prefix for overlap; the
    /// long source itself continues to stream from disk.
    fn ambient_loop(&mut self) {
        let channels = self.info.channels as usize;
        let window = RATE as i64 / 4;
        let scan = (16 * RATE as i64).min(self.info.frames / 4);
        if scan >= window {
            let mut edges = Vec::new();
            for start in [0, self.info.frames - scan] {
                let mut levels = Vec::new();
                let mut data = vec![0.; window as usize * channels];
                for offset in (start..start + scan).step_by(window as usize) {
                    unsafe {
                        (self.seek)(self.file, offset, 0);
                        let got = (self.read)(self.file, data.as_mut_ptr(), window).max(0) as usize;
                        let samples = &data[..got * channels];
                        let rms = (samples.iter().map(|v| v * v).sum::<f32>()
                            / samples.len().max(1) as f32)
                            .sqrt();
                        levels.push((offset, rms));
                    }
                }
                let mut sorted: Vec<_> = levels.iter().map(|(_, rms)| *rms).collect();
                sorted.sort_by(f32::total_cmp);
                // Ignore peaks such as thunder when identifying the steady
                // noise level; keep anything above half the upper quartile.
                let threshold = sorted[sorted.len() * 3 / 4] * 0.5;
                edges.push(
                    levels
                        .into_iter()
                        .filter(|(_, rms)| *rms > threshold)
                        .map(|(offset, _)| offset)
                        .collect::<Vec<_>>(),
                );
            }
            self.loop_start = edges[0].first().copied().unwrap_or(0);
            self.loop_end = edges[1].last().map_or(self.info.frames, |offset| {
                (offset + window).min(self.info.frames)
            });
        }
        let overlap = (2 * RATE as i64).min((self.loop_end - self.loop_start) / 4);
        if overlap < 2 {
            return;
        }
        self.loop_head = vec![0.; overlap as usize * channels];
        unsafe {
            (self.seek)(self.file, self.loop_start, 0);
            let got = (self.read)(self.file, self.loop_head.as_mut_ptr(), overlap);
            if got != overlap {
                self.loop_head.clear();
            }
            self.position = (self.seek)(
                self.file,
                self.loop_start + self.loop_head.len() as i64 / channels as i64,
                0,
            );
        }
    }
    fn block(&mut self, time: f64, looping: bool, frames: usize) -> Vec<f32> {
        unsafe {
            let channels = self.info.channels as usize;
            let overlap = self.loop_head.len() as i64 / channels as i64;
            let restart = self.loop_start + overlap;
            let period = self.loop_end - restart;
            let expected = (time * RATE as f64).max(0.) as i64;
            let expected = if looping {
                restart + expected % period
            } else {
                expected.min(self.info.frames)
            };
            // Small scheduling jitter is absorbed by the stream. Seek only on a
            // real clock discontinuity; pause keeps both clocks stationary.
            // A wrap is not a clock discontinuity. Comparing linear file
            // positions near EOF used to seek back and repeat the fading tail.
            let distance = if looping {
                let delta = (self.position - expected).rem_euclid(period);
                delta.min(period - delta)
            } else {
                (self.position - expected).abs()
            };
            if !self.synced || distance > RATE as i64 / 8 {
                self.position = (self.seek)(self.file, expected, 0);
            }
            self.synced = true;
            let mut output = vec![0.; frames * channels];
            let mut cursor = 0;
            while cursor < frames {
                if looping && self.position >= self.loop_end {
                    self.position = (self.seek)(self.file, restart, 0);
                    if self.position < 0 {
                        break;
                    }
                }
                let remaining = if looping {
                    (frames - cursor).min((self.loop_end - self.position).max(0) as usize)
                } else {
                    frames - cursor
                };
                let got = (self.read)(
                    self.file,
                    output[cursor * channels..].as_mut_ptr(),
                    remaining as i64,
                )
                .max(0) as usize;
                if looping && overlap > 0 {
                    for i in 0..got {
                        let offset = self.position + i as i64 - (self.loop_end - overlap);
                        if offset < 0 {
                            continue;
                        }
                        // Equal-power overlap preserves the level of
                        // uncorrelated rain instead of fading through silence.
                        let phase =
                            offset as f32 / (overlap - 1) as f32 * std::f32::consts::FRAC_PI_2;
                        let (head_gain, tail_gain) = phase.sin_cos();
                        for channel in 0..channels {
                            let sample = &mut output[(cursor + i) * channels + channel];
                            *sample = *sample * tail_gain
                                + self.loop_head[offset as usize * channels + channel] * head_gain;
                        }
                    }
                }
                self.position += got as i64;
                cursor += got;
                if got == 0 {
                    break;
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
            let mut decoder = Decoder::open(&directory.join(&sound.file))?;
            if sound.looping && sound.attached && sound.spatial == 0. {
                decoder.ambient_loop();
            }
            streams.push(Stream {
                sound: sound.clone(),
                decoder,
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
            let gain =
                SFX_GAIN * state.volume * sound.gain * ((1. - spatial) + spatial * attenuation);
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

    fn decoded_wave(samples: &[f32], channels: u16) -> Result<Decoder> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dome-loop-{}-{}.wav",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let bytes = (samples.len() * 4) as u32;
        file.write_all(b"RIFF")?;
        file.write_all(&(36 + bytes).to_le_bytes())?;
        file.write_all(b"WAVEfmt ")?;
        file.write_all(&16u32.to_le_bytes())?;
        file.write_all(&3u16.to_le_bytes())?; // IEEE float WAV
        file.write_all(&channels.to_le_bytes())?;
        file.write_all(&(RATE as u32).to_le_bytes())?;
        file.write_all(&(RATE as u32 * channels as u32 * 4).to_le_bytes())?;
        file.write_all(&(channels * 4).to_le_bytes())?;
        file.write_all(&32u16.to_le_bytes())?;
        file.write_all(b"data")?;
        file.write_all(&bytes.to_le_bytes())?;
        for sample in samples {
            file.write_all(&sample.to_le_bytes())?;
        }
        drop(file);
        let decoder = Decoder::open(&path);
        std::fs::remove_file(path)?;
        decoder
    }

    #[test]
    fn clock_jitter_across_loop_wrap_does_not_repeat_the_tail() -> Result<()> {
        let samples: Vec<_> = (0..RATE).map(|i| i as f32 / RATE as f32).collect();
        let mut decoder = decoded_wave(&samples, 1)?;
        let time = (RATE as usize - FRAMES / 2) as f64 / RATE as f64;
        let first = decoder.block(time, true, FRAMES);
        assert_eq!(&first[FRAMES / 2..], &samples[..FRAMES / 2]);
        // The animation clock can still be just before its wrap on the next
        // audio block. Its small circular error must not trigger an EOF seek.
        let next = decoder.block(time + 0.005, true, FRAMES);
        assert_eq!(&next[..], &samples[FRAMES / 2..FRAMES * 3 / 2]);
        Ok(())
    }

    #[test]
    fn rain_loop_trims_fades_and_crossfades_both_channels() -> Result<()> {
        let length = RATE as usize * 12;
        let mut samples = vec![0.; length * 2];
        for i in RATE as usize / 2..length - RATE as usize {
            let sign = if i < length / 2 { 1. } else { -1. };
            samples[i * 2] = sign * 0.4;
            samples[i * 2 + 1] = sign * -0.2;
        }
        let mut decoder = decoded_wave(&samples, 2)?;
        decoder.ambient_loop();
        assert_eq!(decoder.loop_start, RATE as i64 / 2);
        assert_eq!(decoder.loop_end, length as i64 - RATE as i64);
        let overlap = decoder.loop_head.len() / 2;
        assert_eq!(overlap, RATE as usize * 2);
        let period = decoder.loop_end - decoder.loop_start - overlap as i64;
        let audio = decoder.block(
            (period - FRAMES as i64 / 2) as f64 / RATE as f64,
            true,
            FRAMES,
        );
        assert!(audio.chunks_exact(2).all(|s| s[0] > 0.39 && s[1] < -0.195));
        assert!(audio
            .chunks_exact(2)
            .map(|s| s[0])
            .collect::<Vec<_>>()
            .windows(2)
            .all(|p| (p[1] - p[0]).abs() < 0.001));
        // Non-looping sources still stop at EOF with zero-filled remainder.
        let mut one_shot = decoded_wave(&samples, 2)?;
        let ended = one_shot.block(length as f64 / RATE as f64, false, FRAMES);
        assert!(ended.iter().all(|s| *s == 0.));
        Ok(())
    }

    #[test]
    fn backdrop_gain_is_halved_before_user_volume() -> Result<()> {
        let mut mixer = Mixer {
            streams: vec![Stream {
                sound: Sound {
                    file: String::new(),
                    gain: 1.,
                    looping: true,
                    position: [0.; 3],
                    attached: true,
                    min_distance: 1.,
                    max_distance: 500.,
                    spatial: 0.,
                    motions: vec![],
                },
                decoder: decoded_wave(&vec![0.4; RATE as usize], 1)?,
            }],
            motions: vec![],
        };
        let mut state = State {
            volume: 1.,
            ..Default::default()
        };
        assert!(mixer.mix(&state).iter().all(|v| (*v - 0.2).abs() < 1e-6));
        state.volume = 0.5;
        assert!(mixer.mix(&state).iter().all(|v| (*v - 0.1).abs() < 1e-6));
        state.volume = 0.;
        assert!(mixer.mix(&state).iter().all(|v| *v == 0.));
        Ok(())
    }
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
        let rain = &mut mixer
            .streams
            .iter_mut()
            .find(|s| s.sound.attached)
            .ok_or("No rain ambience")?
            .decoder;
        let overlap = rain.loop_head.len() as i64 / rain.info.channels as i64;
        let period = rain.loop_end - rain.loop_start - overlap;
        assert!(rain.loop_start > 0 && rain.loop_end < rain.info.frames);
        let seam = rain.block(
            (period - RATE as i64 / 2) as f64 / RATE as f64,
            true,
            RATE as usize,
        );
        assert!(seam.iter().all(|v| v.is_finite()));
        let seam_rms = (seam.iter().map(|s| s * s).sum::<f32>() / seam.len() as f32).sqrt();
        assert!(
            seam_rms > 0.01,
            "Rain must remain audible across the loop seam"
        );
        println!("Rain loop: {:.2}s .. {:.2}s, {:.2}s overlap, {} cached float samples, seam RMS {seam_rms:.6}",
            rain.loop_start as f64 / RATE as f64, rain.loop_end as f64 / RATE as f64,
            overlap as f64 / RATE as f64, rain.loop_head.len());
        Ok(())
    }
}
