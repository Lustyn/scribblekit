# ScribbleKit

A modding toolkit for the Scribblenauts games. It includes:
* lossless, human-readable codecs for every asset format;
* a command-line tool to unpack, decode, encode and repack game files;
* **Scribble Studio**, a desktop object viewer/editor and dictionary editor;
* an installer that brings the Wii U-exclusive content to the PC release.

| Game | Platform | Status |
|---|---|---|
| Scribblenauts Unlimited (2012) | PC (Steam) | All 30,935 resources decode to JSON and re-encode byte-for-byte |
| Scribblenauts Unlimited (2012) | Wii U | Unpacks and decodes (all but 5 menu layouts); its Nintendo content can be [ported to PC](#wii-u-content) |
| Scribblenauts Unmasked (2013) | PC | Planned: same engine family |

> ScribbleKit is an unofficial fan project, not affiliated with or endorsed by 5th Cell,
> Warner Bros. Games, Nintendo or DC. It contains no game files or assets: every tool works on
> files from your own copy of the game.

## Building

You need a recent stable [Rust](https://rustup.rs) (edition 2024).

```sh
cargo build --release          # target/release/: scribble, scribble-studio, wiiu-port-installer
cargo test --workspace         # round-trips every resource of ./extracted if present
cargo build --release --target x86_64-pc-windows-gnu    # Windows binaries from macOS/Linux (needs mingw-w64)
```

## Quick start

```sh
# 1. Get the game files: copy your Steam install, or download the Windows depot with
#    DepotDownloader (https://github.com/SteamRE/DepotDownloader; your account must own app 218680)
DepotDownloader -app 218680 -os windows -remember-password -qr -dir game

# 2. Unpack the .p packs into loose files + manifest.json
cargo run --release -p scribble-cli -- unpack game extracted

# 3a. Browse and edit in the GUI
cargo run --release -p scribble-studio -- extracted

# 3b. …or work with readable text files
cargo run --release -p scribble-cli -- decode-all extracted decoded   # binary -> JSON / standard files
#     edit decoded/**.json
cargo run --release -p scribble-cli -- encode-all decoded extracted   # JSON -> binary

# 4. Rebuild the game's packs and copy them over the install
cargo run --release -p scribble-cli -- pack extracted out
cp out/* game/
```

`scribble check extracted` verifies that every resource round-trips byte-for-byte
(decode → JSON text → encode), and `scribble formats` lists the codecs.

In a decoded tree, game formats become `<file>.json` (or `<file>.<codec>.json` where the path
alone doesn't identify the format, e.g. `…$textboxes.text_table.json`), standard formats keep
their usual extension, and `names.json` holds the id → name tables (object/adjective taxonomy,
tags, merits) that the JSON uses, so references like `"mammal/large/hooved/cow"` resolve when
encoding. Single files: `scribble decode <file> --root extracted`, `scribble encode <json> -o <file>
--root decoded`.

## What's covered

Every one of the 30,935 packed resources of the PC release is handled:

| Kind | Formats | Representation |
|---|---|---|
| Objects | `.so` objects, `.sao` background objects, `.sa` adjectives, `.odt` object details | JSON: properties, relations, behaviours/actions, node tree, animation table |
| Art | `.vec` vector art, `.anim` animations | JSON: coloured triangle meshes per part; keyframed part tracks |
| Words | object/adjective/tag dictionaries, jump tables, word-id tables, single-word index, details, `.dtm`, related objects | JSON word lists; derived tables regenerated on encode |
| Levels | `.sod` scenes & scripts, `.tle` tile maps, `.stp` level setup, `.plf` parallax, `.mdb` merits, `.nbtc` collision tiles, `.lvls`, `.dpd`/`.dps` dependency lists | JSON |
| UI & text | localized text tables, event scripts, `.uib` layouts, `.sfb` sprite frames, `.swc`, `.stl`, `.uit`, fast-travel map, credits | JSON |
| Effects & audio | `.gps` particle systems, `.gec` effects, `.trns` transitions, `.exf` filters, `.aaf` audio metadata (protobuf, via `prost`) | JSON |
| Standard | DDS textures, Bink Audio, RIFF WAV, PNG, PSD, HLSL, Scaleform GFx, DLLs | exported unchanged under their usual extension |

The pack container (`.p`, `index.bin`, `pmindex.xml`, `1s`) repacks content-losslessly
(identical resource bytes; zlib streams are recompressed).

Readable JSON conventions: resource indices print as logical paths (`"data\\_game\\...\\cow.so"`),
taxonomy ids as names (`"mammal/large/hooved/cow"`, `"food/nutsgrains/*/*"`), fixed-point numbers
as exact decimals (positions in world pixels), enums and flag sets by name, and behaviours/actions
with the names the game's own object editor uses. See
[docs/format-guide.md](docs/format-guide.md) for conventions and engine notes; each codec documents
its binary layout in its doc comments. [docs/evidence/](docs/evidence/) cites the engine code (or
game text) behind every field name, enum and flag, marking the few names that can only be read
from the data.

## Wii U content

The Wii U release has the Nintendo easter eggs (Mario, Link, Yoshi, the Super Star, …) that the PC
release shipped without. `wiiu-port-installer` adds them to a PC install, built from your own
decrypted Wii U dump (no Nintendo assets are distributed); `scribble unpack-wiiu` unpacks the Wii U
build for the other tools. See [docs/wiiu.md](docs/wiiu.md) for the format differences, exactly
what the port installs, and the Wii U-only engine behaviour it cannot bring over.

```sh
cargo build --release --target x86_64-pc-windows-gnu -p scribble-wiiu   # -> wiiu-port-installer.exe
wiiu-port-installer --game <PC game folder> --wiiu <Wii U dump>        # or run it and follow the prompts
```

## Scribble Studio

* **Objects** — every spawnable object listed by the words that spawn it; assembled from its
  vector art and mesh parts, animated with any of its animation slots, with physics shapes and
  hotspots overlays; a property tree / JSON editor with live preview; Save writes the `.so`.
* **Dictionary** — per-language word lists: rename, add and delete words, change what a word
  spawns, edit choice labels, costs and gender; Save regenerates all eleven related tables.
* **Resources** — browse any resource: textures, vector art, and a JSON editor for every codec.
* **Export game packs** rebuilds the `.p` files from the workspace.

## Layout

```
crates/scribble-core      binary reader/writer, Format/Codec traits, readable JSON, test harness
crates/scribble-pack      .p packs, index.bin, pmindex.xml, 1s
crates/scribble-formats   maps resource paths to codecs
crates/fmt-*              one crate per format family (object, dictionary, vec, anim, map, ui, effects, common)
crates/scribble-cli       the `scribble` command
crates/scribble-studio    the GUI (egui)
crates/scribble-wiiu      the Wii U build: reading it, and porting its content to PC (wiiu-port-installer)
docs/                     format guide, Wii U notes, and the evidence behind every field name
re/ghidra_scripts/        the Ghidra script that exports the decompilation used for reverse engineering
```

## Reverse engineering

Field names and meanings come from the game's code. [docs/evidence/](docs/evidence/) cites the
function (`FUN_xxxxxxxx` addresses in `Scribble.exe`) behind each one. The decompilation itself
is derived from the game and is not in this repository. Regenerate it from your own copy with
[Ghidra](https://ghidra-sre.org):

```sh
analyzeHeadless re/project scribble -import game/Scribble.exe \
    -scriptPath re/ghidra_scripts -postScript ExportAll.java re
# -> re/decompiled.c (grep for `// ==== FUN_xxx @ addr`) and re/strings.txt
```

## Contributing

Issues and pull requests are welcome. New codecs follow [docs/format-guide.md](docs/format-guide.md):
* every file the game ships must round-trip byte-for-byte (`scribble check`);
* the JSON should describe meaning, not bytes;
* field names should cite their evidence.

Please don't attach game files or decompiled code to issues or pull requests.

## License

[MIT](LICENSE). Scribblenauts and all game content belong to their respective owners.
