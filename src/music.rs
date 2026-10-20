//! OctaMED song playback, mixed in software.
//!
//! Implements what the game's tune uses: sample instruments with loops,
//! slides (1, 2), portamento (3), speed (9), position jump (B), volume (C),
//! volume slide (D) and the F group (break, tempo, retrigger, note delay,
//! set pitch, stop, note off). Other commands are ignored.

use crate::formats::med::{Note, Song};
use std::sync::Arc;

const PAULA_CLOCK: f64 = 3_546_895.0;
/// Period of note 1 (C-1), as on ProTracker.
const C1_PERIOD: f64 = 856.0;

#[derive(Clone, Default)]
struct Channel {
    instr: Option<usize>,
    pos: f64,
    period: f64,
    target: f64,
    porta_speed: f64,
    volume: f64,
    playing: bool,
    /// Note to start later this line (F2 delay) and retrigger interval (F1/F3).
    delayed: Option<(Note, u32)>,
    retrigger: Option<u32>,
}

pub struct Player {
    song: Arc<Song>,
    rate: f64,
    order: usize,
    line: usize,
    tick: u32,
    speed: u32,
    tempo: u16,
    tick_left: f64,
    jump: Option<(usize, usize)>,
    channels: Vec<Channel>,
}

fn period_for(note: i32) -> f64 {
    C1_PERIOD * 2f64.powf(-(note - 1) as f64 / 12.0)
}

impl Player {
    pub fn new(song: Arc<Song>, rate: f64) -> Player {
        let tracks = song.blocks.iter().map(|b| b.tracks).max().unwrap_or(4);
        let speed = song.speed as u32;
        let tempo = song.tempo;
        Player {
            song,
            rate,
            order: 0,
            line: 0,
            tick: 0,
            speed,
            tempo,
            tick_left: 0.0,
            jump: None,
            channels: vec![Channel::default(); tracks],
        }
    }

    fn ticks_per_second(&self) -> f64 {
        let s = &self.song;
        let bpm = if s.bpm_mode {
            self.tempo as f64 * s.lines_per_beat as f64 / 4.0
        } else if self.tempo <= 10 {
            // SoundTracker-compatible tempo: a plain 50 Hz tick.
            125.0
        } else {
            self.tempo as f64 * 125.0 / 33.0
        };
        (bpm * 2.0 / 5.0).max(1.0)
    }

    fn trigger(&mut self, ch: usize, n: Note) {
        let song = self.song.clone();
        let c = &mut self.channels[ch];
        if n.instr > 0 {
            let i = n.instr as usize - 1;
            if i < song.instruments.len() {
                c.instr = Some(i);
                c.volume = song.instruments[i].volume as f64;
            }
        }
        if n.note > 0 {
            let transpose =
                c.instr.map_or(0, |i| song.instruments[i].transpose as i32) + song.transpose as i32;
            let p = period_for(n.note as i32 + transpose);
            if n.cmd == 3 && c.playing {
                c.target = p;
                if n.param > 0 {
                    c.porta_speed = n.param as f64;
                }
            } else {
                c.period = p;
                c.target = p;
                c.pos = 0.0;
                c.playing = c.instr.is_some();
            }
        }
    }

    fn start_line(&mut self) {
        let song = self.song.clone();
        let block = &song.blocks[song.sequence[self.order]];
        for ch in 0..block.tracks.min(self.channels.len()) {
            let n = block.notes[self.line * block.tracks + ch];
            let c = &mut self.channels[ch];
            c.retrigger = None;
            c.delayed = None;
            match (n.cmd, n.param) {
                (0x0f, 0xf1) => c.retrigger = Some((self.speed / 2).max(1)),
                (0x0f, 0xf3) => c.retrigger = Some((self.speed / 3).max(1)),
                (0x0f, 0xf2) => {
                    c.delayed = Some((n, (self.speed / 2).max(1)));
                    continue;
                }
                _ => {}
            }
            if n.cmd == 0x0f && n.param == 0xfd && n.note > 0 {
                // Set pitch without restarting the sample.
                let transpose = c.instr.map_or(0, |i| song.instruments[i].transpose as i32)
                    + song.transpose as i32;
                c.period = period_for(n.note as i32 + transpose);
                c.target = c.period;
            } else {
                self.trigger(ch, n);
            }
            let c = &mut self.channels[ch];
            match n.cmd {
                0x09 if n.param > 0 => self.speed = (n.param as u32).min(32),
                0x0b => self.jump = Some((n.param as usize, 0)),
                0x0c => {
                    let v = if self.song.volume_hex {
                        n.param as u32
                    } else {
                        (n.param >> 4) as u32 * 10 + (n.param & 15) as u32
                    };
                    c.volume = v.min(64) as f64;
                }
                0x0f => match n.param {
                    0x00 => self.jump = Some((self.order + 1, 0)),
                    0x01..=0xf0 => self.tempo = n.param as u16,
                    0xfe => self.jump = Some((0, 0)),
                    0xff => c.playing = false,
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn effects_tick(&mut self) {
        let song = self.song.clone();
        let block = &song.blocks[song.sequence[self.order]];
        for ch in 0..block.tracks.min(self.channels.len()) {
            let n = block.notes[self.line * block.tracks + ch];
            if let Some((note, at)) = self.channels[ch].delayed {
                if self.tick == at {
                    self.trigger(ch, note);
                    self.channels[ch].delayed = None;
                }
            }
            let c = &mut self.channels[ch];
            if let Some(every) = c.retrigger {
                if self.tick.is_multiple_of(every) {
                    c.pos = 0.0;
                }
            }
            match n.cmd {
                0x01 => c.period = (c.period - n.param as f64).max(57.0),
                0x02 => c.period = (c.period + n.param as f64).min(3424.0),
                0x03 => {
                    let s = c.porta_speed;
                    c.period = if c.period > c.target {
                        (c.period - s).max(c.target)
                    } else {
                        (c.period + s).min(c.target)
                    };
                }
                0x0d => {
                    let (up, down) = ((n.param >> 4) as f64, (n.param & 15) as f64);
                    c.volume = if up > 0.0 {
                        (c.volume + up).min(64.0)
                    } else {
                        (c.volume - down).max(0.0)
                    };
                }
                _ => {}
            }
        }
    }

    fn advance_line(&mut self) {
        let len = self.song.sequence.len();
        if let Some((order, line)) = self.jump.take() {
            self.order = if order < len { order } else { 0 };
            self.line = line;
            return;
        }
        self.line += 1;
        if self.line >= self.song.blocks[self.song.sequence[self.order]].lines {
            self.line = 0;
            self.order = (self.order + 1) % len;
        }
    }

    fn tick(&mut self) {
        if self.tick == 0 {
            self.start_line();
        } else {
            self.effects_tick();
        }
        self.tick += 1;
        if self.tick >= self.speed {
            self.tick = 0;
            self.advance_line();
        }
    }

    pub fn order(&self) -> usize {
        self.order
    }

    /// Current (sequence position, line), for diagnostics.
    pub fn position(&self) -> String {
        format!("{}:{}", self.order, self.line)
    }

    /// Mixes one stereo frame.
    pub fn next(&mut self) -> (f32, f32) {
        if self.tick_left <= 0.0 {
            self.tick();
            self.tick_left += self.rate / self.ticks_per_second();
        }
        self.tick_left -= 1.0;

        let song = &self.song;
        let master = song.master_volume.min(64) as f64 / 64.0;
        let (mut l, mut r) = (0.0f64, 0.0f64);
        for (ch, c) in self.channels.iter_mut().enumerate() {
            if !c.playing {
                continue;
            }
            let Some(inst) = c.instr.map(|i| &song.instruments[i]) else {
                continue;
            };
            let len = inst.data.len();
            if len == 0 {
                c.playing = false;
                continue;
            }
            let i = c.pos as usize;
            if i >= len {
                c.playing = false;
                continue;
            }
            let v = inst.data[i] as f64 / 128.0
                * (c.volume / 64.0)
                * (song.track_volume[ch % 16].min(64) as f64 / 64.0);
            // Amiga hardware panning: channels 0 and 3 left, 1 and 2 right.
            if matches!(ch % 4, 0 | 3) {
                l += v;
            } else {
                r += v;
            }
            c.pos += PAULA_CLOCK / c.period / self.rate;
            if inst.loop_len > 2 {
                let end = (inst.loop_start + inst.loop_len) as f64;
                while c.pos >= end {
                    c.pos -= inst.loop_len as f64;
                }
            }
        }
        // Blend a little of each side into the other, as hard Amiga stereo
        // is tiring on headphones.
        let (l, r) = (l * 0.75 + r * 0.25, r * 0.75 + l * 0.25);
        ((l * master * 0.5) as f32, (r * master * 0.5) as f32)
    }
}
