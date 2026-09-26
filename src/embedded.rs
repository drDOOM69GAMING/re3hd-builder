use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::pipeline::ModPaths;

const MAGIC: [u8; 8] = *b"re3hdPLD";
const NAME_FIELD: usize = 96;
const RECORD_SIZE: usize = 4 + 8 + 8 + NAME_FIELD; // len, offset, size, name
const CELL_SIZE: usize = 4 + 8 + 8; // len + offset + size

#[derive(Debug, Clone)]
pub struct Payload {
    pub name: String,
    pub offset: u64,
    pub size: u64,
    /// File the payload lives in (the app exe or a `*-partN.exe` carrier).
    pub source: PathBuf,
}

pub fn embedded_root() -> PathBuf {
    std::env::temp_dir().join("re3hd_embedded")
}

fn read_exact_at<R: Read + Seek + ?Sized>(r: &mut R, pos: u64, buf: &mut [u8]) -> io::Result<()> {
    r.seek(SeekFrom::Start(pos))?;
    r.read_exact(buf)
}

/// Scan one file for an appended payload index. None means "not packed".
fn scan_file(path: &Path) -> Result<Option<Vec<Payload>>, String> {
    let mut f = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;

    let len = f.metadata().map_err(|e| format!("cannot stat {}: {e}", path.display()))?.len();
    if len < (12 + RECORD_SIZE as u64) {
        return Ok(None);
    }

    let mut tail = [0u8; 12];
    read_exact_at(&mut f, len - 12, &mut tail).map_err(|e| format!("index read: {e}"))?;

    // tail layout: [count u32][magic 8 bytes]
    let count = u32::from_le_bytes(tail[0..4].try_into().unwrap()) as usize;
    if tail[4..12] != MAGIC {
        return Ok(None);
    }
    if count == 0 || count > 4096 {
        return Err(format!("corrupt embedded index (bad count) in {}.", path.display()));
    }

    let records_start = len - 12 - (count * RECORD_SIZE) as u64;
    let mut payloads = Vec::with_capacity(count);
    let mut pos = records_start;
    for _ in 0..count {
        let mut cell = [0u8; CELL_SIZE];
        read_exact_at(&mut f, pos, &mut cell).map_err(|e| format!("index read: {e}"))?;
        let name_len = u32::from_le_bytes(cell[0..4].try_into().unwrap()) as usize;
        let offset = u64::from_le_bytes(cell[4..12].try_into().unwrap());
        let size = u64::from_le_bytes(cell[12..20].try_into().unwrap());

        let mut name_buf = vec![0u8; NAME_FIELD];
        read_exact_at(&mut f, pos + CELL_SIZE as u64, &mut name_buf)
            .map_err(|e| format!("index read: {e}"))?;
        pos += RECORD_SIZE as u64;
        if name_len > NAME_FIELD {
            return Err(format!("corrupt embedded index (name too long) in {}.", path.display()));
        }
        let name = String::from_utf8(name_buf[..name_len].to_vec())
            .map_err(|_| format!("corrupt embedded index (bad name) in {}.", path.display()))?;
        payloads.push(Payload { name, offset, size, source: path.to_path_buf() });
    }
    Ok(Some(payloads))
}

/// Scan a given exe and its `*-partN.exe` siblings for appended payload
/// indexes. Returns None when that file was not packed. When packed, the main
/// exe plus every part found next to it are merged into one payload list.
///
/// Windows refuses to load PE images whose file size reaches 4 GiB, so the
/// packer splits payloads across the main exe and a series of `-partN.exe`
/// carriers. All of them share the same tail index format.
pub fn scan_from(exe: &Path) -> Result<Option<Vec<Payload>>, String> {
    let mut payloads = match scan_file(exe)? {
        Some(p) => p,
        None => return Ok(None),
    };

    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    for n in 2u32..=16 {
        let part = dir.join(format!("{stem}-part{n}.exe"));
        if !part.is_file() {
            break;
        }
        match scan_file(&part)? {
            Some(p) => payloads.extend(p),
            None => {
                return Err(format!(
                    "corrupt part file (no payload index): {}",
                    part.display()
                ))
            }
        }
    }
    Ok(Some(payloads))
}

/// Scan the running exe and its `*-partN.exe` siblings. See [`scan_from`].
pub fn scan() -> Result<Option<Vec<Payload>>, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot locate this executable: {e}"))?;
    scan_from(&exe)
}

/// Extract a single payload region from its source file (exe or part) into `dest`.
pub fn extract_payload(payload: &Payload, dest: &Path) -> io::Result<()> {
    let mut f = File::open(&payload.source)?;
    f.seek(SeekFrom::Start(payload.offset))?;
    let mut limited = f.take(payload.size);
    let mut out = File::create(dest)?;
    io::copy(&mut limited, &mut out)?;
    Ok(())
}

pub fn all_cached(payloads: &[Payload]) -> bool {
    let root = embedded_root();
    for p in payloads {
        let base = base_name(&p.name);
        // For a payload that was split across carriers, the file we actually
        // need is the rejoined one, sized as the sum of its segments.
        let expect = joined_size(base, payloads).unwrap_or(p.size);
        let ok = root
            .join(base)
            .metadata()
            .map(|m| m.len() as u64 == expect)
            .unwrap_or(false);
        if !ok {
            return false;
        }
    }
    true
}

/// A payload too large for one carrier is stored as `<name>.p1`, `<name>.p2`,
/// ... Return the real payload name for such a segment (and for anything else,
/// the name unchanged).
fn base_name(name: &str) -> &str {
    if let Some(pos) = name.rfind(".p") {
        let (head, tail) = name.split_at(pos);
        let digits = &tail[2..];
        if !head.is_empty()
            && !digits.is_empty()
            && digits.len() <= 4
            && digits.bytes().all(|b| b.is_ascii_digit())
        {
            return head;
        }
    }
    name
}

/// Total size of a split payload, or None when `base` is not split.
fn joined_size(base: &str, payloads: &[Payload]) -> Option<u64> {
    let segs: Vec<&Payload> = payloads.iter().filter(|p| base_name(&p.name) == base).collect();
    if segs.len() < 2 {
        return None;
    }
    Some(segs.iter().map(|p| p.size).sum())
}

/// Stitch `<name>.pN` segments back into a single `<name>` file in the cache
/// and drop the segment files. Runs after `extract_all`; safe to call on every
/// launch, and a no-op once the joined file is already in place.
pub fn reassemble(payloads: &[Payload]) -> io::Result<usize> {
    let root = embedded_root();

    let mut bases: Vec<String> = payloads
        .iter()
        .map(|p| base_name(&p.name).to_string())
        .collect();
    bases.sort();
    bases.dedup();

    let mut rebuilt = 0usize;
    for base in bases {
        let Some(total) = joined_size(&base, payloads) else {
            continue; // not a split payload
        };
        let mut segs: Vec<&Payload> =
            payloads.iter().filter(|p| base_name(&p.name) == base).collect();
        // Order matters: concatenate strictly by segment number.
        segs.sort_by_key(|p| {
            p.name
                .rfind(".p")
                .and_then(|i| p.name[i + 2..].parse::<u32>().ok())
                .unwrap_or(0)
        });

        let dest = root.join(&base);
        if dest.metadata().map(|m| m.len() as u64 == total).unwrap_or(false) {
            for p in &segs {
                let _ = std::fs::remove_file(root.join(&p.name));
            }
            continue;
        }

        let mut out = File::create(&dest)?;
        for p in &segs {
            let mut src = File::open(root.join(&p.name))?;
            io::copy(&mut src, &mut out)?;
        }
        drop(out);
        for p in &segs {
            let _ = std::fs::remove_file(root.join(&p.name));
        }
        rebuilt += 1;
    }
    Ok(rebuilt)
}

/// Extract all payloads into TEMP/re3hd_embedded, reporting fraction done.
pub fn extract_all(
    payloads: &[Payload],
    on_progress: &mut dyn FnMut(u64, u64), // (bytes_done, bytes_total)
) -> io::Result<()> {
    let root = embedded_root();
    std::fs::create_dir_all(&root)?;

    let byte_total: u64 = payloads.iter().map(|p| p.size).sum();
    let mut done: u64 = 0;

    for p in payloads {
        let dest = root.join(&p.name);
        if dest.metadata().map(|m| m.len() == p.size).unwrap_or(false) {
            done += p.size;
            on_progress(done, byte_total);
            continue;
        }
        extract_payload(p, &dest)?;
        done += p.size;
        on_progress(done, byte_total);
    }
    Ok(())
}

fn slot_for(name: &str) -> Option<usize> {
    let lower = name.to_lowercase();
    if lower.ends_with(".exe") {
        return Some(0); // 7z.exe
    }
    if lower.ends_with(".dll") {
        return Some(1); // 7z.dll
    }
    if lower.ends_with(".xm") || lower.ends_with(".it") || lower.ends_with(".mod") || lower.ends_with(".s3m")
    {
        return Some(10); // menu music
    }
    // The seed re3cr config. Matched by full name so the stock bio3.ini that
    // comes off the disc can never be mistaken for it.
    if lower == "config.ini" {
        return Some(9);
    }
    let heuristics: [(usize, &[&str]); 7] = [
        (2, &["hd_mod"]),   // Resident_Evil_3_HD_mod
        (3, &["shdp"]),     // RE3_SHDP seamless HD patch
        (4, &["enhance"]),  // RE-ENHANCE_RE3 textures
        (5, &["epilogue"]), // clean epilogues (AI upscaled)
        (6, &["re3cr"]),    // classic REbirth DLLs
        (7, &["bh3"]),      // bh3 engine patch
        (8, &["zmovie"]),   // high quality movies
    ];
    heuristics
        .iter()
        .find(|(_, needles)| needles.iter().all(|n| lower.contains(n)))
        .map(|(slot, _)| *slot)
}

pub struct Bundle {
    pub seven: PathBuf,
    pub seven_dll: PathBuf,
    pub mods: Option<ModPaths>,
    pub music: Vec<PathBuf>,
    /// Seed `config.ini` dropped into the build root so the HD textures are
    /// live on first launch. Empty when the release was packed without one.
    pub config: PathBuf,
}

/// Map embedded payloads to a ready-to-use bundle (7z engine + mod paths + music tracks).
pub fn resolve(payloads: &[Payload]) -> Bundle {
    let root = embedded_root();
    let mut seven = PathBuf::new();
    let mut seven_dll = PathBuf::new();
    let mut music: Vec<PathBuf> = Vec::new();
    let mut config = PathBuf::new();
    let mut mods = ModPaths {
        pack1: PathBuf::new(),
        pack2: PathBuf::new(),
        pack3: PathBuf::new(),
        epilogue: PathBuf::new(),
        rebirth: PathBuf::new(),
        bh3: PathBuf::new(),
        zmovie: PathBuf::new(),
    };
    let mut found: Vec<bool> = vec![false; 7];

    for p in payloads {
        // A split payload is matched and addressed by its rejoined name, which
        // is what `reassemble` put back in the cache.
        let base = base_name(&p.name);
        let dest = root.join(base);
        match slot_for(base) {
            Some(0) => seven = dest,
            Some(1) => seven_dll = dest,
            Some(2) => {
                mods.pack1 = dest;
                found[0] = true;
            }
            Some(3) => {
                mods.pack2 = dest;
                found[1] = true;
            }
            Some(4) => {
                mods.pack3 = dest;
                found[2] = true;
            }
            Some(5) => {
                mods.epilogue = dest;
                found[3] = true;
            }
            Some(6) => {
                mods.rebirth = dest;
                found[4] = true;
            }
            Some(7) => {
                mods.bh3 = dest;
                found[5] = true;
            }
            Some(8) => {
                mods.zmovie = dest;
                found[6] = true;
            }
            Some(9) => config = dest,
            Some(10) => music.push(dest),
            _ => {}
        }
    }

    music.sort();
    let complete = found.iter().all(|b| *b) && !seven.as_os_str().is_empty();
    Bundle {
        seven,
        seven_dll,
        mods: if complete { Some(mods) } else { None },
        music,
        config,
    }
}