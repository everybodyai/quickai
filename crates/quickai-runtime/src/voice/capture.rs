//! The microphone, as samples of 16 kHz mono from -1 to 1, gathered on a thread of its own and taken by the window
//! as it draws.
//!
//! - Linux: ALSA's "default" device (which PipeWire and PulseAudio serve), or one the person chose, loaded from
//!   `libasound.so.2` when the microphone is first used: the window links no sound library, so it starts on a system
//!   that has none, and only the microphone says it cannot work there.
//! - Windows and macOS: cpal (WASAPI, CoreAudio), resampled here.
//! - `QUICKAI_FAKE_MIC=<file.wav>`: the file is played in the microphone's place, in real time, then silence (for
//!   tests); `QUICKAI_FAKE_MIC=blocked` is a microphone the system keeps from the app.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::RATE;

/// Why the microphone does not work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MicError {
    /// The system keeps the microphone from apps (Windows' and macOS' privacy settings).
    Blocked(String),
    /// There is none, or not the one chosen.
    NoMicrophone(String),
    /// No sound system to ask (no `libasound.so.2`).
    NoSoundSystem(String),
    Failed(String),
}

impl MicError {
    /// What went wrong, in the system's words.
    pub fn detail(&self) -> &str {
        match self {
            MicError::Blocked(d)
            | MicError::NoMicrophone(d)
            | MicError::NoSoundSystem(d)
            | MicError::Failed(d) => d,
        }
    }
}

/// A microphone the person can choose: what the sound system knows it by, and what it is called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Microphone {
    pub id: String,
    pub name: String,
}

/// What the thread of a recording shares with the window.
#[derive(Default)]
struct Shared {
    samples: Mutex<Vec<f32>>,
    error: Mutex<Option<MicError>>,
    /// Some sample was not silence: a blocked microphone on macOS gives silence and no error.
    sound: AtomicBool,
}

/// A recording that goes on until it is dropped.
pub struct Recording {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Recording {
    /// The samples that came since the last time.
    pub fn take(&self) -> Vec<f32> {
        self.shared
            .samples
            .lock()
            .map(|mut s| std::mem::take(&mut *s))
            .unwrap_or_default()
    }

    /// Why it stopped, when it did.
    pub fn error(&self) -> Option<MicError> {
        self.shared.error.lock().ok().and_then(|e| e.clone())
    }

    /// Anything but silence came.
    pub fn heard_sound(&self) -> bool {
        self.shared.sound.load(Ordering::Relaxed)
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Shared {
    fn give(&self, samples: &[f32]) {
        if samples.iter().any(|s| s.abs() > 1e-4) {
            self.sound.store(true, Ordering::Relaxed);
        }
        if let Ok(mut all) = self.samples.lock() {
            // a window that stopped taking them (it hangs) does not fill the memory: ten minutes at most
            if all.len() < RATE as usize * 600 {
                all.extend_from_slice(samples);
            }
        }
    }

    fn fail(&self, error: MicError) {
        if let Ok(mut e) = self.error.lock() {
            e.get_or_insert(error);
        }
    }
}

/// Starts recording from `microphone` (`None`: the system's own choice).
pub fn start(microphone: Option<&str>) -> Result<Recording, MicError> {
    let shared = Arc::new(Shared::default());
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(fake) = std::env::var_os("QUICKAI_FAKE_MIC") {
        let fake = std::path::PathBuf::from(fake);
        if fake.as_os_str() == "blocked" {
            return Err(MicError::Blocked(
                "the test's microphone is kept from apps".into(),
            ));
        }
        let worker = fake::start(&fake, Arc::clone(&shared), Arc::clone(&stop))?;
        return Ok(Recording {
            shared,
            stop,
            worker: Some(worker),
        });
    }
    let worker = platform::start(microphone, Arc::clone(&shared), Arc::clone(&stop))?;
    Ok(Recording {
        shared,
        stop,
        worker: Some(worker),
    })
}

/// The microphones the person can choose from (the system's own choice is not among them: it is `None`).
pub fn microphones() -> Vec<Microphone> {
    if std::env::var_os("QUICKAI_FAKE_MIC").is_some() {
        return vec![Microphone {
            id: "fake".into(),
            name: "The test's recording".into(),
        }];
    }
    platform::microphones()
}

/// Turns samples of any rate and any number of channels into 16 kHz mono: the channels are averaged, the rate is
/// changed by linear steps after a box filter as wide as a step (enough for a voice, which whisper hears at 16 kHz).
pub struct Resampler {
    channels: usize,
    /// Input samples to an output one.
    step: f64,
    /// Where the next output sample is, in `filtered`.
    pos: f64,
    filtered: Vec<f32>,
    /// The box filter: its last samples and their sum.
    window: std::collections::VecDeque<f32>,
    width: usize,
    sum: f32,
}

impl Resampler {
    pub fn new(rate: u32, channels: usize) -> Resampler {
        let step = rate as f64 / RATE as f64;
        Resampler {
            channels: channels.max(1),
            step,
            pos: 0.0,
            filtered: Vec::new(),
            window: std::collections::VecDeque::new(),
            width: (step.round() as usize).max(1),
            sum: 0.0,
        }
    }

    /// Takes interleaved samples; what they make at 16 kHz is added to `out`.
    pub fn push(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        // 16 kHz mono already: as it is
        if self.channels == 1 && (self.step - 1.0).abs() < 1e-9 {
            out.extend_from_slice(interleaved);
            return;
        }
        for frame in interleaved.chunks(self.channels) {
            let mono = frame.iter().sum::<f32>() / frame.len() as f32;
            self.window.push_back(mono);
            self.sum += mono;
            if self.window.len() > self.width {
                self.sum -= self.window.pop_front().unwrap_or(0.0);
            }
            self.filtered.push(self.sum / self.window.len() as f32);
        }
        if (self.step - 1.0).abs() < 1e-9 {
            out.append(&mut self.filtered);
            return;
        }
        while self.pos + 1.0 < self.filtered.len() as f64 {
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            out.push(self.filtered[i] * (1.0 - frac) + self.filtered[i + 1] * frac);
            self.pos += self.step;
        }
        let used = (self.pos as usize).min(self.filtered.len());
        self.filtered.drain(..used);
        self.pos -= used as f64;
    }
}

/// A WAV file played as the microphone.
mod fake {
    use super::*;

    pub fn start(
        path: &std::path::Path,
        shared: Arc<Shared>,
        stop: Arc<AtomicBool>,
    ) -> Result<JoinHandle<()>, MicError> {
        let samples = read_wav(path).map_err(MicError::NoMicrophone)?;
        Ok(std::thread::spawn(move || {
            let chunk = (RATE / 50) as usize;
            let mut at = 0;
            while !stop.load(Ordering::SeqCst) {
                let piece: Vec<f32> = if at < samples.len() {
                    let end = (at + chunk).min(samples.len());
                    let p = samples[at..end].to_vec();
                    at = end;
                    p
                } else {
                    vec![0.0; chunk]
                };
                shared.give(&piece);
                std::thread::sleep(Duration::from_millis(20));
            }
        }))
    }

    /// The samples of a WAV file of 16-bit PCM, at 16 kHz mono.
    pub fn read_wav(path: &std::path::Path) -> Result<Vec<f32>, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(format!("{} is not a WAV file", path.display()));
        }
        let (mut channels, mut rate, mut bits) = (1u16, RATE, 16u16);
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let id = &bytes[at..at + 4];
            let len =
                u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
                    as usize;
            let body = &bytes[at + 8..(at + 8 + len).min(bytes.len())];
            if id == b"fmt " && body.len() >= 16 {
                channels = u16::from_le_bytes([body[2], body[3]]);
                rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                bits = u16::from_le_bytes([body[14], body[15]]);
            } else if id == b"data" {
                if bits != 16 {
                    return Err(format!("{} is not 16-bit PCM", path.display()));
                }
                let pcm: Vec<f32> = body
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0)
                    .collect();
                let mut out = Vec::new();
                Resampler::new(rate, channels as usize).push(&pcm, &mut out);
                return Ok(out);
            }
            at += 8 + len + (len & 1);
        }
        Err(format!("{} has no sound in it", path.display()))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    //! ALSA, from `libasound.so.2`, loaded once.

    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CStr, CString};
    use std::sync::OnceLock;

    use super::*;

    const STREAM_CAPTURE: c_int = 1;
    const FORMAT_S16_LE: c_int = 2;
    const ACCESS_RW_INTERLEAVED: c_int = 3;

    struct Alsa {
        _lib: libloading::Library,
        open: unsafe extern "C" fn(*mut *mut c_void, *const c_char, c_int, c_int) -> c_int,
        set_params:
            unsafe extern "C" fn(*mut c_void, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int,
        readi: unsafe extern "C" fn(*mut c_void, *mut c_void, c_ulong) -> c_long,
        recover: unsafe extern "C" fn(*mut c_void, c_int, c_int) -> c_int,
        close: unsafe extern "C" fn(*mut c_void) -> c_int,
        strerror: unsafe extern "C" fn(c_int) -> *const c_char,
        hint: unsafe extern "C" fn(c_int, *const c_char, *mut *mut *mut c_void) -> c_int,
        get_hint: unsafe extern "C" fn(*const c_void, *const c_char) -> *mut c_char,
        free_hint: unsafe extern "C" fn(*mut *mut c_void) -> c_int,
    }

    fn alsa() -> Result<&'static Alsa, MicError> {
        static ALSA: OnceLock<Result<Alsa, String>> = OnceLock::new();
        ALSA.get_or_init(|| {
            // SAFETY: libasound is a C library with no initialisers that run code of ours; the functions are looked up by
            // the names and signatures of its public header (alsa/pcm.h, alsa/control.h)
            unsafe {
                let lib = libloading::Library::new("libasound.so.2").map_err(|e| e.to_string())?;
                macro_rules! get {
                    ($name:literal) => {
                        *lib.get(concat!($name, "\0").as_bytes())
                            .map_err(|e| e.to_string())?
                    };
                }
                Ok(Alsa {
                    open: get!("snd_pcm_open"),
                    set_params: get!("snd_pcm_set_params"),
                    readi: get!("snd_pcm_readi"),
                    recover: get!("snd_pcm_recover"),
                    close: get!("snd_pcm_close"),
                    strerror: get!("snd_strerror"),
                    hint: get!("snd_device_name_hint"),
                    get_hint: get!("snd_device_name_get_hint"),
                    free_hint: get!("snd_device_name_free_hint"),
                    _lib: lib,
                })
            }
        })
        .as_ref()
        .map_err(|e| {
            MicError::NoSoundSystem(format!(
                "the sound system's library (libasound.so.2) could not be loaded: {e}"
            ))
        })
    }

    fn words(alsa: &Alsa, code: c_int) -> String {
        // SAFETY: snd_strerror returns a static string for any code
        unsafe {
            CStr::from_ptr((alsa.strerror)(code))
                .to_string_lossy()
                .into_owned()
        }
    }

    /// The handle of an open device, handed to the recording's thread.
    struct Pcm(*mut c_void);
    // SAFETY: the handle is used by the one thread it is handed to, and closed by it
    unsafe impl Send for Pcm {}

    pub fn start(
        microphone: Option<&str>,
        shared: Arc<Shared>,
        stop: Arc<AtomicBool>,
    ) -> Result<JoinHandle<()>, MicError> {
        let alsa = alsa()?;
        let name = microphone.unwrap_or("default");
        let c_name = CString::new(name)
            .map_err(|_| MicError::NoMicrophone(format!("{name:?} is not a device")))?;
        let mut pcm: *mut c_void = std::ptr::null_mut();
        // SAFETY: the arguments are as snd_pcm_open takes them; `pcm` is written on success
        let opened = unsafe { (alsa.open)(&mut pcm, c_name.as_ptr(), STREAM_CAPTURE, 0) };
        if opened < 0 {
            let said = format!("{name}: {}", words(alsa, opened));
            return Err(match -opened {
                1 | 13 => MicError::Blocked(said),
                2 | 6 | 19 => MicError::NoMicrophone(said),
                _ => MicError::Failed(said),
            });
        }
        // 16-bit mono at 16 kHz, converted by ALSA's plug layer when the device has other ones; 100 ms of latency
        // SAFETY: `pcm` is open
        let set = unsafe {
            (alsa.set_params)(
                pcm,
                FORMAT_S16_LE,
                ACCESS_RW_INTERLEAVED,
                1,
                RATE,
                1,
                100_000,
            )
        };
        if set < 0 {
            let said = format!("{name} does not record as asked: {}", words(alsa, set));
            // SAFETY: `pcm` is open
            unsafe { (alsa.close)(pcm) };
            return Err(MicError::Failed(said));
        }
        let pcm = Pcm(pcm);
        Ok(std::thread::spawn(move || {
            let pcm = pcm;
            let frames = (RATE / 50) as usize;
            let mut buf = vec![0i16; frames];
            while !stop.load(Ordering::SeqCst) {
                // SAFETY: `buf` holds `frames` samples of one channel
                let got =
                    unsafe { (alsa.readi)(pcm.0, buf.as_mut_ptr().cast(), frames as c_ulong) };
                if got < 0 {
                    // an overrun (the thread was late): recovered, and on
                    // SAFETY: `pcm` is open
                    let again = unsafe { (alsa.recover)(pcm.0, got as c_int, 1) };
                    if again < 0 {
                        shared.fail(MicError::Failed(words(alsa, again)));
                        break;
                    }
                    continue;
                }
                let samples: Vec<f32> = buf[..got as usize]
                    .iter()
                    .map(|s| *s as f32 / 32768.0)
                    .collect();
                shared.give(&samples);
            }
            // SAFETY: `pcm` is open, and closed once
            unsafe { (alsa.close)(pcm.0) };
        }))
    }

    pub fn microphones() -> Vec<Microphone> {
        let Ok(alsa) = alsa() else {
            return Vec::new();
        };
        let mut hints: *mut *mut c_void = std::ptr::null_mut();
        // SAFETY: the arguments are as snd_device_name_hint takes them; the list is freed below
        if unsafe { (alsa.hint)(-1, c"pcm".as_ptr(), &mut hints) } < 0 || hints.is_null() {
            return Vec::new();
        }
        let get = |hint: *const c_void, what: &CStr| -> Option<String> {
            // SAFETY: `hint` is an item of the list; the string it gives is the caller's to free
            unsafe {
                let p = (alsa.get_hint)(hint, what.as_ptr());
                if p.is_null() {
                    return None;
                }
                let s = CStr::from_ptr(p).to_string_lossy().into_owned();
                libc::free(p.cast());
                Some(s)
            }
        };
        let mut found = Vec::new();
        let mut at = hints;
        // SAFETY: the list ends with a null item
        while !unsafe { *at }.is_null() {
            let hint = unsafe { *at } as *const c_void;
            at = unsafe { at.add(1) };
            let Some(name) = get(hint, c"NAME") else {
                continue;
            };
            // a device that only plays is not a microphone
            if get(hint, c"IOID").is_some_and(|io| io == "Output") {
                continue;
            }
            // the ones a person would pick: the sound servers and each card through the plug layer (which converts)
            let wanted = ["pipewire", "pulse"].contains(&name.as_str())
                || name.starts_with("sysdefault:")
                || name.starts_with("plughw:");
            if !wanted {
                continue;
            }
            let desc = get(hint, c"DESC").unwrap_or_default();
            let desc = desc
                .lines()
                .filter(|l| !l.trim().is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            found.push(Microphone {
                name: if desc.is_empty() { name.clone() } else { desc },
                id: name,
            });
        }
        // SAFETY: the list came from snd_device_name_hint
        unsafe { (alsa.free_hint)(hints) };
        found
    }
}

#[cfg(all(feature = "voice", any(windows, target_os = "macos")))]
mod platform {
    //! cpal: WASAPI on Windows, CoreAudio on macOS. Its stream is made and kept on the recording's thread (it may not
    //! be sent between threads), which says whether it started before the window goes on.

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use super::*;

    fn device(host: &cpal::Host, microphone: Option<&str>) -> Result<cpal::Device, MicError> {
        match microphone {
            Some(id) => {
                let wanted: cpal::DeviceId = id.parse().map_err(|_| {
                    MicError::NoMicrophone(format!("{id} is not the id of a microphone"))
                })?;
                host.device_by_id(&wanted).ok_or_else(|| {
                    MicError::NoMicrophone(format!("the microphone chosen ({id}) is not there"))
                })
            }
            None => host
                .default_input_device()
                .ok_or_else(|| MicError::NoMicrophone("this computer has no microphone".into())),
        }
    }

    fn mic_error(e: &cpal::Error) -> MicError {
        let said = e.to_string();
        match e.kind() {
            cpal::ErrorKind::PermissionDenied => MicError::Blocked(said),
            cpal::ErrorKind::DeviceNotAvailable => MicError::NoMicrophone(said),
            cpal::ErrorKind::HostUnavailable => MicError::NoSoundSystem(said),
            _ if said.contains("0x80070005")
                || said.to_lowercase().contains("access is denied") =>
            {
                MicError::Blocked(said)
            }
            _ => MicError::Failed(said),
        }
    }

    fn stream<T>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        shared: Arc<Shared>,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        let mut resampler = Resampler::new(config.sample_rate, config.channels as usize);
        let errors = Arc::clone(&shared);
        device.build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _| {
                let floats: Vec<f32> = data
                    .iter()
                    .map(|s| <f32 as cpal::FromSample<T>>::from_sample_(*s))
                    .collect();
                let mut out = Vec::with_capacity(floats.len() / 2);
                resampler.push(&floats, &mut out);
                shared.give(&out);
            },
            move |e| errors.fail(mic_error(&e)),
            Some(Duration::from_secs(3)),
        )
    }

    pub fn start(
        microphone: Option<&str>,
        shared: Arc<Shared>,
        stop: Arc<AtomicBool>,
    ) -> Result<JoinHandle<()>, MicError> {
        let microphone = microphone.map(str::to_string);
        let (said, started) = std::sync::mpsc::channel::<Result<(), MicError>>();
        let worker = std::thread::spawn(move || {
            let host = cpal::default_host();
            let made = device(&host, microphone.as_deref()).and_then(|device| {
                let supported = device.default_input_config().map_err(|e| mic_error(&e))?;
                let config = supported.config();
                let s = match supported.sample_format() {
                    cpal::SampleFormat::F32 => stream::<f32>(&device, config, Arc::clone(&shared)),
                    cpal::SampleFormat::I16 => stream::<i16>(&device, config, Arc::clone(&shared)),
                    cpal::SampleFormat::U16 => stream::<u16>(&device, config, Arc::clone(&shared)),
                    cpal::SampleFormat::I32 => stream::<i32>(&device, config, Arc::clone(&shared)),
                    other => {
                        return Err(MicError::Failed(format!(
                        "the microphone gives samples of a kind QuickAI does not read ({other:?})"
                    )))
                    }
                }
                .map_err(|e| mic_error(&e))?;
                s.play().map_err(|e| mic_error(&e))?;
                Ok(s)
            });
            match made {
                Ok(s) => {
                    let _ = said.send(Ok(()));
                    while !stop.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(30));
                    }
                    drop(s);
                }
                Err(e) => {
                    let _ = said.send(Err(e));
                }
            }
        });
        match started.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(worker),
            Ok(Err(e)) => {
                let _ = worker.join();
                Err(e)
            }
            Err(_) => Err(MicError::Failed(
                "the microphone did not start in 5 seconds".into(),
            )),
        }
    }

    pub fn microphones() -> Vec<Microphone> {
        let host = cpal::default_host();
        let Ok(devices) = host.input_devices() else {
            return Vec::new();
        };
        devices
            .filter_map(|d| {
                let id = d.id().ok()?.to_string();
                Some(Microphone {
                    name: d.to_string(),
                    id,
                })
            })
            .collect()
    }
}

#[cfg(not(any(
    target_os = "linux",
    all(feature = "voice", any(windows, target_os = "macos"))
)))]
mod platform {
    use super::*;

    pub fn start(
        _: Option<&str>,
        _: Arc<Shared>,
        _: Arc<AtomicBool>,
    ) -> Result<JoinHandle<()>, MicError> {
        Err(MicError::NoSoundSystem(
            "this build of QuickAI has no microphone".into(),
        ))
    }

    pub fn microphones() -> Vec<Microphone> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_of_another_rate_and_two_channels_come_out_at_16_khz_mono() {
        // a second of a 440 Hz tone at 48 kHz in stereo, given in odd pieces
        let tone: Vec<f32> = (0..48_000)
            .flat_map(|i| {
                let s = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
                [s, s]
            })
            .collect();
        let mut r = Resampler::new(48_000, 2);
        let mut out = Vec::new();
        for piece in tone.chunks(1234) {
            r.push(piece, &mut out);
        }
        assert!((out.len() as i64 - 16_000).abs() <= 2, "{}", out.len());
        // still a tone of about the same loudness (the box filter takes a little off)
        let rms = (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt();
        assert!((0.3..0.36).contains(&rms), "{rms}");
        // 16 kHz mono goes through as it is
        let mut same = Resampler::new(16_000, 1);
        let mut out = Vec::new();
        same.push(&[0.1, 0.2, 0.3], &mut out);
        assert_eq!(out, [0.1, 0.2, 0.3]);
    }

    #[test]
    fn a_wav_file_is_read_as_16_khz_mono() {
        let path = std::env::temp_dir().join(format!("quickai-mic-{}.wav", std::process::id()));
        let pcm: Vec<i16> = (0..8_000).map(|i| ((i % 100) as i16 - 50) * 100).collect();
        let mut wav = Vec::new();
        let data_len = (pcm.len() * 2) as u32;
        wav.extend(b"RIFF");
        wav.extend((36 + data_len).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8_000u32.to_le_bytes());
        wav.extend(16_000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        for s in &pcm {
            wav.extend(s.to_le_bytes());
        }
        std::fs::write(&path, wav).unwrap();
        let samples = fake::read_wav(&path).unwrap();
        // a second at 8 kHz is a second at 16 kHz
        assert!(
            (samples.len() as i64 - 16_000).abs() <= 2,
            "{}",
            samples.len()
        );
        assert!(fake::read_wav(&std::env::temp_dir()).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
