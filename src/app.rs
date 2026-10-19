//! Front end: menu, race screens and results.

use crate::audio::Audio;
use crate::data::{palette, track, vehicle, GameData};
use crate::game::car::Controls;
use crate::game::{Driver, EntrantSetup, Phase, Race};
use crate::render::{self, Frame, Rect, SCREEN_H, SCREEN_W};
use minifb::Key;
use std::collections::HashMap;

const THUMB_W: usize = 128;
const THUMB_H: usize = 64;
const CPU_NAMES: [&str; 8] = [
    "ACE", "BLITZ", "DUSTY", "NITRO", "ROCKET", "SPUD", "TURBO", "ZIPPY",
];
const LAP_CHOICES: [usize; 5] = [1, 3, 5, 7, 10];

const WHITE: u32 = 0xffffff;
const YELLOW: u32 = 0xffd840;
const GREY: u32 = 0x9098a0;
const RED: u32 = 0xff5040;

#[derive(Default)]
pub struct Input {
    pub held: Vec<Key>,
    pub pressed: Vec<Key>,
}

impl Input {
    pub fn down(&self, k: Key) -> bool {
        self.held.contains(&k)
    }

    pub fn hit(&self, k: Key) -> bool {
        self.pressed.contains(&k)
    }

    fn controls(&self, up: Key, down: Key, left: Key, right: Key) -> Controls {
        let axis = |neg: Key, pos: Key| (self.down(pos) as i32 - self.down(neg) as i32) as f32;
        Controls {
            throttle: self.down(up) as i32 as f32,
            brake: self.down(down) as i32 as f32,
            steer: axis(left, right),
        }
    }
}

pub struct Settings {
    pub players: usize,
    pub track: usize,
    pub laps: usize,
    pub cars: [usize; 2],
    pub opponents: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Item {
    Start,
    Players,
    Track,
    Laps,
    Car1,
    Car2,
    Opponents,
    Quit,
}

enum Screen {
    Menu {
        cursor: usize,
    },
    Race {
        race: Box<Race>,
        cams: [(f32, f32); 2],
        paused: bool,
    },
    Results {
        race: Box<Race>,
    },
}

pub struct App {
    pub data: GameData,
    pub settings: Settings,
    screen: Screen,
    pub frame: Frame,
    audio: Audio,
    demo: Option<Box<Race>>,
    demo_track: usize,
    thumbs: HashMap<usize, Vec<u32>>,
    tick: u64,
    pub quit: bool,
    pub status: Option<String>,
}

impl App {
    pub fn new(data: GameData, audio: Audio) -> App {
        let cars = [
            data.vehicles.iter().position(|v| v.id == "f1").unwrap_or(0),
            data.vehicles
                .iter()
                .position(|v| v.id == "mini")
                .unwrap_or(0),
        ];
        App {
            data,
            settings: Settings {
                players: 1,
                track: 0,
                laps: 3,
                cars,
                opponents: 3,
            },
            screen: Screen::Menu { cursor: 0 },
            frame: Frame::new(),
            audio,
            demo: None,
            demo_track: 0,
            thumbs: HashMap::new(),
            tick: 0,
            quit: false,
            status: None,
        }
    }

    fn items(&self) -> Vec<Item> {
        let mut v = vec![
            Item::Start,
            Item::Players,
            Item::Track,
            Item::Laps,
            Item::Car1,
        ];
        if self.settings.players == 2 {
            v.push(Item::Car2);
        }
        v.extend([Item::Opponents, Item::Quit]);
        v
    }

    pub fn build_race(
        &self,
        track_index: usize,
        humans: usize,
        cars: [usize; 2],
        opponents: usize,
        laps: usize,
        seed: u32,
    ) -> Result<Race, String> {
        let track = self.data.load_track(track_index)?;
        let nv = self.data.vehicles.len();
        let mut setup = Vec::new();
        for (p, &car) in cars.iter().enumerate().take(humans) {
            setup.push(EntrantSetup {
                vehicle: car,
                driver: Driver::Human(p),
                team: p,
                name: format!("PLAYER {}", p + 1),
            });
        }
        let mut rng = seed.wrapping_mul(2654435761) | 1;
        for i in 0..opponents.min(8 - humans) {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            setup.push(EntrantSetup {
                vehicle: rng as usize % nv,
                driver: Driver::Cpu,
                team: humans + i,
                name: CPU_NAMES[(i + seed as usize) % CPU_NAMES.len()].to_string(),
            });
        }
        // Humans start at the back, as is tradition.
        let back = humans.min(setup.len());
        setup.rotate_left(back);
        Ok(Race::new(track, &self.data.vehicles, &setup, laps, seed))
    }

    fn thumb(&mut self, i: usize) -> Option<&Vec<u32>> {
        if !self.thumbs.contains_key(&i) {
            let t = self.data.load_track(i).ok()?;
            self.thumbs
                .insert(i, render::downscale(&t.rgb(), THUMB_W, THUMB_H));
        }
        self.thumbs.get(&i)
    }

    pub fn update(&mut self, input: &Input) {
        self.tick += 1;
        match &mut self.screen {
            Screen::Menu { .. } => self.update_menu(input),
            Screen::Race { race, paused, .. } => {
                if input.hit(Key::Escape) {
                    self.audio.silence();
                    self.screen = Screen::Menu { cursor: 0 };
                    return;
                }
                if input.hit(Key::P) {
                    *paused = !*paused;
                }
                if *paused {
                    self.audio.silence();
                    return;
                }
                let two = self.settings.players == 2;
                let p1 = if two {
                    input.controls(Key::Up, Key::Down, Key::Left, Key::Right)
                } else {
                    let a = input.controls(Key::Up, Key::Down, Key::Left, Key::Right);
                    let b = input.controls(Key::W, Key::S, Key::A, Key::D);
                    Controls {
                        throttle: a.throttle.max(b.throttle),
                        brake: a.brake.max(b.brake),
                        steer: (a.steer + b.steer).clamp(-1.0, 1.0),
                    }
                };
                let p2 = input.controls(Key::W, Key::S, Key::A, Key::D);
                race.step(&[p1, p2]);
                for (i, e) in race
                    .entrants
                    .iter()
                    .filter(|e| matches!(e.driver, Driver::Human(_)))
                    .enumerate()
                {
                    let c = &e.car;
                    let skid = if c.on_road {
                        ((c.slip - 50.0) / 100.0).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    self.audio.set_voice(i, true, c.rpm, skid, c.bump);
                }
                if race.phase == Phase::Done {
                    self.audio.silence();
                    let Screen::Race { race, .. } =
                        std::mem::replace(&mut self.screen, Screen::Menu { cursor: 0 })
                    else {
                        unreachable!()
                    };
                    self.screen = Screen::Results { race };
                }
            }
            Screen::Results { .. } => {
                if input.hit(Key::Enter) || input.hit(Key::Escape) || input.hit(Key::Space) {
                    self.screen = Screen::Menu { cursor: 0 };
                }
            }
        }
    }

    fn update_menu(&mut self, input: &Input) {
        self.step_demo();
        let items = self.items();
        let Screen::Menu { cursor } = &mut self.screen else {
            return;
        };
        let mut cur = (*cursor).min(items.len() - 1);
        if input.hit(Key::Up) {
            cur = (cur + items.len() - 1) % items.len();
        }
        if input.hit(Key::Down) {
            cur = (cur + 1) % items.len();
        }
        *cursor = cur;
        let delta = input.hit(Key::Right) as i32 - input.hit(Key::Left) as i32;
        let item = items[cur];
        let s = &mut self.settings;
        let nv = self.data.vehicles.len();
        let wrap = |v: usize, n: usize, d: i32| (v as i32 + d).rem_euclid(n as i32) as usize;
        if delta != 0 {
            match item {
                Item::Players => s.players = if s.players == 1 { 2 } else { 1 },
                Item::Track => s.track = wrap(s.track, self.data.tracks.len(), delta),
                Item::Laps => {
                    let i = LAP_CHOICES.iter().position(|&l| l == s.laps).unwrap_or(1);
                    s.laps = LAP_CHOICES[wrap(i, LAP_CHOICES.len(), delta)];
                }
                Item::Car1 => s.cars[0] = wrap(s.cars[0], nv, delta),
                Item::Car2 => s.cars[1] = wrap(s.cars[1], nv, delta),
                Item::Opponents => s.opponents = wrap(s.opponents, 8 - s.players + 1, delta),
                _ => {}
            }
            s.opponents = s.opponents.min(8 - s.players);
        }
        if input.hit(Key::Escape) {
            self.quit = true;
        }
        if input.hit(Key::Enter) || input.hit(Key::Space) {
            match item {
                Item::Quit => self.quit = true,
                Item::Start
                | Item::Track
                | Item::Car1
                | Item::Car2
                | Item::Laps
                | Item::Players
                | Item::Opponents => {
                    let s = &self.settings;
                    match self.build_race(
                        s.track,
                        s.players,
                        s.cars,
                        s.opponents,
                        s.laps,
                        self.tick as u32,
                    ) {
                        Ok(r) => {
                            let cams = [(0.0, 0.0); 2];
                            let mut screen = Screen::Race {
                                race: Box::new(r),
                                cams,
                                paused: false,
                            };
                            if let Screen::Race { race, cams, .. } = &mut screen {
                                for (i, cam) in cams.iter_mut().enumerate() {
                                    if let Some(e) =
                                        race.entrants.iter().find(|e| e.driver == Driver::Human(i))
                                    {
                                        *cam = e.car.screen_pos();
                                    }
                                }
                            }
                            self.screen = screen;
                        }
                        Err(e) => self.status = Some(e),
                    }
                }
            }
        }
    }

    fn step_demo(&mut self) {
        let finished = self
            .demo
            .as_ref()
            .is_none_or(|d| d.phase == Phase::Done || d.race_time() > 75.0);
        if finished {
            self.demo_track = (self.demo_track + 7) % self.data.tracks.len();
            self.demo = self
                .build_race(self.demo_track, 0, [0, 0], 4, 2, self.tick as u32 + 99)
                .ok()
                .map(Box::new);
        }
        if let Some(d) = &mut self.demo {
            d.step(&[]);
        }
    }

    pub fn draw(&mut self) {
        let take = std::mem::replace(&mut self.screen, Screen::Menu { cursor: 0 });
        match &take {
            Screen::Menu { cursor } => self.draw_menu(*cursor),
            Screen::Race { race, paused, .. } => {
                let mut cams = match &take {
                    Screen::Race { cams, .. } => *cams,
                    _ => unreachable!(),
                };
                draw_race(
                    &mut self.frame,
                    &self.data,
                    race,
                    &mut cams,
                    self.settings.players,
                );
                if *paused {
                    self.frame
                        .shade(Rect::new(0, 100, SCREEN_W as i32, 40), 120);
                    self.frame.text_centered(160, 108, "PAUSED", YELLOW, 2);
                    self.frame
                        .text_centered(160, 126, "P: RESUME   ESC: QUIT RACE", WHITE, 1);
                }
                self.screen = take;
                if let Screen::Race { cams: c, .. } = &mut self.screen {
                    *c = cams;
                }
                return;
            }
            Screen::Results { race } => draw_results(&mut self.frame, &self.data, race),
        }
        self.screen = take;
    }

    fn draw_menu(&mut self, cursor: usize) {
        let items = self.items();
        let f = &mut self.frame;
        if let Some(d) = &self.demo {
            let leader = d.standings()[0];
            let mut cam = [d.entrants[leader].car.screen_pos(); 2];
            draw_world(
                f,
                &self.data,
                d,
                Rect::new(0, 0, SCREEN_W as i32, SCREEN_H as i32),
                &mut cam[0],
                true,
                1.0,
            );
            f.shade(Rect::new(0, 0, SCREEN_W as i32, SCREEN_H as i32), 110);
        } else {
            f.clear(0x101820);
        }

        f.text_centered(160, 10, "SKIDS", YELLOW, 4);
        f.text_centered(160, 42, "A SUPER SKIDMARKS REVIVAL", WHITE, 1);

        let cursor = cursor.min(items.len() - 1);
        let s = &self.settings;
        for (i, item) in items.iter().enumerate() {
            let label = match item {
                Item::Start => "START RACE".to_string(),
                Item::Players => format!("PLAYERS    < {} >", s.players),
                Item::Track => format!("TRACK      < {} >", self.data.tracks[s.track].name),
                Item::Laps => format!("LAPS       < {} >", s.laps),
                Item::Car1 => format!("P1 CAR     < {} >", self.data.vehicles[s.cars[0]].name),
                Item::Car2 => format!("P2 CAR     < {} >", self.data.vehicles[s.cars[1]].name),
                Item::Opponents => format!("OPPONENTS  < {} >", s.opponents),
                Item::Quit => "QUIT".to_string(),
            };
            let y = 64 + i as i32 * 14;
            let colour = if i == cursor { YELLOW } else { WHITE };
            if i == cursor {
                f.text_shadow(8, y, ">", RED, 1);
            }
            f.text_shadow(18, y, &label, colour, 1);
        }

        // Track preview.
        let track = s.track;
        let px = SCREEN_W as i32 - THUMB_W as i32 - 10;
        if let Some(th) = self.thumb(track).cloned() {
            let f = &mut self.frame;
            f.frame_rect(
                Rect::new(px - 2, 62, THUMB_W as i32 + 4, THUMB_H as i32 + 4),
                WHITE,
            );
            f.image(&th, THUMB_W, THUMB_H, px, 64);
        }

        // Car preview, slowly turning.
        let f = &mut self.frame;
        let s = &self.settings;
        let which = if items[cursor] == Item::Car2 { 1 } else { 0 };
        let v = &self.data.vehicles[s.cars[which]];
        let heading = (self.tick / 4) as usize % vehicle::HEADINGS;
        let pal = palette::car_palette(which);
        let (cx, cy) = (px + THUMB_W as i32 / 2 - 32, 150);
        f.fill(Rect::new(px - 2, 140, THUMB_W as i32 + 4, 52), 0x203018);
        f.frame_rect(Rect::new(px - 2, 140, THUMB_W as i32 + 4, 52), WHITE);
        let clip = Rect::new(0, 0, SCREEN_W as i32, SCREEN_H as i32);
        f.sprite_shadow(v.frame(vehicle::LEVEL, heading), cx + 3, cy + 3, clip);
        f.sprite(v.frame(vehicle::LEVEL, heading), &pal, cx, cy, clip);
        f.text_centered(px + THUMB_W as i32 / 2, 182, &v.name, WHITE, 1);

        let help = if self.settings.players == 2 {
            "P1: ARROWS   P2: W A S D"
        } else {
            "DRIVE: ARROWS OR W A S D"
        };
        f.text_centered(160, 222, help, GREY, 1);
        f.text_centered(
            160,
            236,
            "UP/DOWN: CHOOSE  LEFT/RIGHT: CHANGE  ENTER: GO",
            GREY,
            1,
        );
        if let Some(st) = &self.status {
            f.text_centered(160, 200, st, RED, 1);
        }
    }
}

pub fn draw_race(
    f: &mut Frame,
    data: &GameData,
    race: &Race,
    cams: &mut [(f32, f32); 2],
    players: usize,
) {
    f.clear(0);
    let views: Vec<Rect> = if players == 2 {
        vec![
            Rect::new(0, 0, SCREEN_W as i32, 127),
            Rect::new(0, 129, SCREEN_W as i32, 127),
        ]
    } else {
        vec![Rect::new(0, 0, SCREEN_W as i32, SCREEN_H as i32)]
    };
    for (p, view) in views.iter().enumerate() {
        let Some(ei) = race
            .entrants
            .iter()
            .position(|e| e.driver == Driver::Human(p))
        else {
            continue;
        };
        let e = &race.entrants[ei];
        // Look ahead in the direction of travel.
        let target = track::to_screen(e.car.x + e.car.vx * 0.3, e.car.y + e.car.vy * 0.3);
        cams[p].0 += (target.0 - cams[p].0) * 0.12;
        cams[p].1 += (target.1 - cams[p].1) * 0.12;
        draw_world(f, data, race, *view, &mut cams[p], false, 1.0);
        draw_hud(f, race, ei, *view);
    }
    if players == 2 {
        f.fill(Rect::new(0, 127, SCREEN_W as i32, 2), 0x000000);
    }
    if let Some(n) = race.countdown() {
        f.text_centered(160, 110, &n.to_string(), YELLOW, 5);
    } else if race.race_time() < 1.0 {
        f.text_centered(160, 110, "GO!", YELLOW, 5);
    }
}

/// Draws the track and cars into `view`, centred on `cam` (screen-space track pixel).
pub fn draw_world(
    f: &mut Frame,
    data: &GameData,
    race: &Race,
    view: Rect,
    cam: &mut (f32, f32),
    _demo: bool,
    _zoom: f32,
) {
    let cx =
        (cam.0 - view.w as f32 / 2.0).clamp(0.0, (track::W as i32 - view.w).max(0) as f32) as i32;
    let cy =
        (cam.1 - view.h as f32 / 2.0).clamp(0.0, (track::H as i32 - view.h).max(0) as f32) as i32;
    f.blit_track(&race.ground, view, cx, cy);

    for p in &race.particles {
        let x = view.x + p.x as i32 - cx;
        let y = view.y + p.y as i32 - cy;
        if x >= view.x && x < view.x + view.w && y >= view.y && y < view.y + view.h {
            f.put(x, y, p.colour);
        }
    }

    let mut order: Vec<usize> = (0..race.entrants.len()).collect();
    order.sort_by(|&a, &b| {
        race.entrants[a]
            .car
            .screen_pos()
            .1
            .total_cmp(&race.entrants[b].car.screen_pos().1)
    });
    for i in order {
        let e = &race.entrants[i];
        let c = &e.car;
        let v = &data.vehicles[c.vehicle];
        let (sx, sy) = c.screen_pos();
        let x = view.x + sx as i32 - cx - (vehicle::SPR_W as i32 / 2);
        let y = view.y + sy as i32 - cy - (vehicle::SPR_H as i32 / 2);
        let pix = v.frame(c.tilt, vehicle::heading_frame(c.heading));
        f.sprite_shadow(pix, x + 3, y + 2, view);
        f.sprite(pix, &palette::car_palette(c.team), x, y, view);
        if let Driver::Human(p) = e.driver {
            let tag = format!("P{}", p + 1);
            let tx = x + 32 - 5;
            let ty = y - 2;
            if ty > view.y && ty < view.y + view.h - 8 {
                f.text_shadow(tx, ty, &tag, if p == 0 { 0xff8070 } else { 0x80a0ff }, 1);
            }
        }
    }
}

fn draw_hud(f: &mut Frame, race: &Race, ei: usize, view: Rect) {
    let e = &race.entrants[ei];
    let n = race.track.gates.len();
    let lap = (e.laps_done(n) + 1).min(race.laps);
    let standings = race.standings();
    let pos = standings.iter().position(|&i| i == ei).unwrap_or(0) + 1;
    let (x, y) = (view.x + 6, view.y + 5);
    f.text_shadow(x, y, &format!("LAP {}/{}", lap, race.laps), WHITE, 1);
    f.text_shadow(
        x,
        y + 10,
        &format!("POS {}/{}", ordinal(pos), race.entrants.len()),
        YELLOW,
        1,
    );
    let t = e.finished.unwrap_or_else(|| race.race_time());
    let ts = fmt_time(t);
    f.text_shadow(
        view.x + view.w - 6 - (ts.len() as i32 * 6),
        y,
        &ts,
        WHITE,
        1,
    );
    if let Some(l) = e.last_lap {
        let ls = format!("LAST {}", fmt_time(l));
        f.text_shadow(
            view.x + view.w - 6 - (ls.len() as i32 * 6),
            y + 10,
            &ls,
            GREY,
            1,
        );
    }
    if e.finished.is_some() {
        let finish_pos = race.finish_order.iter().position(|&i| i == ei).unwrap_or(0) + 1;
        f.text_centered(
            view.x + view.w / 2,
            view.y + view.h / 2 - 20,
            &format!("FINISHED {}", ordinal(finish_pos)),
            YELLOW,
            2,
        );
    }
}

fn draw_results(f: &mut Frame, data: &GameData, race: &Race) {
    f.clear(0x101820);
    f.text_centered(160, 16, "RESULTS", YELLOW, 3);
    f.text_centered(160, 44, &race.track.name, GREY, 1);
    let n = race.track.gates.len();
    f.text_shadow(16, 64, "POS  DRIVER     CAR         TIME     BEST", GREY, 1);
    let leader_passed = race.entrants.iter().map(|e| e.passed).max().unwrap_or(0);
    for (row, &i) in race.standings().iter().enumerate() {
        let e = &race.entrants[i];
        let time = match e.finished {
            Some(t) => fmt_time(t),
            None => {
                let behind = leader_passed.saturating_sub(e.passed).div_ceil(n);
                if behind > 0 {
                    format!("+{behind} LAP")
                } else {
                    "RUNNING".into()
                }
            }
        };
        let best = e.best_lap.map(fmt_time).unwrap_or_else(|| "-".into());
        let car = &data.vehicles[e.car.vehicle].name;
        let line = format!(
            "{:<4} {:<10} {:<11} {:<8} {}",
            ordinal(row + 1),
            e.name,
            truncate(car, 11),
            time,
            best
        );
        let colour = if matches!(e.driver, Driver::Human(_)) {
            YELLOW
        } else {
            WHITE
        };
        f.text_shadow(16, 80 + row as i32 * 12, &line, colour, 1);
    }
    f.text_centered(160, 236, "PRESS ENTER", GREY, 1);
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, x) if x != 11 => "ST",
        (2, x) if x != 12 => "ND",
        (3, x) if x != 13 => "RD",
        _ => "TH",
    };
    format!("{n}{suffix}")
}

pub fn fmt_time(t: f32) -> String {
    let tenths = (t * 10.0).round() as u32;
    format!("{}:{:02}.{}", tenths / 600, (tenths / 10) % 60, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting() {
        assert_eq!(ordinal(1), "1ST");
        assert_eq!(ordinal(12), "12TH");
        assert_eq!(ordinal(23), "23RD");
        assert_eq!(fmt_time(83.46), "1:23.5");
    }
}
