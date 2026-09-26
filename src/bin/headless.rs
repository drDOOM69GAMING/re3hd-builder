use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use re3hd_builder::{embedded, pipeline};
use re3hd_builder::pipeline::{Event, ModPaths};

fn main() {
    // Optional argument: scan that exe + its -partN siblings instead of this
    // binary. Used to validate a real packed release end to end.
    let target: Option<PathBuf> = std::env::args().nth(1).map(PathBuf::from);
    let scanned = match &target {
        Some(p) => embedded::scan_from(p).expect("embedded scan"),
        None => embedded::scan().expect("embedded scan"),
    };
    let iso = PathBuf::from(r"C:\Users\wayneamd\Desktop\BIOHAZARD_3_PC.iso");

    let (mods, config, source_note) = match scanned {
        Some(payloads) => {
            let totals: u64 = payloads.iter().map(|p| p.size).sum();
            if !embedded::all_cached(&payloads) {
                println!("[headless] extracting {:.2} GiB of embedded payloads to temp...", totals as f32 / 1073741824.0);
                embedded::extract_all(&payloads, &mut |done, total| {
                    println!("  prep {:.0}%", done as f32 / total as f32 * 100.0);
                })
                .expect("embedded extraction");
            }
            let stitched = embedded::reassemble(&payloads).expect("payload stitching");
            if stitched > 0 {
                println!("[headless] rejoined {stitched} split payload(s)");
            }
            let bundle = embedded::resolve(&payloads);
            match bundle.mods {
                Some(m) => (m, bundle.config, "embedded payloads".to_string()),
                None => {
                    eprintln!("[headless] embedded payloads present but incomplete");
                    std::process::exit(1);
                }
            }
        }
        None => {
            let m = ModPaths {
                pack1: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\1\Resident_Evil_3_HD_mod_v20220716_3.zip",
                ),
                pack2: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\2\RE3_SHDP_2.0_update_for_TeamX_HD_patch.zip",
                ),
                pack3: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\3\RE-ENHANCE_RE3_v2.2.zip",
                ),
                epilogue: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\4\Clean epilogues (AI upscaled)-58-1-0-1751066053.zip",
                ),
                rebirth: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\5\re3cr-2026-08-16.zip",
                ),
                bh3: PathBuf::from(r"C:\Users\wayneamd\Desktop\mods\6\bh3 1.1.0.7z"),
                zmovie: PathBuf::from(r"C:\Users\wayneamd\Desktop\mods\7\zmovie.7z"),
            };
            let cfg = PathBuf::from(r"C:\Users\wayneamd\Documents\re3hd-builder\assets\config.ini");
            (m, cfg, "direct file paths".to_string())
        }
    };

    println!("[headless] mode: {source_note}");
    println!("[headless] verifying inputs...");
    for (name, p) in [
        ("ISO", &iso),
        ("Team X HD pack", &mods.pack1),
        ("Seamless HD 2.0", &mods.pack2),
        ("RE-ENHANCE", &mods.pack3),
        ("Clean epilogues", &mods.epilogue),
        ("REbirth DLLs", &mods.rebirth),
        ("bh3 engine patch", &mods.bh3),
        ("zmovie videos", &mods.zmovie),
    ] {
        if p.is_file() {
            println!("  OK   {name}: {}", p.display());
        } else {
            println!("  MISS {name}: {}", p.display());
        }
    }

    // `--extract-only` stops after the payloads are out of the carriers. Used to
    // prove a packed release stores every mod without touching the game folder.
    if std::env::args().any(|a| a == "--extract-only") {
        println!("[headless] --extract-only: payloads are staged, not building.");
        for (name, p) in [
            ("Team X HD pack", &mods.pack1),
            ("Seamless HD 2.0", &mods.pack2),
            ("RE-ENHANCE", &mods.pack3),
            ("Clean epilogues", &mods.epilogue),
            ("REbirth DLLs", &mods.rebirth),
            ("bh3 engine patch", &mods.bh3),
            ("zmovie videos", &mods.zmovie),
        ] {
            println!("  {name}\t{}", p.display());
        }
        println!("  config.ini\t{}", config.display());
        return;
    }

    let ctx = eframe::egui::Context::default();
    let (tx, rx) = mpsc::channel();

    let mut last_phase_time = Instant::now();
    let mut current_phase = usize::MAX;
    let total_started = Instant::now();

    let handle = std::thread::spawn(move || {
        pipeline::run_pipeline(ctx, tx, iso, mods, config);
    });

    while let Ok(ev) = rx.recv() {
        match ev {
            Event::Log(l) => println!("  [log] {l}"),
            Event::Phase(p) => {
                if current_phase != usize::MAX {
                    println!(
                        "  phase {} done in {:.1}s",
                        current_phase,
                        last_phase_time.elapsed().as_secs_f32()
                    );
                }
                current_phase = p;
                last_phase_time = Instant::now();
                println!(
                    "=== PHASE {p}: {} ===",
                    pipeline::STEP_NAMES.get(p).copied().unwrap_or("?")
                );
            }
            Event::Progress(p, f) => {
                if p != current_phase {
                    println!("  phase {p} progress {:.0}%", f * 100.0);
                } else if f > 0.01 {
                    println!("  ... {:.0}%", f * 100.0);
                }
            }
            Event::Prep(f) => println!("  [prep] {:.0}%", f * 100.0),
            Event::Status(s) => println!("  [status] {s}"),
            Event::Error(e) => {
                println!("  [ERROR] {e}");
                break;
            }
            Event::Done => {
                println!("=== DONE in {:.1}s ===", total_started.elapsed().as_secs_f32());
                break;
            }
        }
    }

    let _ = handle.join();
}