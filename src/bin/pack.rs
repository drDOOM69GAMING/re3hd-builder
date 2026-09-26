use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAGIC: [u8; 8] = *b"re3hdPLD";
const NAME_FIELD: usize = 96;

/// Windows refuses to load a PE image whose file size reaches 4 GiB (that is
/// why the single-file pack of the RE3 mods was rejected at launch). Keep
/// every output file comfortably below the ceiling and spill the rest into
/// `-partN.exe` carriers, FitGirl-repack style.
///
/// GitHub additionally caps a single release/upload file at ~1.90 GB, so the
/// budget is kept well below that too: every carrier stays under 1.80 GB.
const PART_BUDGET: u64 = 1_800_000_000; // ~1.68 GiB, under GitHub's 1.90 GB/file cap

/// A single archive can be larger than the whole per-file budget (the Team X
/// RE3 mod is ~1.88 GiB). Such a payload is chopped into `<name>.p1`, `.p2`...
/// segments that each fit inside a fresh carrier, and the app stitches them
/// back together on first run. A little headroom is left so the carrier stub
/// plus the index still land under the budget.
const SPLIT_CHUNK: u64 = PART_BUDGET - 2_000_000;

struct Entry {
    name: String,
    path: PathBuf,
}

/// One contiguous slice of a payload file, ready to be appended to a carrier.
struct Seg {
    name: String,
    start: u64,
    len: u64,
}

fn entry(path: &str, name: Option<&str>) -> Entry {
    let pb = PathBuf::from(path);
    let n = match name {
        Some(n) => n.to_string(),
        None => pb
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .expect("no filename"),
    };
    Entry { name: n, path: pb }
}

/// Split a payload that cannot fit inside one carrier into `.pN` segments.
/// Ordinary payloads are returned as a single unsplit segment.
fn segments_for(e: &Entry, len: u64) -> Vec<Seg> {
    if len <= SPLIT_CHUNK {
        return vec![Seg {
            name: e.name.clone(),
            start: 0,
            len,
        }];
    }
    let mut out = Vec::new();
    let mut off = 0u64;
    let mut i = 1u32;
    while off < len {
        let n = SPLIT_CHUNK.min(len - off);
        out.push(Seg {
            name: format!("{}.p{}", e.name, i),
            start: off,
            len: n,
        });
        off += n;
        i += 1;
    }
    out
}

fn write_index(out: &mut File, records: &[(String, u64, u64)]) {
    for (name, offset, size) in records {
        let name_bytes = name.as_bytes();
        assert!(name_bytes.len() <= NAME_FIELD, "name too long: {name}");
        out.write_all(&(name_bytes.len() as u32).to_le_bytes()).unwrap();
        out.write_all(&offset.to_le_bytes()).unwrap();
        out.write_all(&size.to_le_bytes()).unwrap();
        let mut pad = vec![0u8; NAME_FIELD];
        pad[..name_bytes.len()].copy_from_slice(name_bytes);
        out.write_all(&pad).unwrap();
    }
    out.write_all(&(records.len() as u32).to_le_bytes()).unwrap();
    out.write_all(&MAGIC).unwrap();
}

fn main() {
    let mut args = std::env::args().skip(1);
    let exe = args
        .next()
        .expect("usage: pack.exe <input-exe> <output-name> [music-dir]");
    let out_path = args
        .next()
        .expect("usage: pack.exe <input-exe> <output-name> [music-dir]");
    let music_dir = args.next();

    let mut entries: Vec<Entry> = Vec::new();
    // Order matters for where things land: 7z engine + menu music first so they
    // always ride inside the main exe, then the 7 mods in load order.
    entries.push(entry(r"tools\7z.exe", Some("7z.exe")));
    entries.push(entry(r"tools\7z.dll", Some("7z.dll")));

    if let Some(mdir) = music_dir {
        let mut music: Vec<PathBuf> = std::fs::read_dir(&mdir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let l = p
                            .extension()
                            .map(|x| x.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                        l == "xm" || l == "it" || l == "mod" || l == "s3m"
                    })
                    .collect()
            })
            .unwrap_or_default();
        music.sort();
        println!("[pack] embedding {} menu music tracks from {}", music.len(), mdir);
        for m in music {
            entries.push(entry(m.to_str().unwrap(), None));
        }
    }

    entries.push(entry(
        r"C:\Users\wayneamd\Desktop\mods\1\Resident_Evil_3_HD_mod_v20220716_3.zip",
        None,
    ));
    entries.push(entry(
        r"C:\Users\wayneamd\Desktop\mods\2\RE3_SHDP_2.0_update_for_TeamX_HD_patch.zip",
        None,
    ));
    entries.push(entry(
        r"C:\Users\wayneamd\Desktop\mods\3\RE-ENHANCE_RE3_v2.2.zip",
        None,
    ));
    entries.push(entry(
        r"C:\Users\wayneamd\Desktop\mods\4\Clean epilogues (AI upscaled)-58-1-0-1751066053.zip",
        None,
    ));
    entries.push(entry(
        r"C:\Users\wayneamd\Desktop\mods\5\re3cr-2026-08-16.zip",
        None,
    ));
    entries.push(entry(
        r"C:\Users\wayneamd\Desktop\mods\6\bh3 1.1.0.7z",
        None,
    ));
    entries.push(entry(
        r"C:\Users\wayneamd\Desktop\mods\7\zmovie.7z",
        None,
    ));
    // Seed re3cr config.ini so the build boots with the HD texture path already
    // enabled instead of the defaults the DLL writes on first launch. The fixed
    // name keeps the embedded resolver's "config.ini" match unambiguous.
    entries.push(entry(
        r"C:\Users\wayneamd\Documents\re3hd-builder\assets\config.ini",
        None,
    ));

    // Small valid PE used as the base of every `-partN.exe` carrier.
    let stub = PathBuf::from(r"target\release\partstub.exe");

    for e in &entries {
        if !e.path.is_file() {
            eprintln!("[pack] MISSING payload: {}", e.path.display());
            std::process::exit(1);
        }
    }
    if !stub.is_file() {
        eprintln!("[pack] MISSING stub: {} (run `cargo build --release` first)", stub.display());
        std::process::exit(1);
    }

    let out = PathBuf::from(&out_path);
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut part_index = 1usize;
    let mut cur = out.clone();
    let mut cur_size: u64;
    let mut cur_records: Vec<(String, u64, u64)> = Vec::new();
    let mut by_file: BTreeMap<PathBuf, u64> = BTreeMap::new();

    // Part 1 (main output) = a copy of the app itself.
    let mut dest = File::create(&cur).expect("cannot create output");
    {
        let mut src = File::open(&exe).expect("cannot open input exe");
        io::copy(&mut src, &mut dest).expect("cannot copy exe");
    }
    cur_size = dest.metadata().expect("cannot stat output").len();
    println!("[pack] part 1 -> {} (app {} MiB)", cur.display(), cur_size / 1048576);

    for e in &entries {
        let len = e.path.metadata().expect("cannot stat payload").len();
        let segs = segments_for(e, len);
        if segs.len() > 1 {
            println!(
                "  [split] {} is {} - cutting into {} segments of <= {} MiB",
                e.name,
                format_bytes(len),
                segs.len(),
                SPLIT_CHUNK / 1048576
            );
        }

        for seg in segs {
            if cur_size + seg.len > PART_BUDGET && !cur_records.is_empty() {
                write_index(&mut dest, &cur_records);
                dest.flush().unwrap();
                drop(dest);
                println!("[pack] closing part {part_index} ({})", cur.display());
                part_index += 1;
                cur = out.with_file_name(format!("{stem}-part{part_index}.exe"));
                dest = File::create(&cur).expect("cannot create part");
                let mut s = File::open(&stub).expect("cannot open stub");
                io::copy(&mut s, &mut dest).expect("cannot copy stub");
                // No need to record the stub size: `cur_records` is empty so the
                // budget check above short-circuits, and `cur_size` is refreshed
                // from the stream position once the segment lands.
                cur_records.clear();
                println!("[pack] part {part_index} -> {}", cur.display());
            }

            let start = dest.stream_position().expect("cannot tell");
            let mut f = File::open(&e.path).expect("cannot open payload");
            if seg.start > 0 {
                f.seek(SeekFrom::Start(seg.start)).expect("cannot seek payload");
            }
            let mut limited = f.take(seg.len);
            let copied = io::copy(&mut limited, &mut dest).expect("cannot append payload");
            if copied != seg.len {
                panic!(
                    "short read on {} segment {}: {copied} of {}",
                    e.name, seg.name, seg.len
                );
            }
            cur_records.push((seg.name.clone(), start, seg.len));
            *by_file.entry(cur.clone()).or_default() += seg.len;
            cur_size = start + seg.len;
            println!(
                "  + {:>9} {:.1} MiB  {}",
                format_bytes(seg.len),
                seg.len as f32 / 1048576.0,
                seg.name
            );
        }
    }
    write_index(&mut dest, &cur_records);
    dest.flush().unwrap();
    drop(dest);

    let files = by_file.len();
    let total: u64 = by_file.values().sum();
    for (file, size) in &by_file {
        println!(
            "[pack] {} = {:.2} GiB payload",
            file.display(),
            *size as f32 / 1073741824.0
        );
    }
    println!(
        "[pack] done. {files} output file(s), total payload {:.2} GiB",
        total as f32 / 1073741824.0
    );
}

fn format_bytes(n: u64) -> String {
    if n >= 1 << 30 {
        format!("{:.2} GiB", n as f32 / 1073741824.0)
    } else {
        format!("{:.1} MiB", n as f32 / 1048576.0)
    }
}

#[allow(dead_code)]
fn _uses_path(_p: &Path) {}