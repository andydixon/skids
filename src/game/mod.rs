//! Race rules: grid, countdown, gate-based lap counting and finishing order.

pub mod ai;
pub mod car;

use crate::data::track::{Gate, Track};
use crate::data::vehicle::Vehicle;
use ai::Ai;
use car::{Car, Controls};

pub const DT: f32 = 1.0 / 50.0;
const COUNTDOWN: f32 = 3.0;
/// Gates are extended by this fraction at each end when testing crossings,
/// so a car clipping the verge still counts.
const GATE_SLACK: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Driver {
    Human(usize),
    Cpu,
}

pub struct Entrant {
    pub car: Car,
    pub driver: Driver,
    pub ai: Ai,
    pub name: String,
    /// Gates crossed in order since the start.
    pub passed: usize,
    pub next_gate: usize,
    pub lap_start: f32,
    pub best_lap: Option<f32>,
    pub last_lap: Option<f32>,
    pub finished: Option<f32>,
    pub trailer: Option<Trailer>,
    /// Race time when this entrant last crossed a gate.
    pub progress_at: f32,
}

/// A caravan on a tow bar behind a car.
#[derive(Clone, Copy, Debug)]
pub struct Trailer {
    pub vehicle: usize,
    pub x: f32,
    pub y: f32,
    pub heading: f32,
}

/// Tow bar length from the car's hitch (`Spec::hitch`) to the trailer's
/// centre, in world units.
const TOW_BAR: f32 = 20.0;
const TRAILER_RADIUS: f32 = 10.0;
/// Seconds without crossing a gate before a computer car is rescued.
const RESCUE_AFTER: f32 = 8.0;

fn rescue(e: &mut Entrant, track: &Track) {
    let gates = &track.gates;
    let n = gates.len();
    let from = gates[(e.next_gate + n - 1) % n].mid();
    let to = gates[e.next_gate].mid();
    let heading = (to.1 - from.1).atan2(to.0 - from.0);
    let c = &mut e.car;
    c.x = from.0;
    c.y = from.1;
    c.heading = heading.rem_euclid(std::f32::consts::TAU);
    c.place(track);
    if let Some(t) = &mut e.trailer {
        let back = c.spec.hitch + TOW_BAR;
        t.x = from.0 - heading.cos() * back;
        t.y = from.1 - heading.sin() * back;
        t.heading = heading;
    }
}

impl Trailer {
    /// Drags the trailer after its car: it keeps its distance from the
    /// hitch and swings to point at it.
    fn follow(&mut self, car: &Car) {
        let (fx, fy) = car.forward();
        let (hx, hy) = (car.x - fx * car.spec.hitch, car.y - fy * car.spec.hitch);
        let (dx, dy) = (self.x - hx, self.y - hy);
        let d = dx.hypot(dy).max(1e-3);
        self.x = hx + dx / d * TOW_BAR;
        self.y = hy + dy / d * TOW_BAR;
        self.heading = (hy - self.y).atan2(hx - self.x);
    }
}

impl Entrant {
    pub fn laps_done(&self, gates: usize) -> usize {
        self.passed.saturating_sub(1) / gates
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Countdown,
    Racing,
    Done,
}

pub struct Race {
    pub track: Track,
    pub entrants: Vec<Entrant>,
    pub laps: usize,
    pub time: f32,
    pub phase: Phase,
    pub finish_order: Vec<usize>,
    /// Track image with skid marks burnt in, 0RGB.
    pub ground: Vec<u32>,
    pub particles: Vec<Particle>,
    done_timer: f32,
    rng: u32,
}

pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub life: f32,
    pub colour: u32,
}

pub struct EntrantSetup {
    pub vehicle: usize,
    pub driver: Driver,
    pub team: usize,
    pub name: String,
    /// Vehicle index of a caravan to tow, if any.
    pub trailer: Option<usize>,
    /// Handling preset (`Spec::with_preset`); 0 = the car's own.
    pub preset: usize,
}

impl Race {
    pub fn new(
        track: Track,
        vehicles: &[Vehicle],
        setup: &[EntrantSetup],
        laps: usize,
        seed: u32,
    ) -> Race {
        let gates = track.gates.clone();
        // Gate 0 is the start/finish line; the grid forms up behind it.
        let start_gate = 0;
        let path: Vec<(f32, f32)> = gates.iter().map(Gate::mid).collect();

        let mut rng = seed | 1;
        let mut entrants = Vec::new();
        for (i, s) in setup.iter().enumerate() {
            // Two-wide grid, rows 30 units apart, following the road back
            // from the line.
            let row = (i / 2) as f32;
            let side = if i % 2 == 0 { -1.0 } else { 1.0 };
            // Leave room for a caravan between rows when towing.
            let spec = vehicles[s.vehicle].spec.with_preset(s.preset);
            let spacing = if s.trailer.is_some() {
                30.0 + spec.hitch + TOW_BAR + TRAILER_RADIUS
            } else {
                30.0
            };
            let ((px, py), heading) = behind(&path, 18.0 + row * spacing);
            let (rx, ry) = (-heading.sin(), heading.cos());
            let (x, y) = (px + rx * side * 12.0, py + ry * side * 12.0);
            let mut car = Car::new(s.vehicle, spec, s.team, x, y, heading);
            car.computer = s.driver == Driver::Cpu;
            car.place(&track);
            let line = 0.3 + 0.4 * rand01(&mut rng);
            let pace = 0.86 + 0.12 * rand01(&mut rng);
            entrants.push(Entrant {
                car,
                driver: s.driver,
                ai: Ai::new(line, pace),
                name: s.name.clone(),
                passed: 0,
                next_gate: start_gate,
                lap_start: 0.0,
                best_lap: None,
                last_lap: None,
                finished: None,
                progress_at: 0.0,
                trailer: s.trailer.map(|t| {
                    let (fx, fy) = (heading.cos(), heading.sin());
                    let back = spec.hitch + TOW_BAR;
                    Trailer {
                        vehicle: t,
                        x: x - fx * back,
                        y: y - fy * back,
                        heading,
                    }
                }),
            });
        }
        let ground = track.rgb();
        Race {
            track,
            entrants,
            laps,
            time: 0.0,
            phase: Phase::Countdown,
            finish_order: Vec::new(),
            ground,
            particles: Vec::new(),
            done_timer: 0.0,
            rng,
        }
    }

    pub fn countdown(&self) -> Option<u32> {
        (self.phase == Phase::Countdown).then(|| (COUNTDOWN - self.time).ceil().max(1.0) as u32)
    }

    pub fn race_time(&self) -> f32 {
        (self.time - COUNTDOWN).max(0.0)
    }

    pub fn humans_finished(&self) -> bool {
        self.entrants
            .iter()
            .all(|e| e.driver == Driver::Cpu || e.finished.is_some())
    }

    pub fn step(&mut self, human: &[Controls]) {
        self.time += DT;
        if self.phase == Phase::Countdown && self.time >= COUNTDOWN {
            self.phase = Phase::Racing;
        }
        let racing = self.phase != Phase::Countdown;
        let rt = self.race_time();
        let n = self.track.gates.len();
        // Where every car started the frame: gate crossings are judged on
        // the whole frame's movement, including knocks from collisions.
        let starts: Vec<(f32, f32)> = self.entrants.iter().map(|e| (e.car.x, e.car.y)).collect();

        for e in self.entrants.iter_mut() {
            let controls = match (e.driver, e.finished) {
                (_, Some(_)) => e.ai.drive(&e.car, &self.track, DT, racing),
                (Driver::Human(p), None) if racing => human.get(p).copied().unwrap_or_default(),
                (Driver::Human(_), None) => Controls::default(),
                (Driver::Cpu, None) => e.ai.drive(&e.car, &self.track, DT, racing),
            };
            e.car.controls = controls;
            // Finished cars cruise round on autopilot at reduced pace.
            let pace = if e.finished.is_some() { 0.6 } else { 1.0 };
            e.car.step(&self.track, pace);
        }

        for i in 0..self.entrants.len() {
            for j in i + 1..self.entrants.len() {
                let (a, b) = self.entrants.split_at_mut(j);
                car::collide(&mut a[i].car, &mut b[0].car);
            }
        }
        // Caravans follow their cars and shove other cars out of the way.
        for i in 0..self.entrants.len() {
            let Some(mut t) = self.entrants[i].trailer else {
                continue;
            };
            t.follow(&self.entrants[i].car);
            for (j, other) in self.entrants.iter_mut().enumerate() {
                if j != i {
                    push_off(&mut other.car, t.x, t.y);
                }
            }
            self.entrants[i].trailer = Some(t);
        }

        for (e, &(px, py)) in self.entrants.iter_mut().zip(&starts) {
            // Gate crossing, allowing one gate to be skipped.
            for skip in 0..2 {
                let gi = (e.next_gate + skip) % n;
                if crosses(&self.track.gates[gi], (px, py), (e.car.x, e.car.y)) {
                    let before = e.passed;
                    e.passed += skip + 1;
                    e.progress_at = rt;
                    e.next_gate = (gi + 1) % n;
                    if before == 0 {
                        // First time over the line: the clock starts.
                        e.lap_start = rt;
                    } else if e.finished.is_none() && (e.passed - 1) / n > (before - 1) / n {
                        let lap = rt - e.lap_start;
                        e.lap_start = rt;
                        e.last_lap = Some(lap);
                        e.best_lap = Some(e.best_lap.map_or(lap, |b: f32| b.min(lap)));
                    }
                    break;
                }
            }
        }

        // A computer car that has made no progress for a while is put back
        // on the racing line at the last gate it crossed.
        if self.phase == Phase::Racing {
            for e in self.entrants.iter_mut() {
                if e.driver == Driver::Cpu
                    && e.finished.is_none()
                    && rt - e.progress_at > RESCUE_AFTER
                {
                    rescue(e, &self.track);
                    e.progress_at = rt;
                }
            }
        }

        for (i, e) in self.entrants.iter_mut().enumerate() {
            if e.finished.is_none() && e.passed > self.laps * n {
                e.finished = Some(rt);
                self.finish_order.push(i);
            }
        }

        self.effects();

        if self.phase == Phase::Racing && self.humans_finished() {
            self.done_timer += DT;
            if self.done_timer > 3.0 {
                self.phase = Phase::Done;
            }
        }
    }

    fn effects(&mut self) {
        let mut spawn = Vec::new();
        for e in &self.entrants {
            let c = &e.car;
            let speed = c.speed();
            if c.slip > 55.0 && speed > 40.0 && c.on_road && !c.airborne {
                let strength = ((c.slip - 55.0) / 120.0).clamp(0.08, 0.35);
                for (wx, wy) in c.rear_wheels(&self.track) {
                    burn(&mut self.ground, wx, wy, strength);
                }
            }
            if !c.on_road && speed > 50.0 {
                spawn.push((c.rear_wheels(&self.track)[0], c.track_colour(&self.track)));
            }
        }
        for ((x, y), colour) in spawn {
            let a = rand01(&mut self.rng) * std::f32::consts::TAU;
            self.particles.push(Particle {
                x,
                y,
                vx: a.cos() * 12.0,
                vy: a.sin() * 6.0 - 10.0,
                life: 0.5,
                colour,
            });
        }
        for p in self.particles.iter_mut() {
            p.x += p.vx * DT;
            p.y += p.vy * DT;
            p.life -= DT;
        }
        self.particles.retain(|p| p.life > 0.0);
    }

    /// Indices of entrants from first to last.
    pub fn standings(&self) -> Vec<usize> {
        let n = self.track.gates.len();
        let mut order = self.finish_order.clone();
        let mut rest: Vec<usize> = (0..self.entrants.len())
            .filter(|i| !order.contains(i))
            .collect();
        rest.sort_by(|&a, &b| {
            let (ea, eb) = (&self.entrants[a], &self.entrants[b]);
            eb.passed.cmp(&ea.passed).then_with(|| {
                let d = |e: &Entrant| {
                    let m = self.track.gates[e.next_gate % n].mid();
                    (m.0 - e.car.x).hypot(m.1 - e.car.y)
                };
                d(ea).total_cmp(&d(eb))
            })
        });
        order.extend(rest);
        order
    }
}

impl Car {
    fn track_colour(&self, track: &Track) -> u32 {
        let (sx, sy) = self.screen_pos();
        track.palette[track.pixel(sx, sy) as usize]
            .scale(1.15)
            .to_u32()
    }
}

/// Moves a car out of a caravan it has run into, killing the speed it had
/// into it.
fn push_off(c: &mut Car, tx: f32, ty: f32) {
    let (dx, dy) = (c.x - tx, c.y - ty);
    let d = dx.hypot(dy);
    let min = car::RADIUS + TRAILER_RADIUS;
    if d >= min || d < 1e-3 {
        return;
    }
    let (nx, ny) = (dx / d, dy / d);
    c.x += nx * (min - d);
    c.y += ny * (min - d);
    let vn = c.vx * nx + c.vy * ny;
    if vn < 0.0 {
        c.vx -= vn * nx * 1.3;
        c.vy -= vn * ny * 1.3;
        c.bump = c.bump.max((-vn / 150.0).min(1.0));
    }
}

fn burn(ground: &mut [u32], sx: f32, sy: f32, strength: f32) {
    use crate::data::track::{H, W};
    let (x, y) = (sx as isize, sy as isize);
    for dx in 0..2 {
        let (px, py) = (x + dx, y);
        if px < 0 || py < 0 || px >= W as isize || py >= H as isize {
            continue;
        }
        let p = &mut ground[py as usize * W + px as usize];
        let k = 1.0 - strength;
        let ch = |v: u32, s: u32| ((((v >> s) & 0xff) as f32 * k) as u32) << s;
        *p = ch(*p, 16) | ch(*p, 8) | ch(*p, 0);
    }
}

/// The point `dist` world units back along the gate path from gate 0, and
/// the direction of travel there.
fn behind(path: &[(f32, f32)], mut dist: f32) -> ((f32, f32), f32) {
    let n = path.len();
    let mut i = 0;
    for _ in 0..n {
        let (a, b) = (path[(i + n - 1) % n], path[i]);
        let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
        let heading = (b.1 - a.1).atan2(b.0 - a.0);
        if dist <= len {
            let t = dist / len;
            return ((b.0 + (a.0 - b.0) * t, b.1 + (a.1 - b.1) * t), heading);
        }
        dist -= len;
        i = (i + n - 1) % n;
    }
    let (a, b) = (path[n - 1], path[0]);
    (b, (b.1 - a.1).atan2(b.0 - a.0))
}

fn crosses(g: &Gate, p: (f32, f32), q: (f32, f32)) -> bool {
    let (dx, dy) = (g.b.0 - g.a.0, g.b.1 - g.a.1);
    let a = (g.a.0 - dx * GATE_SLACK, g.a.1 - dy * GATE_SLACK);
    let b = (g.b.0 + dx * GATE_SLACK, g.b.1 + dy * GATE_SLACK);
    let cross = |o: (f32, f32), u: (f32, f32), v: (f32, f32)| {
        (u.0 - o.0) * (v.1 - o.1) - (u.1 - o.1) * (v.0 - o.0)
    };
    let d1 = cross(a, b, p);
    let d2 = cross(a, b, q);
    let d3 = cross(p, q, a);
    let d4 = cross(p, q, b);
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

fn rand01(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_crossing() {
        let g = Gate {
            a: (0.0, 0.0),
            b: (0.0, 10.0),
        };
        assert!(crosses(&g, (-1.0, 5.0), (1.0, 5.0)));
        assert!(!crosses(&g, (1.0, 5.0), (2.0, 5.0)));
        // Within the slack beyond the gate end.
        assert!(crosses(&g, (-1.0, 12.0), (1.0, 12.0)));
    }

    #[test]
    fn grid_follows_path_backwards() {
        let path = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
        let ((x, y), h) = behind(&path, 30.0);
        assert!((x - 0.0).abs() < 1e-3 && (y - 30.0).abs() < 1e-3);
        assert!((h + std::f32::consts::FRAC_PI_2).abs() < 1e-3);
        let ((x, y), _) = behind(&path, 130.0);
        assert!((x - 30.0).abs() < 1e-3 && (y - 100.0).abs() < 1e-3);
    }

    #[test]
    fn walls_keep_cars_on_the_road() {
        let gates: Vec<Gate> = (0..12)
            .map(|i| {
                let a = i as f32 / 12.0 * std::f32::consts::TAU;
                let (c, s) = (a.cos(), a.sin());
                Gate {
                    a: (512.0 + c * 150.0, 512.0 + s * 150.0),
                    b: (512.0 + c * 210.0, 512.0 + s * 210.0),
                }
            })
            .collect();
        let track = Track {
            name: "ring".into(),
            set_name: String::new(),
            thumbnail: None,
            pixels: vec![8; crate::data::track::W * crate::data::track::H],
            palette: crate::data::palette::TRACK_DEFAULT,
            height: vec![0; 64 * 64],
            edges: crate::data::track::edges(&gates),
            gates,
            start: (0.0, 0.0),
            flags: 0,
        };
        let spec = crate::data::vehicle::Spec::from_set(&crate::data::vehicle::DEFAULT_SET, 1.0);
        let mut c = Car::new(0, spec, 0, 512.0 + 180.0, 512.0, 0.0);
        c.place(&track);
        // Flat out in a straight line: the outer wall turns the car back.
        c.controls.throttle = 1.0;
        for _ in 0..500 {
            c.step(&track, 1.0);
            let r = (c.x - 512.0).hypot(c.y - 512.0);
            assert!(r > 140.0 && r < 215.0, "left the road at radius {r}");
        }
    }
}
