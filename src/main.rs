//! Skids: a cross-platform revival of Super Skidmarks (Acid Software, 1995).
//!
//! The engine reads the original Amiga data (ADF images, the LHA archives on
//! them, or extracted files) at run time; no game assets are bundled.

mod app;
mod audio;
mod data;
mod formats;
mod game;
mod music;
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
    skids extract [--data DIR] [--out DIR]        convert the data to editable assets (default ./assets)
    skids unpack  FILE.adf|FILE.lha --out DIR     extract every file from a disk image or archive
    skids music   [--data DIR] --out FILE.wav [--seconds S]
                                                  render the tune to a WAV file
    skids render  [--data DIR] --out FILE.png [--track N] [--seconds S] [--players 1|2] [--menu] [--caravans]
                                                  simulate a race (or the menu) and save a frame
    skids bench   [--data DIR] [--seconds S] [--caravans]
                                                  race computer cars on every track
    skids trace   [--data DIR] --out FILE.png [--track N] [--seconds S]
                                                  draw the computer cars' paths on a track

The game data is built into the executable, so no disks are needed. --data
adds to or overrides it with ADF images, LHA archives, or extracted track/,
vehicles/ and palettes/ folders, or an assets folder of edits. Without
--data, a folder like that in the current directory or next to the
executable is picked up automatically.

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
    caravans: bool,
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
        caravans: false,
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
            "--caravans" => a.caravans = true,
            "info" | "extract" | "render" | "bench" | "trace" | "music" | "play" => a.cmd = arg,
            "unpack" => {
                a.cmd = arg;
                a.data = Some(val("unpack")?.into());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

/// An explicit --data directory, or a nearby folder that looks like it holds
/// game data (e.g. edited assets next to the executable). The built-in data
/// is always loaded first, so finding nothing is fine.
fn find_data(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if explicit.is_some() {
        return explicit;
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
    candidates.into_iter().find(|d| looks_right(d))
}

/// Sound output using the game's samples and tune.
fn make_audio(data: &GameData, enabled: bool) -> audio::Audio {
    let song = data
        .music
        .as_ref()
        .and_then(|(name, d)| match formats::med::parse(d) {
            Ok(s) => Some(std::sync::Arc::new(s)),
            Err(e) => {
                eprintln!("warning: music {name}: {e}");
                None
            }
        });
    audio::Audio::new(enabled, audio::Bank::from_sounds(&data.sounds), song)
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
    if args.cmd == "unpack" {
        return match unpack(args.data.as_deref().unwrap(), args.out.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let root = find_data(args.data.clone());
    // Converting disks must read only the disks, not the built-in assets.
    let data = match if args.cmd == "extract" {
        GameData::load_from(root.as_deref(), false)
    } else {
        GameData::load(root.as_deref())
    } {
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
            info(&data, root.as_deref());
            Ok(())
        }
        "extract" => extract(&data, args.out.as_deref()),
        "render" => render_shot(data, &args),
        "bench" => bench(data, &args),
        "trace" => trace(data, &args),
        "music" => render_music(&data, &args),
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

fn info(data: &GameData, dir: Option<&Path>) {
    let built_in = data::embedded::files().len();
    match (built_in, dir) {
        (0, Some(d)) => println!("Data: {}", d.display()),
        (n, Some(d)) => println!("Data: built in ({n} files), plus {}", d.display()),
        (n, None) => println!("Data: built in ({n} files)"),
    }
    println!("Tracks:");
    for (i, entry) in data.tracks.iter().enumerate() {
        let from = match entry.source {
            data::TrackSource::Asset { .. } => "asset",
            data::TrackSource::Packed { .. } => "original data",
        };
        match data.load_track(i) {
            Ok(t) => println!("  {:<10} {:>2} gates  ({from})", t.name, t.gates.len()),
            Err(e) => println!("  {:<10} ERROR {e}", entry.name),
        }
    }
    println!("Vehicles:");
    for v in &data.vehicles {
        println!("  {:<10} ({})", v.name, v.id);
    }
}

/// Extracts every file from an ADF image or LHA archive, unpacking LHA
/// archives found on disks. Files keep their stored (packed) form.
fn unpack(input: &Path, out: Option<&Path>) -> Result<(), String> {
    use formats::{adf::Adf, lha};
    let out = out.ok_or("unpack needs --out DIR")?;
    let data = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if input
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("adf"))
    {
        let adf = Adf::open(data).map_err(|e| e.to_string())?;
        for f in adf.files().map_err(|e| e.to_string())? {
            files.push((f.path, f.data));
        }
    } else {
        files.push((input.file_name().unwrap().to_string_lossy().into(), data));
    }
    let mut failed = 0;
    while let Some((path, data)) = files.pop() {
        if path.to_lowercase().ends_with(".lha") {
            for e in lha::parse(&data).map_err(|e| format!("{path}: {e}"))? {
                match e.data {
                    Ok(d) => files.push((e.path, d)),
                    Err(why) => {
                        eprintln!("FAILED {} ({}): {why}", e.path, e.method);
                        failed += 1;
                    }
                }
            }
            continue;
        }
        // Keep paths inside `out`.
        let safe: PathBuf = path
            .split(['/', '\\'])
            .filter(|c| !c.is_empty() && *c != "..")
            .collect();
        let dest = out.join(safe);
        std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&dest, &data).map_err(|e| format!("{}: {e}", dest.display()))?;
        println!("{:>8}  {}", data.len(), path);
    }
    if failed > 0 {
        return Err(format!("{failed} files could not be extracted"));
    }
    Ok(())
}

/// Writes every track and vehicle as editable assets, then reads each one
/// back to check the conversion is lossless.
fn extract(data: &GameData, out: Option<&Path>) -> Result<(), String> {
    use data::{Track, Vehicle};
    let out = out.unwrap_or(Path::new("assets"));
    let write = |path: PathBuf, bytes: &[u8]| -> Result<(), String> {
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    };
    for i in 0..data.tracks.len() {
        let t = data.load_track(i)?;
        let dir = out.join("tracks").join(t.name.to_lowercase());
        let (png, txt) = (t.to_png()?, t.to_text());
        let back = Track::from_assets(&t.name, &png, &txt)?;
        let same = back.pixels == t.pixels
            && back.palette == t.palette
            && back.height == t.height
            && back
                .gates
                .iter()
                .zip(&t.gates)
                .all(|(a, b)| a.a == b.a && a.b == b.b)
            && back.gates.len() == t.gates.len();
        if !same {
            return Err(format!("{}: asset does not round-trip", t.name));
        }
        write(dir.join("track.png"), &png)?;
        write(dir.join("track.txt"), txt.as_bytes())?;
        println!("wrote {}", dir.display());
    }
    for v in &data.vehicles {
        let (png, txt) = (v.sheet_png()?, v.settings_text());
        let back = Vehicle::from_assets(&v.id, &png, Some(&txt))?;
        if back.frames != v.frames || back.spec != v.spec || back.name != v.name {
            return Err(format!("{}: asset does not round-trip", v.id));
        }
        let dir = out.join("vehicles");
        write(dir.join(format!("{}.png", v.id)), &png)?;
        write(dir.join(format!("{}.txt", v.id)), txt.as_bytes())?;
        if let Some(menu) = v.menu_png() {
            write(dir.join(format!("{}-menu.png", v.id)), &menu?)?;
        }
        println!("wrote {}", dir.join(&v.id).display());
    }
    for (name, s) in &data.sounds {
        let wav = formats::iff::sound_to_wav(s);
        let back = formats::iff::wav_to_sound(&wav)?;
        if back.data != s.data || back.rate != s.rate || back.loop_start != s.loop_start {
            return Err(format!("sound {name}: WAV does not round-trip"));
        }
        let path = out.join("sounds").join(format!("{name}.wav"));
        write(path.clone(), &wav)?;
        println!("wrote {}", path.display());
    }
    if let Some((name, med)) = &data.music {
        formats::med::parse(med).map_err(|e| format!("music {name}: {e}"))?;
        let path = out.join("music").join(format!("{name}.med"));
        write(path.clone(), med)?;
        println!("wrote {}", path.display());
    }
    for (name, p) in &data.pictures {
        let pal: Vec<data::palette::Rgb> = p
            .palette
            .iter()
            .map(|c| data::palette::Rgb(c.0, c.1, c.2))
            .collect();
        let png = data::image::write_indexed(p.w, p.h, &pal, false, &p.pixels)?;
        if data::image::read_indexed(&png, &pal)?.pixels != p.pixels {
            return Err(format!("picture {name}: PNG does not round-trip"));
        }
        let path = out.join("gfx").join(format!("{name}.png"));
        write(path.clone(), &png)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// Runs a race with computer drivers in the human seats and saves the screen.
fn render_shot(data: GameData, args: &Args) -> Result<(), String> {
    let out = args.out.clone().ok_or("render needs --out FILE.png")?;
    let mut app = {
        let audio = make_audio(&data, false);
        App::new(data, audio)
    };
    app.settings.caravans = args.caravans;
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
    let mut app = {
        let audio = make_audio(&data, false);
        App::new(data, audio)
    };
    app.settings.caravans = args.caravans;
    let frames = (args.seconds / game::DT) as usize;
    println!(
        "{:<10} {:>5} {:>8} {:>9} {:>8}",
        "TRACK", "GATES", "LAPS", "BEST LAP", "ON ROAD"
    );
    for ti in 0..app.data.tracks.len() {
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

fn render_music(data: &GameData, args: &Args) -> Result<(), String> {
    let out = args.out.clone().ok_or("music needs --out FILE.wav")?;
    let (name, d) = data.music.as_ref().ok_or("no music in the data")?;
    let song = std::sync::Arc::new(formats::med::parse(d).map_err(|e| format!("{name}: {e}"))?);
    let rate = 44100u32;
    let mut player = music::Player::new(song, rate as f64);
    let frames = (args.seconds * rate as f32) as usize;
    let mut pcm = Vec::with_capacity(frames * 4);
    let mut peak = 0.0f32;
    let mut wrapped_at = None;
    let mut last_order = 0;
    for i in 0..frames {
        let (l, r) = player.next();
        let order = player.order();
        if order < last_order && wrapped_at.is_none() {
            wrapped_at = Some(i as f32 / rate as f32);
        }
        last_order = order;
        peak = peak.max(l.abs()).max(r.abs());
        for v in [l, r] {
            pcm.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
    }
    let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend(pcm);
    let n = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&n.to_le_bytes());
    std::fs::write(&out, wav).map_err(|e| e.to_string())?;
    println!(
        "wrote {} ({:.0} s, peak {:.2}, position {})",
        out.display(),
        args.seconds,
        peak,
        player.position()
    );
    if let Some(t) = wrapped_at {
        println!(
            "the song loops back to the start after {}:{:04.1}",
            (t / 60.0) as u32,
            t % 60.0
        );
    }
    Ok(())
}

fn trace(data: GameData, args: &Args) -> Result<(), String> {
    use data::track;
    let out = args.out.clone().ok_or("trace needs --out FILE.png")?;
    let mut app = {
        let audio = make_audio(&data, false);
        App::new(data, audio)
    };
    app.settings.caravans = args.caravans;
    // Same seed as `bench` uses for this track, so problems seen there can be traced.
    let mut race = app.build_race(
        args.track.saturating_sub(1),
        0,
        [0, 0],
        4,
        99,
        args.track.max(1) as u32,
    )?;
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
    let mut app = {
        let audio = make_audio(&data, !args.mute);
        App::new(data, audio)
    };
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
