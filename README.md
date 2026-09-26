# RE3HD Builder

![Rust](https://img.shields.io/badge/rust-stable-orange?style=flat-square&logo=rust)
![Platform](https://img.shields.io/badge/platform-Windows%20x64-0078D4?style=flat-square&logo=windows)
![GUI](https://img.shields.io/badge/gui-egui-2A2A2A?style=flat-square)
![Build](https://img.shields.io/badge/build-verified-brightgreen?style=flat-square)
![RE3](https://img.shields.io/badge/game-Resident%20Evil%203%20(1999)-8B0000?style=flat-square)

A one click builder that turns a classic **Resident Evil 3** disc image into a fully
modded HD install. Pick your disc image, press READY, and it produces a playable
`RE3HD` folder with HD textures, restored cutscenes, widescreen support and
re-encoded movies.

No modding tools, no manual file copying, no installing anything.

## What you get

* HD textures for every environment, character and item
* Clean AI upscaled epilogues
* The Classic REbirth engine patches and DLLs
* The bh3 widescreen engine patch
* 720x540 H.264 movies
* A ready to run folder, roughly 2.5 GB

## Download

The release ships as **four files** because of GitHub's 2 GB per asset limit.
Download all four and keep them in the same folder with their original names.

| File | Size | Purpose |
| --- | --- | --- |
| `re3hd-builder-packed.exe` | 23 MB | the program, plus the smaller payloads |
| `re3hd-builder-packed-part2.exe` | 1.68 GB | continuation |
| `re3hd-builder-packed-part3.exe` | 1.27 GB | continuation |
| `re3hd-builder-packed-part4.exe` | 1.63 GB | continuation |

The first file is the program itself. The other three must sit next to it.
Rename nothing. The builder finds its siblings automatically.

## How to use

1. Download all four files into one folder.
2. Run `re3hd-builder-packed.exe`.
3. Select your Resident Evil 3 disc image (`.iso` or `.bin`/`.cue`).
4. Press **READY**.
5. Wait for **BUILD COMPLETE**.
6. The finished game is in an `RE3HD` folder sitting next to your disc image.

The first run unpacks about 4.6 GB of archives into your temp folder, so allow
roughly 10 GB of free space while it works. A cold build takes a few minutes.
Later runs reuse the unpacked cache and are much faster.

## The eleven step pipeline

1. Extract game disc image
2. Isolate data / build RE3HD
3. Install re3cr config.ini
4. Texture pack 1: Team X HD
5. Texture pack 2: Seamless HD 2.0
6. Texture pack 3: RE-ENHANCE
7. Clean epilogues (AI upscaled)
8. Classic REbirth DLLs
9. bh3 1.1.0 engine patch
10. zmovie high quality movies
11. Final check

Step 2 renames the disc's `data` folder to `RE3HD` and discards everything else
on the disc. Step 3 installs the re3cr configuration before any mod touches the
folder, so later steps can rely on it.

## The seven mods

Applied in load order, contents overwrite. Later mods win on shared paths.

| # | Mod | Role |
| --- | --- | --- |
| 1 | `Resident_Evil_3_HD_mod_v20220716_3.zip` (Team X HD) | base HD texture set |
| 2 | `RE3_SHDP_2.0_update_for_TeamX_HD_patch.zip` | Seamless HD patch for Team X |
| 3 | `RE-ENHANCE_RE3_v2.2.zip` | RE-ENHANCE texture pass |
| 4 | `Clean epilogues (AI upscaled)` | AI upscaled ending art |
| 5 | `re3cr-2026-08-16.zip` | Classic REbirth DLLs and config |
| 6 | `bh3 1.1.0.7z` | widescreen engine patch |
| 7 | `zmovie.7z` | 720x540 H.264 movies |

Mods 1, 2 and 3 are three versions of the **same** `hires` texture set rather
than three separate packs. Between them they hold about 13,500 files, but 8,395
of those share a path with a later mod and get replaced. The finished build keeps
5,111 unique texture files, not 13,500. That is why a 4.6 GB set of archives
correctly produces a 2.5 GB game folder.

RE-ENHANCE is mod 3, so it overwrites Team X and Seamless HD wherever all three
overlap. Team X fills the gaps.

Mod 4 is the one special case. Its archive wraps everything in a
`Clean epilogues (AI upscaled)` folder, and only the nested `hires` tree is
merged. `BONUS`, `OPTIONAL` and the readme are intentionally left out.

## Please read: config.ini and input bindings

The builder installs a `config.ini` for the re3cr patch. It is shipped
byte for byte as the author created it, which includes three lines that are
specific to **their** hardware and controller:

```
Device_ID = ...
Key_Def = ...
Joy_Def_DualShockr4_(V2) = ...
```

**If the keyboard or controller does not respond in game, this is why.**
Delete `config.ini` from the `RE3HD` folder, configure your controls in game,
then set `HDTextures = 1` under `[DLL]` to turn the HD textures back on.

## Rebuilding over an existing install

If an `RE3HD` folder already exists, pressing READY asks
*"Would you really like to recreate the project again?"* before it deletes
anything. Choose NO and your existing build is left untouched.

## Requirements

* Windows 10 or 11, 64 bit
* A copy of Resident Evil 3 (1999) as a disc image
* About 10 GB of free space while building

## Building from source

```
git clone https://github.com/drDOOM69GAMING/re3hd-builder.git
cd re3hd-builder
cargo build --release
```

Requires a Rust toolchain and 7-Zip compatible tools in `tools/`.

| Path | Purpose |
| --- | --- |
| `src/app.rs` | the egui interface |
| `src/pipeline.rs` | the eleven build steps |
| `src/embedded.rs` | finds and stitches the embedded payloads |
| `src/bin/pack.rs` | builds the packed release carriers |
| `src/bin/headless.rs` | end to end test harness |
| `assets/config.ini` | the shipped re3cr config |
| `tools/7z.exe`, `tools/7z.dll` | 7-Zip, used to unpack the mod archives |
| `music/` | 100 chiptune tracks for the builder's player |

## Credits

Mods and patches are the work of their respective authors. This builder only
packages and applies them. Please support the original projects:

* Team X HD
* Seamless HD
* RE-ENHANCE
* Clean epilogues (AI upscaled)
* Classic REbirth / re3cr
* bh3
* zmovie

The 100 background tracks are chip music by their various authors, credited in
the file names under `music/`.
