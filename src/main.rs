//! Skids: a cross-platform revival of Super Skidmarks (Acid Software, 1995).
//!
//! The engine reads the original Amiga data (ADF images, the LHA archives on
//! them, or extracted files) at run time; no game assets are bundled.

mod app;
mod audio;
mod data;
mod formats;
mod game;
mod render;

use app::{App, Input};
use data::GameData;
use minifb::{Key, KeyRepeat, Scale, ScaleMode, Window, WindowOptions};
use render::{SCREEN_H, SCREEN_W};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
skids - Super Skidmarks revival

USAGE:
    skids [--data DIR] [--scale 1|2|4] [--mute]   play
    skids info    [--data DIR]                    list what was found in the data
    skids extract [--data DIR] --out DIR          write PNGs of every track and car
    skids render  [--data DIR] --out FILE.png [--track N] [--seconds S] [--players 1|2] [--menu]
                                                  simulate a race (or the menu) and save a frame
    skids bench   [--data DIR] [--seconds S]      race computer cars on every track
    skids trace   [--data DIR] --out FILE.png [--track N] [--seconds S]
                                                  draw the computer cars' paths on a track

The data directory should hold the Super Skidmarks ADF images (or the LHA
archives from them, or extracted track/ and vehicles/ folders). It defaults to
the current directory, then the directory the executable is in.

CONTROLS:
    Player 1: arrow keys (or W A S D in one-player games)
    Player 2: W A S D
    P pauses, Esc leaves the race.";

struct Args {
    cmd: String,
    data: Option<PathBuf>,
    out: Option<PathBuf>,
    scale: u32,
    mute: bool,
    track: usize,
    seconds: f32,
    players: usize,
    menu: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        cmd: "play".into(),
        data: None,
        out: None,
        scale: 2,
        mute: false,
        track: 1,
        seconds: 12.0,
        players: 1,
        menu: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" | "help" => return Err(String::new()),
            "--data" | "-d" => a.data = Some(val("--data")?.into()),
            "--out" | "-o" => a.out = Some(val("--out")?.into()),
            "--scale" => a.scale = val("--scale")?.parse().map_err(|_| "bad --scale")?,
            "--track" => a.track = val("--track")?.parse().map_err(|_| "bad --track")?,
            "--seconds" => a.seconds = val("--seconds")?.parse().map_err(|_| "bad --seconds")?,
            "--players" => a.players = val("--players")?.parse().map_err(|_| "bad --players")?,
            "--mute" => a.mute = true,
            "--menu" => a.menu = true,
            "info" | "extract" | "render" | "bench" | "trace" | "play" => a.cmd = arg,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

fn find_data(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(p) = explicit {
        return p;
    }
    let looks_right = |d: &Path| {
        std::fs::read_dir(d).is_ok_and(|rd| {
            rd.flatten().any(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                n.ends_with(".adf") || n.ends_with(".lha") || n == "track" || n == "data"
            })
        })
    };
    let mut candidates = vec![PathBuf::from(".")];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        candidates.push(dir);
    }
    candidates.push(PathBuf::from("data"));
    candidates
        .into_iter()
        .find(|d| looks_right(d))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprintln!("{USAGE}");
            return if e.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            };
        }
    };
    let root = find_data(args.data.clone());
    let data = match GameData::load(&root) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    for w in &data.warnings {
        eprintln!("warning: {w}");
    }
    let result = match args.cmd.as_str() {
        "info" => {
            info(&data);
            Ok(())
        }
        "extract" => extract(&data, args.out.as_deref()),
        "render" => render_shot(data, &args),
        "bench" => bench(data, &args),
        "trace" => trace(data, &args),
        _ => play(data, &args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn info(data: &GameData) {
    println!("Tracks:");
    for (f, s) in data.track_list() {
        let tf = &data.track_files[f];
        match tf.decode(s) {
            Ok(t) => println!("  {:<10} {:>2} gates", t.name, t.gates.len()),
            Err(e) => println!("  {:<10} ERROR {e}", tf.track_name(s)),
        }
    }
    println!("Vehicles:");
    for v in &data.vehicles {
        println!("  {:<10} ({})", v.name, v.id);
    }
}

fn extract(data: &GameData, out: Option<&Path>) -> Result<(), String> {
    use data::{track, vehicle};
    let out = out.ok_or("extract needs --out DIR")?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for (f, s) in data.track_list() {
        let t = data.load_track(f, s)?;
        let mut img = t.rgb();
        // Overlay the lap gates so the track data can be checked by eye.
        for g in &t.gates {
            let steps = 64;
            for i in 0..=steps {
                let k = i as f32 / steps as f32;
                let (x, y) =
                    track::to_screen(g.a.0 + (g.b.0 - g.a.0) * k, g.a.1 + (g.b.1 - g.a.1) * k);
                if (0.0..track::W as f32).contains(&x) && (0.0..track::H as f32).contains(&y) {
                    img[y as usize * track::W + x as usize] = 0xff2020;
                }
            }
        }
        let path = out.join(format!("{}.png", t.name.to_lowercase()));
        render::save_png(&path, track::W, track::H, &img).map_err(|e| e.to_string())?;
        println!("wrote {}", path.display());
    }
    for (vi, v) in data.vehicles.iter().enumerate() {
        // Sheet: 32 headings across, 25 tilt blocks down.
        let (w, h) = (
            vehicle::SPR_W * vehicle::HEADINGS,
            vehicle::SPR_H * vehicle::TILTS,
        );
        let pal = data::palette::car_palette(vi);
        let mut img = vec![0x203018u32; w * h];
        for tilt in 0..vehicle::TILTS {
            for hd in 0..vehicle::HEADINGS {
                let fr = v.frame(tilt, hd);
                for y in 0..vehicle::SPR_H {
                    for x in 0..vehicle::SPR_W {
                        let i = fr[y * vehicle::SPR_W + x];
                        if i != 0 {
                            img[(tilt * vehicle::SPR_H + y) * w + hd * vehicle::SPR_W + x] =
                                pal[i as usize];
                        }
                    }
                }
            }
        }
        let path = out.join(format!("car-{}.png", v.id));
        render::save_png(&path, w, h, &img).map_err(|e| e.to_string())?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// Runs a race with computer drivers in the human seats and saves the screen.
fn render_shot(data: GameData, args: &Args) -> Result<(), String> {
    let out = args.out.clone().ok_or("render needs --out FILE.png")?;
    let mut app = App::new(data, audio::Audio::new(false));
    if args.menu {
        for _ in 0..(args.seconds / game::DT) as usize {
            app.update(&Input::default());
        }
        app.draw();
        return render::save_png(&out, SCREEN_W, SCREEN_H, &app.frame.buf)
            .map_err(|e| e.to_string());
    }
    let track = args.track.saturating_sub(1);
    let players = args.players.clamp(1, 2);
    let mut race = app.build_race(track, players, app.settings.cars, 3, 3, 7)?;
    // Let the CPU drive the human cars too.
    let mut cams = [(0.0, 0.0); 2];
    for (i, cam) in cams.iter_mut().enumerate() {
        if let Some(e) = race
            .entrants
            .iter()
            .find(|e| e.driver == game::Driver::Human(i))
        {
            *cam = e.car.screen_pos();
        }
    }
    let frames = (args.seconds / game::DT) as usize;
    let mut frame = render::Frame::new();
    for _ in 0..frames {
        let controls: Vec<_> = race
            .entrants
            .iter_mut()
            .filter(|e| matches!(e.driver, game::Driver::Human(_)))
            .map(|e| e.ai.drive(&e.car, &race.track, e.next_gate, game::DT, true))
            .collect();
        race.step(&controls);
        app::draw_race(&mut frame, &app.data, &race, &mut cams, players);
    }
    render::save_png(&out, SCREEN_W, SCREEN_H, &frame.buf).map_err(|e| e.to_string())?;
    let n = race.track.gates.len();
    for &i in &race.standings() {
        let e = &race.entrants[i];
        println!(
            "{:<10} laps {} gates {} best {:?}",
            e.name,
            e.laps_done(n),
            e.passed,
            e.best_lap
        );
    }
    println!("wrote {}", out.display());
    Ok(())
}

/// Races four computer cars on every track and reports how they got on:
/// a quick health check for the track decoding and the AI.
fn bench(data: GameData, args: &Args) -> Result<(), String> {
    let app = App::new(data, audio::Audio::new(false));
    let frames = (args.seconds / game::DT) as usize;
    println!(
        "{:<10} {:>5} {:>8} {:>9} {:>8}",
        "TRACK", "GATES", "LAPS", "BEST LAP", "ON ROAD"
    );
    for ti in 0..app.data.track_list().len() {
        let mut race = app.build_race(ti, 0, [0, 0], 4, 99, ti as u32 + 1)?;
        let (mut road, mut samples) = (0usize, 0usize);
        for _ in 0..frames {
            race.step(&[]);
            for e in &race.entrants {
                road += e.car.on_road as usize;
                samples += 1;
            }
        }
        let n = race.track.gates.len();
        let laps: Vec<String> = race
            .entrants
            .iter()
            .map(|e| e.laps_done(n).to_string())
            .collect();
        let best = race
            .entrants
            .iter()
            .filter_map(|e| e.best_lap)
            .fold(f32::MAX, f32::min);
        let best = if best == f32::MAX {
            "-".to_string()
        } else {
            app::fmt_time(best)
        };
        println!(
            "{:<10} {:>5} {:>8} {:>9} {:>7.0}%",
            race.track.name,
            n,
            laps.join(","),
            best,
            100.0 * road as f32 / samples as f32
        );
    }
    Ok(())
}

fn trace(data: GameData, args: &Args) -> Result<(), String> {
    use data::track;
    let out = args.out.clone().ok_or("trace needs --out FILE.png")?;
    let app = App::new(data, audio::Audio::new(false));
    let mut race = app.build_race(args.track.saturating_sub(1), 0, [0, 0], 4, 99, 1)?;
    let mut img = race.track.rgb();
    let plot = |img: &mut Vec<u32>, wx: f32, wy: f32, c: u32| {
        let (x, y) = track::to_screen(wx, wy);
        if track::on_screen(x, y, 0.0) {
            img[y as usize * track::W + x as usize] = c;
        }
    };
    for (gi, g) in race.track.gates.iter().enumerate() {
        let c = if gi == 0 { 0xffff00 } else { 0xff2020 };
        for i in 0..=64 {
            let k = i as f32 / 64.0;
            plot(
                &mut img,
                g.a.0 + (g.b.0 - g.a.0) * k,
                g.a.1 + (g.b.1 - g.a.1) * k,
                c,
            );
        }
    }
    const TRAIL: [u32; 4] = [0x00ffff, 0xff00ff, 0x40ff40, 0xffffff];
    for _ in 0..(args.seconds / game::DT) as usize {
        race.step(&[]);
        for (i, e) in race.entrants.iter().enumerate() {
            plot(&mut img, e.car.x, e.car.y, TRAIL[i % 4]);
        }
    }
    let n = race.track.gates.len();
    for e in &race.entrants {
        println!(
            "{:<8} laps {} next gate {}",
            e.name,
            e.laps_done(n),
            e.next_gate
        );
    }
    render::save_png(&out, track::W, track::H, &img).map_err(|e| e.to_string())
}

fn play(data: GameData, args: &Args) -> Result<(), String> {
    let scale = match args.scale {
        1 => Scale::X1,
        4 => Scale::X4,
        _ => Scale::X2,
    };
    let opts = WindowOptions {
        scale,
        resize: true,
        scale_mode: ScaleMode::AspectRatioStretch,
        ..WindowOptions::default()
    };
    let mut window = Window::new("Skids - Super Skidmarks revival", SCREEN_W, SCREEN_H, opts)
        .map_err(|e| e.to_string())?;
    // PAL Amigas ran at 50 frames per second; the simulation is tied to it.
    window.set_target_fps(50);
    let mut app = App::new(data, audio::Audio::new(!args.mute));
    while window.is_open() && !app.quit {
        let input = Input {
            held: window.get_keys(),
            pressed: window
                .get_keys_pressed(KeyRepeat::Yes)
                .into_iter()
                .filter(|&k| k != Key::Unknown)
                .collect(),
        };
        app.update(&input);
        app.draw();
        window
            .update_with_buffer(&app.frame.buf, SCREEN_W, SCREEN_H)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
