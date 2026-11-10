//! Talking to QuickAI (C216): what the window needs to hear a person and to read answers aloud, apart from how it
//! draws it (`app/talk.rs`).
//!
//! - `capture`: the microphone, as 16 kHz mono samples. ALSA (loaded when first used) on Linux, cpal elsewhere, or a
//!   recording played in its place for tests (`QUICKAI_FAKE_MIC`).
//! - `vad`: when someone is talking, and when they stopped: hands-free and "send when I stop talking".
//! - `transcribe`: the speech program quickai-whisper (whisper.cpp), run beside the app (feature `voice`).
//! - `speak`: reading aloud with a voice the person installed (never a robotic one of the system's), a sentence at a
//!   time, so that the one being read can be shown.
//! - `spoken`: what a model wrote, made into what should be heard, and cut into sentences, also as it streams in.
//! - `settings`: the person's choices (`app.voice`), the speech models and the languages.
//! - `voices`: the voices to read aloud with, none of them shipped: packs to download, and the person's own.
//!
//! Nothing of it leaves the computer: the microphone's samples go to the speech program beside the app, and the
//! answers to the system's speech program.

// hearing (feature `voice`): the microphone, when someone talks, and whisper.cpp
#[cfg(feature = "voice")]
pub mod capture;
#[cfg(feature = "voice")]
pub mod transcribe;
#[cfg(feature = "voice")]
pub mod vad;
// reading aloud, which every build has
mod engine;
pub mod settings;
pub mod speak;
pub mod spoken;
pub mod voices;

/// The samples the microphone gives, a second of them.
#[cfg_attr(not(feature = "voice"), allow(dead_code))]
pub const RATE: u32 = 16_000;
