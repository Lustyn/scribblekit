# Writing a format codec

Every Scribblenauts Unlimited resource format gets a **lossless**, **human-readable** codec.

## Layout

```
crates/scribble-core      Reader/Writer, Fx16, Hex, ResRef, Context, Format trait, json, testing
crates/scribble-pack      .p packs, index.bin, pmindex.xml, 1s (done)
crates/scribble-formats   registry: calls each fmt crate's handler()
crates/fmt-<group>        one crate per family of formats (object, dictionary, vec, anim, map, ui, effects)
crates/scribble-cli       `scribble` binary: unpack/pack/decode/encode/decode-all/encode-all/check
extracted/                every resource unpacked (logical path `a\b\c.ext` -> extracted/a/b/c.ext)
re/decompiled.c           Ghidra decompilation of game/Scribble.exe (grep by `// ==== FUN_xxx @ addr`)
re/strings.txt            defined strings with referencing functions
game/Scribble.exe         32-bit x86 PE (use rizin / capstone for disassembly if decompile is unclear)
```

## Contract

For every file the game ships, `encode(decode(bytes)) == bytes`, and the decoded value survives
a trip through JSON text (`scribble_core::format::assert_roundtrip`). Implement:

```rust
impl Format for MyThing {
    const NAME: &'static str = "ext";
    const DESCRIPTION: &'static str = "one line";
    fn decode(data: &[u8], ctx: &Context) -> Result<Self>;
    fn encode(&self, ctx: &Context) -> Result<Vec<u8>>;
}
```

and claim files in your crate's `handler(path, data)` using `scribble_core::codec!(MyThing)`;
list them in `codecs()`. Only edit files inside your own `crates/fmt-<group>/` directory.

## Human readability — the important part

The JSON is what people will read and edit, so it must describe *meaning*, not bytes:

* Name every field after what it does (`mass`, `is_flammable`, `adjective`, `vertices`), using
  the game code (loaders in `re/decompiled.c`) and the data itself to work it out. Document the
  binary layout in a doc comment on each type, with the byte layout as a code block.
* Use enums (serde `rename_all = "snake_case"`) for enumerations and named bit flags instead of
  raw integers when the meaning is known. Bit flags: serialize as a list of flag names, keeping
  unknown bits as e.g. `"bit_13"` so they stay lossless.
* Resource indices (u32/u16 pmindex indices — very common) should be `ResRef`, which prints the
  referenced resource path (`"data\\_game\\scribbleobjects\\cow.so"`) using `Context`.
* Ids from a named namespace (taxonomy levels, merits, tags, dictionary words) should print as
  names through the `Context` too: `NamedPath` for a path of ids (`"mammal/large/hooved/cow"`),
  `NamedId` for a single id. See *Context and named ids* below.
* 16.16 fixed-point values -> `Fx16`; floats -> `f32` (serde_json round-trips f32 exactly).
* Text is Latin-1/Windows-1252: use `Reader::cstr/str_u8/...` which map bytes 1:1 to chars.
* `Hex` (opaque bytes) is a last resort for genuinely unknown/unused regions. Keep them small
  and name them `unknown_*`; never dump a whole file as hex.
* Don't store what can be derived (counts, offsets, sizes, padding) — recompute on encode —
  *unless* the stored value can disagree with the derived one in shipped files; then keep it.
* Prefer `Option` / `#[serde(skip_serializing_if = ...)]` to drop noise such as always-default
  fields, as long as the round trip stays exact.

## Context and named ids

`scribble_core::Context` carries cross-file knowledge into `decode`/`encode` (serde never sees it,
so codecs resolve names at decode time into strings and map them back at encode, like `ResRef`):

* resource names: pmindex index <-> logical path (`manifest.json` / `pmindex.xml`);
* named id namespaces (`ctx.namespace(name)` -> `IdNames`), well-known names in `scribble_core::ns`.

Build it with `scribble_formats::load_context(dir)` (CLI and tests; `scribble_pack::load_context`
still gives just the resource names). Tests get it from `scribble_core::testing::context()` after
calling `scribble_formats::install_test_context()` (core cannot depend on the format crates, so the
loader is installed as a hook). `scribble names extracted [namespace]` lists what was derived.

| namespace | ids | names come from |
|-----------|-----|-----------------|
| `object.category`, `object.subcategory`, `object.group`, `object` | the 4 u16 at the start of every `.so` (`[16, 1562, 1588, 1598]` = cow); the last is the dictionary object word id | `.so` file names `<category>_<subcategory>_<group>_<object>.so`: names may contain `_`, so each level takes the one split consistent across all files sharing its id (`gameplay_gameonly_editor__zone` = `gameplay`/`gameonly`/`editor`/`_zone`). The special objects (`_self_self_me.so`, `_stage_stage_stageobject.so`, `_adjective_adjective_adjective1.so`, ...) have an empty category name, printed `_` (id 2866). |
| `adjective.category`, `adjective.group`, `adjective` | the 3 u16 at byte 4 of every `.sa` | leaf: `.sa` file names; category/group: `data\customfilters\asadjbycat_<category>.exf` and `asadjbysubcat_<category>__<group>.exf`, whose contents (u32 n, n x u16 `.sa` index) are exactly the adjectives of that category/group. The `gameplay` filters ship empty; ids 1392 (`gameplay`) and its groups are named from the adjectives they hold (`scribble_formats::context::ADJECTIVE_FALLBACK`). Spaces become `_`. |
| `tag` | u16 tag ids (`.so`/`.sa` tag lists) | English tag dictionary: the word the game displays for the id (tag jump table entry), snake_case |
| `merit` | u16 merit ids | `data\merits\everything.mdb` (every merit, global ids) + the title string (first of three per merit) of its text table |

Facts: the object taxonomy ids of all four levels are distinct numbers (they are all dictionary
word ids), and adjective ids are a separate id space (adjective 1562 = `topiary`, object subcategory
1562 = `large`). Category, subcategory, group and object ids are all unique per level, but names are
not (`other` is a subcategory of six categories): names are resolved within the parent level.

Ids with no file behind them stay numeric: e.g. the special subcategories/groups of category
`_` that no `.so` uses (`_/4519/4522/*`, which the relation loader `FUN_00658830` treats
specially, `_/5980/*/*`, `_/2882/2900/*`, ...) and adjective category 1577 (`myadjectives`).

Printed forms (`NamedPath`/`NamedId`, bijective by construction): a bare name when parsing it back
(within the parent id, if any) gives the same id; `name#id` when the name is ambiguous there; the
decimal id when the id has no name; `*` for the `0xFFFF` wildcard in paths
(`"food/nutsgrains/*/*"`). Names never look like numbers or contain `/`, `#` or `*`.

## Testing

In `crates/fmt-<group>/tests/roundtrip.rs`:

```rust
#[test]
fn all_so_files() {
    scribble_formats::install_test_context(); // full context (names + taxonomy); optional
    scribble_core::testing::check_all::<fmt_object::ScribbleObject>(|p| p.ext == "so");
}
```

Run with `cargo test -p fmt-<group> --release` (release is much faster over thousands of files).
`cargo run --release -p scribble-cli -- check extracted --filter .so` checks via the registry.
Decode one file: `cargo run -p scribble-cli -- decode extracted/<path> --root extracted`.

## Reverse-engineering tips

* pmindex indices appear as immediate constants in code wherever the game loads a specific
  resource, e.g. `.odt` is index 7687 -> grep `0x1e07` / `7687` in `re/decompiled.c`. Find
  resource indices with `grep -n 'name.of.file' extracted/manifest.json`.
* The engine reads fields sequentially from a byte stream; look for loaders that call a small
  set of read_u8/read_u16/read_u32 helpers in order — that sequence *is* the format.
* Cross-check hypotheses across all files of a type with quick Python scripts before writing Rust.
* Build/test with a private target dir if another build is holding the lock:
  `CARGO_TARGET_DIR=target/<group> cargo test -p fmt-<group> --release`.

## Known engine facts

* `FUN_00492a10(&out, resource_index, ...)` (and thunks) loads a resource by pmindex index;
  `FUN_004e1300(index)` is another resource-by-index entry point. Grep for your resource's index
  in hex (`0x24d2` = 9426 = `scribbleobject.odt`) to find the code that consumes it.
* `FUN_004944a0` opens `index.bin` (file manager, `Engine\FileSystem\filemanager.cpp`).
* Dependency kinds (from `.dps`): 0 data, 1 object (.so/.sao), 2 adjective (.sa), 3 anim,
  4 texture, 5 vec, 6 effect (.gec/.gps/.trns). The engine likely dispatches loaders by these.
* Some data is protobuf (`.odt` entries parsed by `FUN_0057c910`, `.aaf` AudioItem by
  `FUN_00441850`; source files ObjectDetails.pb.cpp, AudioItem.pb.cpp), but the exe links
  protobuf-lite: no FileDescriptorProtos are embedded, so field names must come from the code
  that consumes each parsed member (see docs/evidence/object.md, ui-effects.md).
* `fmt-common/src/dps.rs` is a complete small example codec to copy the style from.
* **Scripts are one vocabulary.** `.so` behaviours, `.sod` scene triggers and event-script `@`
  actions are the same engine classes: triggers from `FUN_006d36e0` (parse = vtable slot 8,
  base fields `FUN_006d4640`), actions from `FUN_0064e1f0` (parse = slot 9, target
  `FUN_0064f630`/`FUN_0064f570`), modifiers from `FUN_0068d970`. Their schemas live only in
  `fmt_object::behaviour` (`BEHAVIOURS`, `ACTIONS`, `MODIFIERS`); `fmt-map` and `fmt-ui` reuse
  them. Names come from each class's `static_trigger_*` / `static_action_*` text table
  (vtable `+0x30` / `+0x34`; parameter labels in `static_trigact_param_names`). Trigger type
  byte bit 7 = fires once (`!REPEATABLE`). Action slots: 2 start, 8 write, 9 parse, 11 type id,
  12 run, 13 text table, 14 preload list.
* Adjective lists (`FUN_0053d8d0`) are `(u16 .sa resource, u16 display word)`; the word is a
  key of the language `.dtm` adjective table, `0xFFFF` = the adjective's own name, `0xFFFE` =
  hidden. Filter clauses (`FUN_00676330`) list object taxonomy paths, adjective taxonomy paths
  and **tag ids**. `0x1E01` (7681) as a spawned object means "the spawner's own type"; object
  word ids 6353/6354 (`_/self/self/me`, `_/self/self/myobject`) in filters mean the owner and
  its type.
