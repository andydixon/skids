//! Embeds the editable game assets in the executable so it runs on its own.
//!
//! Every file under `assets/` (or `SKIDS_ASSETS_DIR`) is packed into
//! `$OUT_DIR/embedded.bin`, which `src/data/embedded.rs` includes. Edit the
//! assets and rebuild to change the game. If the folder is missing the pack
//! is empty and the game asks for `--data`. Disable embedding with
//! `--no-default-features --features audio`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn collect(dir: &Path, rel: &str, out: &mut Vec<(String, Vec<u8>)>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let rel = format!("{rel}/{name}");
        if path.is_dir() {
            collect(&path, &rel, out);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
            out.push((rel, fs::read(&path).unwrap()));
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SKIDS_ASSETS_DIR");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("embedded.bin");

    let mut files = Vec::new();
    if env::var_os("CARGO_FEATURE_EMBED").is_some() {
        let dir = env::var_os("SKIDS_ASSETS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets")
            });
        collect(&dir, "assets", &mut files);
        if files.is_empty() {
            println!(
                "cargo:warning=no assets in {}; the game will need --data (run `skids extract` to create them)",
                dir.display()
            );
        }
    }

    // Pack: "SKIDPAK1", u32 count, then per file u16 name length, name,
    // u32 data length, data. All little-endian.
    let mut pack = b"SKIDPAK1".to_vec();
    pack.extend_from_slice(&(files.len() as u32).to_le_bytes());
    for (name, data) in &files {
        pack.extend_from_slice(&(name.len() as u16).to_le_bytes());
        pack.extend_from_slice(name.as_bytes());
        pack.extend_from_slice(&(data.len() as u32).to_le_bytes());
        pack.extend_from_slice(data);
    }
    fs::write(out, pack).unwrap();
}
