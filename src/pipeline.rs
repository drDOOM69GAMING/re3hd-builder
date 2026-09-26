use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::archiver;
use crate::fsutil;

pub const STEP_NAMES: [&str; 11] = [
    "Extract game disc image",
    "Isolate data / build RE3HD",
    "Install re3cr config.ini",
    "Texture pack 1 - Team X HD",
    "Texture pack 2 - Seamless HD 2.0",
    "Texture pack 3 - RE-ENHANCE",
    "Clean epilogues (AI upscaled)",
    "Classic REbirth DLLs",
    "bh3 1.1.0 engine patch",
    "zmovie - high quality movies",
    "Final check",
];

#[derive(Debug, Clone)]
pub struct ModPaths {
    pub pack1: PathBuf,    // Resident_Evil_3_HD_mod_v20220716_3.zip
    pub pack2: PathBuf,    // RE3_SHDP_2.0_update_for_TeamX_HD_patch.zip
    pub pack3: PathBuf,    // RE-ENHANCE_RE3_v2.2.zip
    pub epilogue: PathBuf, // Clean epilogues (AI upscaled)-58-1-0-1751066053.zip
    pub rebirth: PathBuf,  // re3cr-2026-08-16.zip
    pub bh3: PathBuf,      // bh3 1.1.0.7z
    pub zmovie: PathBuf,   // zmovie.7z
}

#[derive(Debug, Clone)]
pub enum Event {
    Log(String),
    Phase(usize),
    Progress(usize, f32),
    Status(String),
    Error(String),
    Prep(f32),
    Done,
}

fn send(tx: &Sender<Event>, cx: &egui::Context, ev: Event) {
    let _ = tx.send(ev);
    cx.request_repaint();
}

pub fn beep_triple() {
    // "STARRRS" in International Morse Code: dot(ding)=short, dash(DING)=long.
    // Unit = 100ms. Letters: S ... / T - / A .- / R .-. / R .-. / R .-. / S ...
    const UNIT: u64 = 100;
    let letters: [&[bool]; 7] = [
        &[false, false, false], // S ...
        &[true],                // T -
        &[false, true],         // A .-
        &[false, true, false],  // R .-.
        &[false, true, false],  // R .-.
        &[false, true, false],  // R .-.
        &[false, false, false], // S ...
    ];
    for letter in letters {
        for &is_dash in letter {
            let on_ms = UNIT * if is_dash { 3 } else { 1 };
            unsafe {
                windows_sys::Win32::System::Diagnostics::Debug::Beep(700, on_ms as u32);
            }
            std::thread::sleep(Duration::from_millis(UNIT)); // intra-letter gap
        }
        std::thread::sleep(Duration::from_millis(UNIT * 2)); // makes a 3-unit letter gap
    }
}

fn is_named(p: &Path, names: &[&str]) -> bool {
    p.file_name()
        .map(|n| {
            let n = n.to_string_lossy();
            names.iter().any(|want| n.eq_ignore_ascii_case(want))
        })
        .unwrap_or(false)
}

fn find_named(root: &Path, names: &[&str], max_depth: usize) -> Option<PathBuf> {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if is_named(&p, names) {
                    return Some(p);
                }
                if p.is_dir() && depth < max_depth {
                    stack.push((p, depth + 1));
                }
            }
        }
    }
    None
}

/// The game files live in a folder called "data" on the Resident Evil 3 disc.
fn find_data_folder(stage: &Path) -> Option<PathBuf> {
    find_named(stage, &["data"], 2)
}

/// Everything on the disc that is not the game itself (setup.exe, the `inst`
/// installer folder, AUTORUN.INF, the PDF manual, EULA/readme/disc.id) is not
/// needed once `data` has been lifted out as the RE3HD build.
fn remove_unneeded_files(stage: &Path, keep: &Path) -> usize {
    let mut removed = 0usize;
    if let Ok(rd) = std::fs::read_dir(stage) {
        for e in rd.flatten() {
            let p = e.path();
            if p == keep {
                continue;
            }
            let ok = if p.is_dir() {
                fsutil::remove_dir_all_including_ro(&p).is_ok()
            } else {
                std::fs::remove_file(&p).is_ok()
            };
            if ok {
                removed += 1;
            }
        }
    }
    removed
}

fn work_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("re3hd_build_{}", tag))
}

fn clear_dir(p: &Path) {
    let _ = fsutil::remove_dir_all_including_ro(p);
    let _ = std::fs::create_dir_all(p);
}

/// Merge the *contents* of `src` into `dst`, overwriting existing files.
fn copy_step(
    ctx: &egui::Context,
    tx: &Sender<Event>,
    phase: usize,
    src: &Path,
    dst: &Path,
) -> Result<(), String> {
    let total = fsutil::dir_size(src).unwrap_or(0) as f32;
    let mut last_report = Instant::now();
    let mut copied = 0u64;

    fsutil::copy_tree_contents(src, dst, &mut |bytes: u64| -> bool {
        copied = bytes;
        if last_report.elapsed().as_millis() > 80 {
            let frac = if total > 0.0 {
                (bytes as f32 / total).min(1.0)
            } else {
                1.0
            };
            send(tx, ctx, Event::Progress(phase, frac));
            last_report = Instant::now();
        }
        true
    })
    .map_err(|e| format!("copy failed: {e}"))?;

    send(tx, ctx, Event::Progress(phase, 1.0));
    if total > 0.0 {
        send(
            tx,
            ctx,
            Event::Log(format!(
                "[copy] {:.1} MiB merged into RE3HD",
                copied as f32 / 1048576.0
            )),
        );
    }
    Ok(())
}

fn unpack(
    ctx: &egui::Context,
    tx: &Sender<Event>,
    seven: &Path,
    archive: &Path,
    tmp: &Path,
) -> Result<(), String> {
    send(tx, ctx, Event::Log(format!("[unpack] {}", archive.display())));
    archiver::extract(seven, archive, tmp)
}

pub fn run_pipeline(
    cx: egui::Context,
    tx_event: Sender<Event>,
    iso: PathBuf,
    mods: ModPaths,
    config: PathBuf,
) {
    let tx = tx_event;

    let fail = |msg: String| {
        send(&tx, &cx, Event::Error(msg));
    };

    let seven = match archiver::locate_7z() {
        Some(p) => p,
        None => {
            fail(
                "7-Zip not found. Place 7z.exe + 7z.dll in a 'tools' folder next to this app."
                    .to_string(),
            );
            return;
        }
    };

    send(&tx, &cx, Event::Log(format!("7-Zip engine: {}", seven.display())));

    let iso_dir = match iso.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let stem = iso
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "game".to_string());
    let stage = iso_dir.join(format!(".{}.re3hd_stage", stem));
    let re3hd = iso_dir.join("RE3HD");

    // ---- Phase 0: disc image extraction ----
    send(&tx, &cx, Event::Phase(0));
    send(&tx, &cx, Event::Status("Extracting game disc image...".into()));
    clear_dir(&stage);

    let iso_mb = std::fs::metadata(&iso).map(|m| m.len() as f32 / 1048576.0).unwrap_or(0.0);
    send(&tx, &cx, Event::Log(format!("[iso] extracting {:.0} MiB...", iso_mb)));
    if let Err(e) = unpack(&cx, &tx, &seven, &iso, &stage) {
        fail(e);
        return;
    }
    send(&tx, &cx, Event::Progress(0, 1.0));
    send(&tx, &cx, Event::Phase(1));

    // ---- Phase 1: isolate data -> RE3HD ----
    let data = match find_data_folder(&stage) {
        Some(d) => d,
        None => {
            fail("Could not locate the 'data' folder inside the extracted game disc.".into());
            return;
        }
    };

    if re3hd.exists() {
        send(&tx, &cx, Event::Log("Removing previous RE3HD output folder...".into()));
        if let Err(e) = fsutil::remove_dir_all_including_ro(&re3hd) {
            fail(format!("could not remove existing RE3HD folder: {e}"));
            return;
        }
    }

    send(&tx, &cx, Event::Status("Moving data folder as RE3HD...".into()));
    if let Err(e) = fsutil::move_into_place(&data, &re3hd) {
        fail(format!("could not finalize RE3HD folder: {e}"));
        return;
    }
    send(&tx, &cx, Event::Log("[iso] data renamed to RE3HD".into()));

    // Drop the installer, docs and disc metadata - only the game folder is used.
    let dropped = remove_unneeded_files(&stage, &re3hd);
    if dropped > 0 {
        send(
            &tx,
            &cx,
            Event::Log(format!("[iso] removed {dropped} unneeded disc item(s)")),
        );
    }
    send(&tx, &cx, Event::Progress(1, 1.0));

    if stage.exists() {
        send(&tx, &cx, Event::Log("removing temporary extraction folder...".into()));
        let _ = fsutil::remove_dir_all_including_ro(&stage);
    }

    // ---- Phase 2: drop the re3cr config in before anything else runs ----
    // Classic REbirth writes its own config.ini on first launch, and the
    // defaults it picks do not turn the HD texture path on. Seeding a known
    // good file means the build boots with textures already enabled.
    send(&tx, &cx, Event::Phase(2));
    send(&tx, &cx, Event::Status("Installing re3cr config.ini...".into()));
    if config.as_os_str().is_empty() {
        send(
            &tx,
            &cx,
            Event::Log("[config] no config.ini supplied - RE3 will write its own".into()),
        );
    } else if !config.is_file() {
        fail(format!("config.ini not found at {}", config.display()));
        return;
    } else {
        let dest = re3hd.join("config.ini");
        if let Err(e) = std::fs::copy(&config, &dest) {
            fail(format!("could not install config.ini: {e}"));
            return;
        }
        let bytes = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        send(
            &tx,
            &cx,
            Event::Log(format!("[config] installed config.ini ({bytes} bytes)")),
        );
    }
    send(&tx, &cx, Event::Progress(2, 1.0));

    // ---- Phases 3..9: the seven mod archives, in load order ----
    let packs: [(usize, &str, &PathBuf, &str); 7] = [
        (3, "Team X HD textures", &mods.pack1, "tex1"),
        (4, "Seamless HD 2.0 patch", &mods.pack2, "tex2"),
        (5, "RE-ENHANCE textures", &mods.pack3, "tex3"),
        (6, "Clean epilogues (AI upscaled)", &mods.epilogue, "epilogue"),
        (7, "Classic REbirth DLLs", &mods.rebirth, "rebirth"),
        (8, "bh3 1.1.0 engine patch", &mods.bh3, "bh3"),
        (9, "zmovie videos", &mods.zmovie, "zmovie"),
    ];

    for (p, name, archive, tag) in packs {
        send(&tx, &cx, Event::Phase(p));
        send(&tx, &cx, Event::Status(format!("Integrating {name}...")));
        let tmp = work_dir(tag);
        clear_dir(&tmp);
        if let Err(e) = unpack(&cx, &tx, &seven, archive, &tmp) {
            fail(e);
            return;
        }

        if tag == "epilogue" {
            // This archive wraps everything in a "Clean epilogues (AI upscaled)"
            // folder and also ships a README plus a BONUS/OPTIONAL extras set.
            // Only the nested `hires` folder is wanted, merged into RE3HD\hires.
            match find_named(&tmp, &["hires"], 1) {
                Some(src) => {
                    send(
                        &tx,
                        &cx,
                        Event::Log("[epilogue] merging hires folder into RE3HD\\hires".into()),
                    );
                    if let Err(e) = copy_step(&cx, &tx, p, &src, &re3hd.join("hires")) {
                        fail(e);
                        return;
                    }
                }
                None => {
                    fail(
                        "No 'hires' folder found inside the Clean epilogues archive.".into()
                    );
                    return;
                }
            }
        } else if let Err(e) = copy_step(&cx, &tx, p, &tmp, &re3hd) {
            fail(e);
            return;
        }

        send(&tx, &cx, Event::Progress(p, 1.0));

        let _ = fsutil::remove_dir_all_including_ro(&tmp);
        send(&tx, &cx, Event::Log(format!("[mod] {name} applied")));
    }

    // ---- Phase 10: final check ----
    send(&tx, &cx, Event::Phase(10));
    send(&tx, &cx, Event::Status("All archives applied. Verifying build...".into()));

    let mut found_any = false;

    if re3hd.join("BIOHAZARD(R) 3 PC.exe").is_file() {
        found_any = true;
    }
    if re3hd.join("hires").is_dir() {
        found_any = true;
    }
    if re3hd.join("ddraw.dll").is_file() {
        found_any = true;
    }
    if re3hd.join("bio3hd.asi").is_file() {
        found_any = true;
    }
    if re3hd.join("zmovie").is_dir() {
        found_any = true;
    }
    if re3hd.join("Rofs1.dat").is_file() {
        found_any = true;
    }

    // The config only counts as done if the HD texture switch is actually on,
    // otherwise the build would silently boot with stock textures.
    let cfg = re3hd.join("config.ini");
    if cfg.is_file() {
        if let Ok(text) = std::fs::read_to_string(&cfg) {
            let hd_on = text
                .lines()
                .any(|l| {
                    let l = l.trim();
                    l.eq_ignore_ascii_case("HDTextures = 1")
                });
            if hd_on {
                send(&tx, &cx, Event::Log("[config] verified HDTextures = 1".into()));
            } else {
                send(
                    &tx,
                    &cx,
                    Event::Log("[warn] config.ini has HDTextures disabled".into()),
                );
            }
        }
    } else {
        send(
            &tx,
            &cx,
            Event::Log("[warn] config.ini missing from the build root".into()),
        );
    }

    send(&tx, &cx, Event::Progress(10, 1.0));
    if found_any {
        send(
            &tx,
            &cx,
            Event::Status(format!("BUILD COMPLETE - RE3HD ready at {}", re3hd.display())),
        );
        send(&tx, &cx, Event::Log(format!("[done] output: {}", re3hd.display())));
    } else {
        send(
            &tx,
            &cx,
            Event::Status("Build finished but expected files were not found.".into()),
        );
        send(&tx, &cx, Event::Log("[warn] expected output files missing".into()));
    }
    send(&tx, &cx, Event::Done);

    std::thread::spawn(beep_triple);
}
