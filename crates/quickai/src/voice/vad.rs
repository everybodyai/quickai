//! When someone is talking, and when they stopped: by how loud the microphone is, against how loud the room is when
//! nobody talks. Enough for one person at a computer; a noisy room asks for the button instead.

use super::RATE;

/// What a stretch of samples said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    Nothing,
    /// Someone began to talk.
    Began,
    /// They stopped for long enough: what they said is whole.
    Ended,
}

/// Listens to frames of 20 ms.
#[derive(Clone, Debug)]
pub struct Vad {
    /// How loud the room is (the root mean square of a frame), learnt as it goes.
    floor: f32,
    frames: u32,
    talking: bool,
    /// Frames in a row louder than the room, and quieter, while talking.
    loud: u32,
    quiet: u32,
    /// How many quiet frames end what was said.
    end_after: u32,
    /// The samples of a frame not yet whole.
    partial: Vec<f32>,
    /// How loud the last frame was, 0 to 1, for the bars.
    pub level: f32,
}

const FRAME: usize = (RATE / 50) as usize;
/// The quietest a voice is taken to be, whatever the room (a root mean square of samples of -1 to 1).
const LEAST_VOICE: f32 = 0.012;
/// Frames louder than the room that make talking: 140 ms.
const BEGIN_FRAMES: u32 = 7;

impl Vad {
    /// One that ends what was said after `silence_ms` of quiet.
    pub fn new(silence_ms: u32) -> Vad {
        Vad {
            floor: 0.0,
            frames: 0,
            talking: false,
            loud: 0,
            quiet: 0,
            end_after: (silence_ms / 20).max(1),
            partial: Vec::with_capacity(FRAME),
            level: 0.0,
        }
    }

    #[cfg(test)]
    pub fn talking(&self) -> bool {
        self.talking
    }

    /// Takes samples; what they said (the last thing, when they hold several frames).
    pub fn push(&mut self, samples: &[f32]) -> Heard {
        let mut heard = Heard::Nothing;
        for &s in samples {
            self.partial.push(s);
            if self.partial.len() == FRAME {
                let rms = (self.partial.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
                self.partial.clear();
                match self.frame(rms) {
                    Heard::Nothing => {}
                    h => heard = h,
                }
            }
        }
        heard
    }

    fn frame(&mut self, rms: f32) -> Heard {
        self.frames += 1;
        // the level of the bars: -60 dB to 0, as 0 to 1
        let db = 20.0 * rms.max(1e-6).log10();
        self.level = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
        // the room: the first quarter second as it is, then it falls at once and rises slowly, so that talking does
        // not raise it
        if self.frames <= 12 {
            self.floor = if self.frames == 1 {
                rms
            } else {
                self.floor.min(rms).max(self.floor * 0.5)
            };
        } else if !self.talking {
            self.floor = if rms < self.floor {
                rms
            } else {
                self.floor * 0.995 + rms * 0.005
            };
        }
        let begin = (self.floor * 3.0).max(LEAST_VOICE);
        let keep = (self.floor * 2.0).max(LEAST_VOICE * 0.6);
        if !self.talking {
            // nothing counts before the room is known
            if self.frames > 12 && rms > begin {
                self.loud += 1;
                if self.loud >= BEGIN_FRAMES {
                    self.talking = true;
                    self.quiet = 0;
                    return Heard::Began;
                }
            } else {
                self.loud = 0;
            }
            return Heard::Nothing;
        }
        if rms < keep {
            self.quiet += 1;
            if self.quiet >= self.end_after {
                self.talking = false;
                self.loud = 0;
                return Heard::Ended;
            }
        } else {
            self.quiet = 0;
        }
        Heard::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ms` of a tone as loud as `amplitude` (0 is silence with a little noise).
    fn sound(ms: u32, amplitude: f32) -> Vec<f32> {
        let n = (RATE * ms / 1000) as usize;
        (0..n)
            .map(|i| {
                let noise = ((i * 7919 % 101) as f32 / 101.0 - 0.5) * 0.004;
                amplitude * (i as f32 * 0.07).sin() + noise
            })
            .collect()
    }

    #[test]
    fn talking_begins_after_a_moment_of_voice_and_ends_after_the_silence_asked_for() {
        let mut vad = Vad::new(1000);
        assert_eq!(vad.push(&sound(500, 0.0)), Heard::Nothing, "the room");
        assert_eq!(
            vad.push(&sound(100, 0.2)),
            Heard::Nothing,
            "too short to be talking"
        );
        assert_eq!(vad.push(&sound(100, 0.2)), Heard::Began);
        assert!(vad.talking());
        assert!(vad.level > 0.5);
        assert_eq!(
            vad.push(&sound(600, 0.0)),
            Heard::Nothing,
            "a pause between words"
        );
        assert_eq!(vad.push(&sound(300, 0.2)), Heard::Nothing);
        assert_eq!(vad.push(&sound(1100, 0.0)), Heard::Ended);
        assert!(!vad.talking());
    }

    #[test]
    fn a_noisy_room_needs_a_louder_voice() {
        let mut vad = Vad::new(800);
        assert_eq!(
            vad.push(&sound(600, 0.03)),
            Heard::Nothing,
            "the hum of the room"
        );
        assert_eq!(
            vad.push(&sound(400, 0.04)),
            Heard::Nothing,
            "a little louder than the room is not talking"
        );
        assert_eq!(vad.push(&sound(400, 0.3)), Heard::Began);
    }
}
