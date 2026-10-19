//! Sound: the game's own samples and tune, mixed in software.
//!
//! Each human player has an engine voice (the `motor3` loop, pitched with
//! revs) and a tyre voice (`screech2`). One-shots cover crashes, backfires,
//! the horn and the start beeps; the OctaMED tune plays on top. When a
//! sample is missing, a simple synthesised stand-in is used instead.

use crate::formats::iff::Sound;
use crate::formats::med::Song;
use crate::music::Player;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub const VOICES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sfx {
    /// Countdown tick.
    Beep,
    /// Lights out.
    Go,
    Horn,
    Backfire,
    /// Strength 0..1.
    Crash(f32),
}

/// The samples the game uses, picked out of the sound assets by name.
#[derive(Default, Clone)]
pub struct Bank {
    engine: Option<Arc<Sound>>,
    skid: Option<Arc<Sound>>,
    crashes: Vec<Arc<Sound>>,
    beep: Option<Arc<Sound>>,
    go: Option<Arc<Sound>>,
    horn: Option<Arc<Sound>>,
    backfire: Option<Arc<Sound>>,
}

impl Bank {
    pub fn from_sounds(sounds: &BTreeMap<String, Sound>) -> Bank {
        let get = |names: &[&str]| {
            names
                .iter()
                .find_map(|n| sounds.get(*n))
                .map(|s| Arc::new(s.clone()))
        };
        Bank {
            engine: get(&["motor3", "motor", "engine"]),
            skid: get(&["screech2", "screech3", "screech"]),
            crashes: ["crash1", "crash2", "crash3"]
                .iter()
                .filter_map(|n| sounds.get(*n))
                .map(|s| Arc::new(s.clone()))
                .collect(),
            beep: get(&["beep"]),
            go: get(&["beep2", "beep"]),
            horn: get(&["horn"]),
            backfire: get(&["backfire"]),
        }
    }
}

/// A sample playing from `pos`, `step` source frames per output frame.
struct Play {
    sound: Arc<Sound>,
    pos: f64,
    step: f64,
    volume: f32,
    pan: f32,
    /// Stop after this many output frames (0 = play to the end).
    frames_left: usize,
}

impl Play {
    fn new(sound: Arc<Sound>, rate: f32, volume: f32, pan: f32) -> Play {
        let step = sound.rate as f64 / rate as f64;
        Play {
            sound,
            pos: 0.0,
            step,
            volume,
            pan,
            frames_left: 0,
        }
    }

    /// Next sample value, or None when finished.
    fn next(&mut self, pitch: f64) -> Option<f32> {
        let s = &self.sound;
        let i = self.pos as usize;
        if i >= s.data.len() {
            return None;
        }
        let v = s.data[i] as f32 / 128.0 * s.volume;
        self.pos += self.step * pitch;
        if self.pos as usize >= s.data.len() {
            if let Some(start) = s.loop_start {
                let len = (s.data.len() - start).max(1) as f64;
                while self.pos as usize >= s.data.len() {
                    self.pos -= len;
                }
            }
        }
        Some(v)
    }
}

#[derive(Default)]
struct Voice {
    active: bool,
    rpm: f32,
    skid: f32,
    level: f32,
    engine: Option<Play>,
    tyres: Option<Play>,
    synth: Synth,
}

struct Mixer {
    rate: f32,
    bank: Bank,
    voices: [Voice; VOICES],
    oneshots: Vec<Play>,
    music: Option<Player>,
    song: Option<Arc<Song>>,
    music_on: bool,
    music_volume: f32,
    noise: u32,
}

impl Mixer {
    fn frame(&mut self) -> (f32, f32) {
        let (mut l, mut r) = (0.0f32, 0.0f32);
        let rate = self.rate;
        for (i, v) in self.voices.iter_mut().enumerate() {
            let target = if v.active { 1.0 } else { 0.0 };
            v.level += (target - v.level) * 0.0008;
            if v.level < 1e-4 {
                continue;
            }
            let pan = if i == 0 { 0.35 } else { 0.65 };
            let mut out = 0.0;
            match (&mut v.engine, &self.bank.engine) {
                (Some(p), _) => out += p.next(0.55 + v.rpm as f64 * 1.35).unwrap_or(0.0) * 0.55,
                (None, Some(s)) => v.engine = Some(Play::new(s.clone(), rate, 1.0, pan)),
                (None, None) => out += v.synth.engine(v.rpm, rate),
            }
            match (&mut v.tyres, &self.bank.skid) {
                (Some(p), _) => {
                    out += p.next(0.9 + v.skid as f64 * 0.2).unwrap_or(0.0) * v.skid.min(1.0) * 0.45
                }
                (None, Some(s)) => v.tyres = Some(Play::new(s.clone(), rate, 1.0, pan)),
                (None, None) => out += v.synth.squeal(v.skid),
            }
            out *= v.level;
            l += out * (1.0 - pan) * 2.0;
            r += out * pan * 2.0;
        }
        self.oneshots.retain_mut(|p| {
            if p.frames_left == 1 {
                return false;
            }
            p.frames_left = p.frames_left.saturating_sub(1);
            match p.next(1.0) {
                Some(v) => {
                    l += v * p.volume * (1.0 - p.pan) * 2.0;
                    r += v * p.volume * p.pan * 2.0;
                    true
                }
                None => false,
            }
        });
        if self.music_on {
            if let Some(m) = &mut self.music {
                let (ml, mr) = m.next();
                l += ml * self.music_volume;
                r += mr * self.music_volume;
            }
        }
        (l * 0.5, r * 0.5)
    }

    fn rand(&mut self) -> u32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        self.noise
    }

    fn play(&mut self, sfx: Sfx, pan: f32) {
        let (sound, volume, max_secs) = match sfx {
            Sfx::Beep => (self.bank.beep.clone(), 0.6, 0.35),
            Sfx::Go => (self.bank.go.clone(), 0.7, 0.8),
            Sfx::Horn => (self.bank.horn.clone(), 0.8, 0.6),
            Sfx::Backfire => (self.bank.backfire.clone(), 0.7, 0.0),
            Sfx::Crash(strength) => {
                let n = self.bank.crashes.len();
                let r = self.rand() as usize;
                let pick = if n > 0 {
                    Some(self.bank.crashes[r % n].clone())
                } else {
                    None
                };
                (pick, 0.3 + 0.7 * strength.clamp(0.0, 1.0), 0.0)
            }
        };
        let Some(sound) = sound else { return };
        // A crash pile-up should not stack dozens of copies.
        if self.oneshots.len() >= 12 {
            self.oneshots.remove(0);
        }
        let mut p = Play::new(sound, self.rate, volume, pan);
        // Looped samples (beeps, horn) are cut off after a fixed time.
        p.frames_left = if max_secs > 0.0 {
            (max_secs * self.rate) as usize + 1
        } else {
            0
        };
        self.oneshots.push(p);
    }
}

pub struct Audio {
    mixer: Arc<Mutex<Mixer>>,
    #[cfg(feature = "audio")]
    _stream: Option<cpal::Stream>,
}

impl Audio {
    pub fn new(enabled: bool, bank: Bank, song: Option<Arc<Song>>) -> Audio {
        let mixer = Arc::new(Mutex::new(Mixer {
            rate: 44100.0,
            bank,
            voices: Default::default(),
            oneshots: Vec::new(),
            music: None,
            song,
            music_on: false,
            music_volume: 0.7,
            noise: 0x2468_ace1,
        }));
        #[cfg(feature = "audio")]
        {
            let stream = if enabled {
                backend::open(mixer.clone())
            } else {
                None
            };
            Audio {
                mixer,
                _stream: stream,
            }
        }
        #[cfg(not(feature = "audio"))]
        {
            let _ = enabled;
            Audio { mixer }
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Mixer) -> R) -> R {
        let mut m = self.mixer.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut m)
    }

    pub fn set_voice(&self, i: usize, active: bool, rpm: f32, skid: f32) {
        self.with(|m| {
            let v = &mut m.voices[i];
            v.active = active;
            v.rpm = rpm;
            v.skid = skid;
        });
    }

    pub fn play(&self, sfx: Sfx, pan: f32) {
        self.with(|m| m.play(sfx, pan.clamp(0.0, 1.0)));
    }

    /// Starts the tune from the top, or stops it.
    pub fn set_music(&self, on: bool) {
        self.with(|m| {
            if on && !m.music_on {
                m.music = m.song.clone().map(|s| Player::new(s, m.rate as f64));
            }
            m.music_on = on;
        });
    }

    pub fn has_music(&self) -> bool {
        self.with(|m| m.song.is_some())
    }

    pub fn silence(&self) {
        for i in 0..VOICES {
            self.set_voice(i, false, 0.0, 0.0);
        }
    }

    /// Renders `frames` stereo frames without a sound device (for tests).
    #[cfg(test)]
    fn render(&self, frames: usize) -> Vec<(f32, f32)> {
        self.with(|m| (0..frames).map(|_| m.frame()).collect())
    }
}

/// Stand-in sounds for when the samples are not available.
#[derive(Default)]
struct Synth {
    phase: f32,
    sub_phase: f32,
    lp: f32,
    noise: u32,
    skid_lp: f32,
}

impl Synth {
    fn white(&mut self) -> f32 {
        self.noise = self
            .noise
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.noise >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }

    fn engine(&mut self, rpm: f32, rate: f32) -> f32 {
        let freq = 38.0 + rpm * 150.0;
        self.phase = (self.phase + freq / rate).fract();
        self.sub_phase = (self.sub_phase + freq * 0.5 / rate).fract();
        let pulse = if self.phase < 0.3 { 1.0 } else { -0.45 };
        let sub = if self.sub_phase < 0.5 { 0.5 } else { -0.5 };
        let raw = pulse + sub + self.white() * 0.15;
        self.lp += (raw - self.lp) * (0.06 + rpm * 0.12);
        self.lp * 0.32
    }

    fn squeal(&mut self, skid: f32) -> f32 {
        let w = self.white();
        self.skid_lp += (w - self.skid_lp) * 0.35;
        (w - self.skid_lp) * skid.min(1.0) * 0.22
    }
}

#[cfg(feature = "audio")]
mod backend {
    use super::Mixer;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SizedSample};
    use std::sync::{Arc, Mutex};

    pub fn open(mixer: Arc<Mutex<Mixer>>) -> Option<cpal::Stream> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let config = device.default_output_config().ok()?;
        let fmt = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        mixer.lock().ok()?.rate = config.sample_rate.0 as f32;
        let stream = match fmt {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, mixer),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, mixer),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, mixer),
            _ => None,
        }?;
        stream.play().ok()?;
        Some(stream)
    }

    fn build<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        mixer: Arc<Mutex<Mixer>>,
    ) -> Option<cpal::Stream>
    where
        T: SizedSample + FromSample<f32>,
    {
        let channels = config.channels as usize;
        device
            .build_output_stream(
                config,
                move |data: &mut [T], _| {
                    let mut m = mixer.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in data.chunks_mut(channels) {
                        let (l, r) = m.frame();
                        let (l, r) = (l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0));
                        for (c, out) in frame.iter_mut().enumerate() {
                            let v = if channels == 1 {
                                (l + r) * 0.5
                            } else if c % 2 == 0 {
                                l
                            } else {
                                r
                            };
                            *out = T::from_sample(v);
                        }
                    }
                },
                |err| eprintln!("audio stream error: {err}"),
                None,
            )
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, looped: bool) -> Sound {
        let data = (0..400)
            .map(|i| ((i as f32 * 0.3).sin() * 100.0) as i8)
            .collect();
        Sound {
            rate,
            data,
            loop_start: looped.then_some(0),
            volume: 1.0,
        }
    }

    #[test]
    fn samples_mix_and_one_shots_finish() {
        let mut sounds = BTreeMap::new();
        sounds.insert("motor3".to_string(), tone(13714, true));
        sounds.insert("crash1".to_string(), tone(8363, false));
        let a = Audio::new(false, Bank::from_sounds(&sounds), None);
        a.set_voice(0, true, 0.5, 0.0);
        a.play(Sfx::Crash(1.0), 0.5);
        let out = a.render(44100);
        assert!(out.iter().any(|f| f.0.abs() > 0.01), "engine is audible");
        // A 400-sample crash at 8363 Hz lasts about 2100 output frames.
        assert!(a.with(|m| m.oneshots.is_empty()));
    }
}
