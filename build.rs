//! Embeds the game data in the executable so it runs without the disks.
//!
//! The ADF images next to Cargo.toml (or in `SKIDS_EMBED_DIR`) are unpacked
//! at build time and the files inside their LHA archives are packed into
//! `$OUT_DIR/embedded.bin`, which `src/data/embedded.rs` includes. If no
//! images are found the pack is empty and the game asks for `--data`.
//! Disable with `--no-default-features --features audio`.

#[path = "src/formats/mod.rs"]
#[allow(dead_code)]
mod formats;

use formats::{adf::Adf, lha};
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SKIDS_EMBED_DIR");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("embedded.bin");

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if env::var_os("CARGO_FEATURE_EMBED").is_some() {
        let dir = env::var_os("SKIDS_EMBED_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()));
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut images: Vec<PathBuf> = fs::read_dir(&dir)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        images.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("adf")));
        images.sort();
        for image in images {
            println!("cargo:rerun-if-changed={}", image.display());
            let name = image.display().to_string();
            let disk = match fs::read(&image)
                .map_err(|e| e.to_string())
                .and_then(|d| Adf::open(d).map_err(|e| e.to_string()))
                .and_then(|a| a.files().map_err(|e| e.to_string()))
            {
                Ok(f) => f,
                Err(e) => {
                    println!("cargo:warning=skipping {name}: {e}");
                    continue;
                }
            };
            for f in disk {
                if !f.path.to_lowercase().ends_with(".lha") {
                    files.push((f.path, f.data));
                    continue;
                }
                match lha::parse(&f.data) {
                    Ok(entries) => {
                        for e in entries {
                            if let Some(d) = e.data {
                                files.push((e.path, d));
                            }
                        }
                    }
                    Err(e) => println!("cargo:warning=skipping {name}/{}: {e}", f.path),
                }
            }
        }
        if files.is_empty() {
            println!(
                "cargo:warning=no Super Skidmarks ADF images in {}; the game will need --data",
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
