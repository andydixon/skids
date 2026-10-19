//! Procedural sound: an engine note per human player, tyre squeal and knocks.
//!
//! The retail samples are on disk 1, which is not in this dump, so everything
//! is synthesised. Parameters are passed to the audio thread through atomics.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub const VOICES: usize = 2;

#[derive(Default)]
struct VoiceParams {
    rpm: AtomicU32,
    skid: AtomicU32,
    bump: AtomicU32,
    active: AtomicU32,
}

#[derive(Default)]
pub struct Shared {
    voices: [VoiceParams; VOICES],
    master: AtomicU32,
}

fn store(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

pub struct Audio {
    shared: Arc<Shared>,
    #[cfg(feature = "audio")]
    _stream: Option<cpal::Stream>,
}

impl Audio {
    pub fn new(enabled: bool) -> Audio {
        let shared = Arc::new(Shared::default());
        store(&shared.master, 0.5);
        #[cfg(feature = "audio")]
        {
            let stream = if enabled {
                backend::open(shared.clone())
            } else {
                None
            };
            Audio {
                shared,
                _stream: stream,
            }
        }
        #[cfg(not(feature = "audio"))]
        {
            let _ = enabled;
            Audio { shared }
        }
    }

    pub fn set_voice(&self, i: usize, active: bool, rpm: f32, skid: f32, bump: f32) {
        let v = &self.shared.voices[i];
        store(&v.active, if active { 1.0 } else { 0.0 });
        store(&v.rpm, rpm);
        store(&v.skid, skid);
        // Knocks are latched until the audio thread consumes them.
        if bump > load(&v.bump) {
            store(&v.bump, bump);
        }
    }

    pub fn silence(&self) {
        for i in 0..VOICES {
            self.set_voice(i, false, 0.0, 0.0, 0.0);
        }
    }
}

/// Synth state for one voice, run on the audio thread.
#[derive(Default)]
struct Synth {
    phase: f32,
    sub_phase: f32,
    rpm: f32,
    skid: f32,
    knock: f32,
    noise: u32,
    lp: f32,
    skid_lp: f32,
    level: f32,
}

impl Synth {
    fn sample(&mut self, p: &VoiceParams, rate: f32) -> f32 {
        let target_level = load(&p.active);
        self.level += (target_level - self.level) * 0.0008;
        self.rpm += (load(&p.rpm) - self.rpm) * 0.002;
        self.skid += (load(&p.skid) - self.skid) * 0.004;
        let b = load(&p.bump);
        if b > 0.0 {
            self.knock = self.knock.max(b);
            store(&p.bump, 0.0);
        }

        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        let white = (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0;

        // Engine: a buzzy pulse with a sub-octave, low-passed.
        let freq = 38.0 + self.rpm * 150.0;
        self.phase = (self.phase + freq / rate).fract();
        self.sub_phase = (self.sub_phase + freq * 0.5 / rate).fract();
        let pulse = if self.phase < 0.3 { 1.0 } else { -0.45 };
        let sub = if self.sub_phase < 0.5 { 0.5 } else { -0.5 };
        let raw = pulse + sub + white * 0.15;
        self.lp += (raw - self.lp) * (0.06 + self.rpm * 0.12);
        let engine = self.lp * 0.32;

        // Tyres: band-limited noise.
        self.skid_lp += (white - self.skid_lp) * 0.35;
        let squeal = (white - self.skid_lp) * self.skid.min(1.0) * 0.22;

        let knock = white * self.knock * 0.6;
        self.knock *= 0.9985;

        (engine + squeal + knock) * self.level
    }
}

#[cfg(feature = "audio")]
mod backend {
    use super::{load, Shared, Synth, VOICES};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SizedSample};
    use std::sync::Arc;

    pub fn open(shared: Arc<Shared>) -> Option<cpal::Stream> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let config = device.default_output_config().ok()?;
        let fmt = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let stream = match fmt {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, shared),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, shared),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, shared),
            _ => None,
        }?;
        stream.play().ok()?;
        Some(stream)
    }

    fn build<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        shared: Arc<Shared>,
    ) -> Option<cpal::Stream>
    where
        T: SizedSample + FromSample<f32>,
    {
        let rate = config.sample_rate.0 as f32;
        let channels = config.channels as usize;
        let mut synths: [Synth; VOICES] = Default::default();
        for (i, s) in synths.iter_mut().enumerate() {
            s.noise = 0x1234_5678 ^ (i as u32 * 0x9e37_79b9);
        }
        device
            .build_output_stream(
                config,
                move |data: &mut [T], _| {
                    let master = load(&shared.master);
                    for frame in data.chunks_mut(channels) {
                        let mut l = 0.0;
                        let mut r = 0.0;
                        for (i, s) in synths.iter_mut().enumerate() {
                            let v = s.sample(&shared.voices[i], rate);
                            // Player one leans left, player two right.
                            let pan = if i == 0 { 0.35 } else { 0.65 };
                            l += v * (1.0 - pan) * 2.0;
                            r += v * pan * 2.0;
                        }
                        let (l, r) = ((l * master).clamp(-1.0, 1.0), (r * master).clamp(-1.0, 1.0));
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
