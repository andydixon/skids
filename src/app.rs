//! Front end: menu, race screens and results.

use crate::audio::{Audio, Sfx};
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

#[derive(Clone, Copy, PartialEq)]
pub enum Music {
    Menus,
    Always,
    Off,
}

pub struct Settings {
    pub players: usize,
    pub track: usize,
    pub laps: usize,
    pub cars: [usize; 2],
    pub opponents: usize,
    /// Every car tows a caravan.
    pub caravans: bool,
    /// 0 = each car's own handling, else a preset (`vehicle::PRESETS`).
    pub handling: usize,
    pub music: Music,
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
    Caravans,
    Handling,
    Music,
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
        sounds: RaceSounds,
    },
    Results {
        race: Box<Race>,
    },
}

/// What the race screen remembers between frames to decide when to play
/// one-shot sounds.
#[derive(Default)]
struct RaceSounds {
    countdown: Option<u32>,
    go_played: bool,
    bumps: Vec<f32>,
    throttle: [f32; 2],
}

pub struct App {
    pub data: GameData,
    /// Indices of vehicles that can be driven (not trailers).
    cars: Vec<usize>,
    /// The title picture at half size, if the data has one.
    title: Option<(usize, usize, Vec<u32>)>,
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
        let drivable: Vec<usize> = (0..data.vehicles.len())
            .filter(|&i| !data.vehicles[i].trailer)
            .collect();
        let pick = |id: &str| {
            data.vehicles
                .iter()
                .position(|v| v.id == id)
                .unwrap_or(drivable[0])
        };
        let cars = [pick("f1"), pick("mini")];
        let title = data.pictures.get("skidtoon").map(half_size);
        let music = if audio.has_music() {
            Music::Menus
        } else {
            Music::Off
        };
        audio.set_music(music != Music::Off);
        App {
            data,
            cars: drivable,
            title,
            settings: Settings {
                players: 1,
                track: 0,
                laps: 3,
                cars,
                opponents: 3,
                caravans: false,
                handling: 0,
                music,
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
        v.push(Item::Opponents);
        if self.data.vehicles.iter().any(|v| v.trailer) {
            v.push(Item::Caravans);
        }
        v.push(Item::Handling);
        if self.audio.has_music() {
            v.push(Item::Music);
        }
        v.push(Item::Quit);
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
        let trailer = if self.settings.caravans {
            self.data.vehicles.iter().position(|v| v.trailer)
        } else {
            None
        };
        let mut setup = Vec::new();
        for (p, &car) in cars.iter().enumerate().take(humans) {
            setup.push(EntrantSetup {
                vehicle: car,
                driver: Driver::Human(p),
                team: p,
                name: format!("PLAYER {}", p + 1),
                trailer,
                preset: self.settings.handling,
            });
        }
        let mut rng = seed.wrapping_mul(2654435761) | 1;
        for i in 0..opponents.min(8 - humans) {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            setup.push(EntrantSetup {
                vehicle: self.cars[rng as usize % self.cars.len()],
                driver: Driver::Cpu,
                team: humans + i,
                name: CPU_NAMES[(i + seed as usize) % CPU_NAMES.len()].to_string(),
                trailer,
                preset: self.settings.handling,
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
            let img = match &t.thumbnail {
                // The original thumbnail: Amiga hi-res pixels, so each row is
                // shown twice to keep its shape.
                Some(px) => {
                    let pal: Vec<u32> = t.palette.iter().map(|c| c.to_u32()).collect();
                    let mut out = vec![0x101010u32; THUMB_W * THUMB_H];
                    for y in 0..THUMB_H {
                        for x in 0..track::THUMB_W.min(THUMB_W) {
                            out[y * THUMB_W + x] = pal[px[(y / 2) * track::THUMB_W + x] as usize];
                        }
                    }
                    out
                }
                None => render::downscale(&t.rgb(), THUMB_W, THUMB_H),
            };
            self.thumbs.insert(i, img);
        }
        self.thumbs.get(&i)
    }

    pub fn update(&mut self, input: &Input) {
        self.tick += 1;
        match &mut self.screen {
            Screen::Menu { .. } => self.update_menu(input),
            Screen::Race {
                race,
                paused,
                sounds,
                cams,
            } => {
                if input.hit(Key::Escape) {
                    self.audio.silence();
                    self.audio.set_music(self.settings.music != Music::Off);
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
                race_sounds(&self.audio, race, sounds, cams, input, self.tick);
                if race.phase == Phase::Done {
                    self.audio.silence();
                    self.audio.set_music(self.settings.music != Music::Off);
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
        let wrap = |v: usize, n: usize, d: i32| (v as i32 + d).rem_euclid(n as i32) as usize;
        if delta != 0 {
            match item {
                Item::Players => s.players = if s.players == 1 { 2 } else { 1 },
                Item::Track => s.track = wrap(s.track, self.data.tracks.len(), delta),
                Item::Laps => {
                    let i = LAP_CHOICES.iter().position(|&l| l == s.laps).unwrap_or(1);
                    s.laps = LAP_CHOICES[wrap(i, LAP_CHOICES.len(), delta)];
                }
                Item::Car1 | Item::Car2 => {
                    let slot = (item == Item::Car2) as usize;
                    let at = self
                        .cars
                        .iter()
                        .position(|&c| c == s.cars[slot])
                        .unwrap_or(0);
                    s.cars[slot] = self.cars[wrap(at, self.cars.len(), delta)];
                }
                Item::Opponents => s.opponents = wrap(s.opponents, 8 - s.players + 1, delta),
                Item::Caravans => s.caravans = !s.caravans,
                Item::Handling => s.handling = wrap(s.handling, vehicle::PRESETS.len() + 1, delta),
                Item::Music => {
                    const ORDER: [Music; 3] = [Music::Menus, Music::Always, Music::Off];
                    let at = ORDER.iter().position(|&m| m == s.music).unwrap_or(0);
                    s.music = ORDER[wrap(at, 3, delta)];
                    self.audio.set_music(s.music != Music::Off);
                }
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
                | Item::Caravans
                | Item::Handling
                | Item::Music
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
                                sounds: RaceSounds::default(),
                            };
                            self.audio.set_music(self.settings.music == Music::Always);
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

        let top = match &self.title {
            Some((w, h, px)) => {
                f.image(px, *w, *h, (SCREEN_W as i32 - *w as i32) / 2, 2);
                *h as i32 + 6
            }
            None => {
                f.text_centered(160, 10, "SKIDS", YELLOW, 4);
                f.text_centered(160, 42, "A SUPER SKIDMARKS REVIVAL", WHITE, 1);
                60
            }
        };

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
                Item::Caravans => {
                    format!("CARAVANS   < {} >", if s.caravans { "ON" } else { "OFF" })
                }
                Item::Handling => format!(
                    "HANDLING   < {} >",
                    match s.handling {
                        0 => "CUSTOM",
                        n => vehicle::PRESETS[n - 1].0,
                    }
                ),
                Item::Music => format!(
                    "MUSIC      < {} >",
                    match s.music {
                        Music::Menus => "MENUS",
                        Music::Always => "ALWAYS",
                        Music::Off => "OFF",
                    }
                ),
                Item::Quit => "QUIT".to_string(),
            };
            let y = top + i as i32 * 12;
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
                Rect::new(px - 2, top - 2, THUMB_W as i32 + 4, THUMB_H as i32 + 4),
                WHITE,
            );
            f.image(&th, THUMB_W, THUMB_H, px, top);
            let set = self
                .data
                .load_track(track)
                .map(|t| t.set_name)
                .unwrap_or_default();
            f.text_centered(
                px + THUMB_W as i32 / 2,
                top + THUMB_H as i32 + 4,
                &truncate(&set.to_uppercase(), 21),
                GREY,
                1,
            );
        }

        // Car preview, slowly turning.
        let f = &mut self.frame;
        let s = &self.settings;
        let which = if items[cursor] == Item::Car2 { 1 } else { 0 };
        let v = &self.data.vehicles[s.cars[which]];
        let heading = (self.tick / 4) as usize % vehicle::HEADINGS;
        let pal = palette::car_palette(which);
        let by = top + THUMB_H as i32 + 16;
        let (cx, cy) = (px + THUMB_W as i32 / 2 - 32, by + 3);
        f.fill(Rect::new(px - 2, by, THUMB_W as i32 + 4, 48), 0x203018);
        f.frame_rect(Rect::new(px - 2, by, THUMB_W as i32 + 4, 48), WHITE);
        let clip = Rect::new(0, 0, SCREEN_W as i32, SCREEN_H as i32);
        f.sprite_shadow(v.frame(vehicle::LEVEL, heading), cx + 3, cy + 3, clip);
        f.sprite(v.frame(vehicle::LEVEL, heading), &pal, cx, cy, clip);
        let label = if v.driver.is_empty() {
            v.name.clone()
        } else {
            format!("{} - {}", v.name, v.driver)
        };
        f.text_centered(
            px + THUMB_W as i32 / 2,
            by + 38,
            &truncate(&label, 21),
            WHITE,
            1,
        );

        let help = if self.settings.players == 2 {
            "P1: ARROWS   P2: W A S D"
        } else {
            "DRIVE: ARROWS OR W A S D"
        };
        f.text_centered(160, 230, help, GREY, 1);
        f.text_centered(
            160,
            242,
            "UP/DOWN: CHOOSE  LEFT/RIGHT: CHANGE  ENTER: GO",
            GREY,
            1,
        );
        if let Some(st) = &self.status {
            f.text_centered(160, 218, st, RED, 1);
        }
    }
}

/// Plays the sounds a race frame calls for: engines and tyres for each
/// player, countdown beeps, horns, backfires, and crashes near a camera.
fn race_sounds(
    audio: &Audio,
    race: &Race,
    s: &mut RaceSounds,
    cams: &[(f32, f32); 2],
    input: &Input,
    tick: u64,
) {
    let humans: Vec<usize> = (0..race.entrants.len())
        .filter(|&i| matches!(race.entrants[i].driver, Driver::Human(_)))
        .collect();
    for (slot, &i) in humans.iter().enumerate().take(2) {
        let c = &race.entrants[i].car;
        let skid = if c.on_road {
            ((c.slip - 50.0) / 100.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        audio.set_voice(slot, true, c.rpm, skid);
        let pan = if humans.len() == 2 {
            [0.3, 0.7][slot]
        } else {
            0.5
        };
        let throttle = c.controls.throttle;
        // Lifting off at high revs pops now and then.
        if s.throttle[slot] > 0.0
            && throttle == 0.0
            && c.rpm > 0.7
            && (tick.wrapping_mul(2654435761)).is_multiple_of(4)
        {
            audio.play(Sfx::Backfire, pan);
        }
        s.throttle[slot] = throttle;
    }
    // Player one's horn is Right Shift, player two's Left Shift.
    let horn_keys = [Key::RightShift, Key::LeftShift];
    for (slot, key) in horn_keys.iter().enumerate().take(humans.len().max(1)) {
        if input.hit(*key) {
            audio.play(
                Sfx::Horn,
                if humans.len() == 2 {
                    [0.3, 0.7][slot]
                } else {
                    0.5
                },
            );
        }
    }

    match race.countdown() {
        Some(n) if s.countdown != Some(n) => {
            s.countdown = Some(n);
            audio.play(Sfx::Beep, 0.5);
        }
        None if !s.go_played && race.phase != Phase::Countdown => {
            s.go_played = true;
            audio.play(Sfx::Go, 0.5);
        }
        _ => {}
    }

    s.bumps.resize(race.entrants.len(), 0.0);
    for (i, e) in race.entrants.iter().enumerate() {
        let b = e.car.bump;
        if b > 0.3 && b > s.bumps[i] + 0.2 {
            let (sx, sy) = e.car.screen_pos();
            let near = cams
                .iter()
                .take(humans.len().max(1))
                .map(|c| (c.0 - sx).hypot(c.1 - sy))
                .fold(f32::MAX, f32::min);
            if near < 220.0 {
                let pan = (0.5 + (sx - cams[0].0) / 400.0).clamp(0.1, 0.9);
                audio.play(Sfx::Crash(b), pan);
            }
        }
        s.bumps[i] = b;
    }
}

/// Halves a picture with a 2x2 box filter, for the menu header.
fn half_size(p: &crate::formats::iff::Picture) -> (usize, usize, Vec<u32>) {
    let (w, h) = (p.w / 2, p.h / 2);
    let rgb = |x: usize, y: usize| {
        let c = p
            .palette
            .get(p.pixels[y * p.w + x] as usize)
            .copied()
            .unwrap_or((0, 0, 0));
        (c.0 as u32, c.1 as u32, c.2 as u32)
    };
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let px = [
                rgb(2 * x, 2 * y),
                rgb(2 * x + 1, 2 * y),
                rgb(2 * x, 2 * y + 1),
                rgb(2 * x + 1, 2 * y + 1),
            ];
            let avg = |f: fn(&(u32, u32, u32)) -> u32| px.iter().map(f).sum::<u32>() / 4;
            out.push((avg(|c| c.0) << 16) | (avg(|c| c.1) << 8) | avg(|c| c.2));
        }
    }
    (w, h, out)
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
        let target = track::project(e.car.x + e.car.vx * 0.3, e.car.y + e.car.vy * 0.3, e.car.z);
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

    // Cars and caravans, back to front.
    let mut order: Vec<(f32, usize, bool)> = Vec::new();
    for (i, e) in race.entrants.iter().enumerate() {
        order.push((e.car.screen_pos().1, i, false));
        if let Some(t) = e.trailer {
            order.push((race.track.to_screen(t.x, t.y).1, i, true));
        }
    }
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, i, is_trailer) in order {
        let e = &race.entrants[i];
        let c = &e.car;
        let pal = palette::car_palette(c.team);
        if is_trailer {
            let t = e.trailer.unwrap();
            let (sx, sy) = race.track.to_screen(t.x, t.y);
            let x = view.x + sx as i32 - cx - (vehicle::SPR_W as i32 / 2);
            let y = view.y + sy as i32 - cy - (vehicle::SPR_H as i32 / 2);
            let pix =
                data.vehicles[t.vehicle].frame(vehicle::LEVEL, vehicle::heading_frame(t.heading));
            f.sprite_shadow(pix, x + 3, y + 2, view);
            f.sprite(pix, &pal, x, y, view);
            continue;
        }
        let v = &data.vehicles[c.vehicle];
        let (sx, sy) = c.screen_pos();
        let x = view.x + sx as i32 - cx - (vehicle::SPR_W as i32 / 2);
        let y = view.y + sy as i32 - cy - (vehicle::SPR_H as i32 / 2);
        let pix = v.frame(c.tilt, vehicle::heading_frame(c.heading));
        f.sprite_shadow(pix, x + 3, y + 2, view);
        f.sprite(pix, &pal, x, y, view);
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
