# Evidence ledger: `fmt-ui` and `fmt-effects`

Conventions are in [README.md](README.md). Offsets like `+0x128` are fields of the engine object
the loader fills (element, player, voice…); `slot 0x94` is a vtable offset. `FUN_` addresses are
in `re/decompiled.c`, or decompiled from `re/project` when the dump lacks them (the dump has no
body for several vtable methods: `FUN_005259e0`, `FUN_0052ba30`, `FUN_0052bcc0`,
`FUN_00531a80`, `FUN_00528560`, `FUN_0052f310`, `FUN_0052be10`, `FUN_0052bed0`,
`FUN_00528370`, `FUN_006d4d70`).

## `.uib` UI layout (`crates/fmt-ui/src/uib.rs`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| uib | layout read by `FUN_005222c0`; elements by `FUN_005212b0`; type data by slot 0x50 of each class | `FUN_005222c0` loads the resource (`FUN_00492a10`), reads header, calls `FUN_005212b0` per root element, `FUN_0052af80` per template, `FUN_00531be0` per animation | code |
| uib | `text` | u32 at +0 → layout +0x68; `FUN_005233f0(out, i)` looks string `i` up in it with `FUN_0049a710` (-1 = none) | code |
| uib | `width`, `height` | u16s at +4/+6 → layout +0x44..+0x4a | code |
| uib | `keyboard_navigation` | byte 8 → root +0xf4; `FUN_0052b6a0` / `FUN_0052b610` move the focus on inputs 0x23..0x26 only when it is set | code |
| uib | `templates` | named blob; `FUN_0052af80` reads each template's element tree | code |
| uib | `constants` (`name`, `value`) | `FUN_005222c0` copies ≤32 name bytes + u32 into 0x24-byte entries (+200); `FUN_00521220(name)` returns the value; e.g. `FUN_00483b90` reads `"ExpandedButtonsPerPage"` | code |
| uib | `Version` `no_help_text`/`early`/`earliest`, `legacy_data` | `FUN_005212b0` has no version switch (always reads the current layout), so it cannot read these 7 files; their indices 0x6c99/0x6c9b/0x6c9d/0x6c9e/0x6cbb/0x6cbd/0x6cef appear nowhere in the code. The layouts themselves are inferred from the data (all 78 files decode exactly) | unused |
| uib Element | `kind` numbers | `FUN_005212b0` switch: 1 → ctor `FUN_00526dd0`, 2 `FUN_00528420`, 3 `FUN_0052b9e0`, 4 `FUN_00525ee0`, 5 `FUN_00531990`, 6 `FUN_0052f1b0`, 7 `FUN_0052bc30`, 8 `FUN_005234f0`; each class's slot 0x40 returns the number (`FUN_005255b0` → 4, `FUN_0052bca0` → 7) | code |
| uib Element | kind names `group`/`image`/`button`/`text` | `fast_travel.uit` (the menu tool's XML of `fast_travel.uib`) names these types `Layer`, `Image`, `Button`, `TextBox` | text |
| uib Element | kind names `sprite`/`toggle`/`slider`/`frame` | behaviour: 3 builds a `.sfb` flipbook quad (`FUN_0052ba30`); 5 plays `toggle_*` animations (`FUN_005319e0`); 7 drags child `"Thumb"` along `"track"` (`FUN_0052bd50`); 8 builds a nine-slice mesh (`FUN_0053a2d0`) | code |
| uib Element | `name` | matched segment by segment against dotted paths by slot 0x58 `FUN_00527d00` | code |
| uib Element | `position`, `scale`, `size`, `rotation`, `base_size`, `pivot`, `base_rotation` | read in this order by `FUN_005212b0` (size goes to slot 0x60, degrees converted with `*0xb60b60b`); names `x`/`y`/`width`/`height` match the `.uit` attributes | code |
| uib Element | `drawn` | +0x88; drawing and hit testing (`FUN_005278d0`) skip undrawn elements; blink toggles it (`FUN_00526c10`) | code |
| uib Element | `enabled` | `FUN_005212b0` passes `byte == 0` to slot 0x64 (disable); buttons switch to state 3 "disabled" (`FUN_00525e20`) | code |
| uib Element | `clip` | +0xa8, `FUN_0052d390` clips children | code |
| uib Element | `default_focus` | `FUN_005212b0`: `if (byte) root(+0x70)->+0xe8 = element`; `FUN_0052b560` returns +0xe8 as the first focus if slot 0x44 accepts it | code |
| uib Element | `help_text` (was `order`) | stored at +0xe0, read only by `FUN_00528370` → `FUN_005233f0(out, +0xe0)`; `FUN_005729c0` passes the string to `FUN_00724a20`, which shows it in `mainmenu.InfoText`. Data: object-editor buttons index "SAVE OBJECT!", "UNDO LAST ACTION!", … | code |
| uib Image | `vector` | `FUN_00528560` passes `byte != 0` to slot 0x94 (`FUN_00528a90`), which picks scribblematerialvector.gp (`FUN_005afa80`) | code |
| uib Image | `texture`, `rect` | `FUN_00528560`: u32 → slot 0x94; 4 u16 → +0xec..+0xf2 | code |
| uib Image | `texture_flags` `tile_x`/`tile_y`/`srgb` | `FUN_00528560`: `bit0`, `(b & 0x7f) >> 1 & 1` passed to slot 0x94 as tiling; `b >> 7` → +0xf4 (srgbtexture.gp). Bits 2-6 not read | code |
| uib Sprite | `texture` (1st u32), `animation` (2nd u32) — **swapped from before** | `FUN_0052ba30`: 1st u32 → `FUN_006ef1f0` → `FUN_005af500` (texture quad, as for frames); 2nd → `FUN_006efd80` → `FUN_006ef060` (the `.sfb` reader) | code |
| uib Sprite | `unused` | `FUN_0052ba30` reads 8 bytes and advances the cursor by 9 | unused |
| uib Button | `auto_repeat`, `repeat_delay`, `repeat_interval` | `FUN_005255d0` → +0xf4, +0x11c, +0x120; `FUN_00526440` repeats while held when +0xf4 is set; `FUN_00525cd0` reloads the timer from +0x11c | code |
| uib Button | `hit_masks` (`normal_size`/`normal_mask`, `active_size`/`active_mask`) | `FUN_005255d0` → +0xfa/+0xfc/+0x104 and +0xfe/+0x100/+0x108; `FUN_00526030` uses the first set in states 0/3, the second in states 1/2, and tests bit `floor((y+h/2)/4) * (w/4) + floor((x+w/2)/4)` (LSB first; constants 0.5, 0.25 at `0x00824a10`, `0x00825798`) | code |
| uib Button/Toggle | `hover_sound`, `hover_fallback_sound`, `press_sound` (was `sounds[3]`) | u32s at +0x128/+0x12c/+0x130; slot 0x94 of both classes is `FUN_005259e0(old, new)`, called by `FUN_00526440` on state changes: new state 1 plays +0x128 (else +0x12c), new state 2 plays +0x130 (`FUN_0051d120(s, 0x10)`), new 0/3 from 1 stops +0x128 (`FUN_0051cb60`). State names `normal`/`hover`/`pressed`/`disabled` from the table at `0x00895e74` | code |
| uib Toggle | `checked` | `FUN_00531a80` → +0x134; `FUN_005319e0` plays the `toggle_*` animations when set, `normal`/… otherwise | code |
| uib Text | `font` | `FUN_00530fb0`: +0x108 = `DAT_008a82fc + 8 + font * 0x18` (font table) | code |
| uib Text | `align` `center`/`left`/`right`/`justify` | +0x128; `FUN_005301a0` offsets each line by `(w - lw)/2` (0), `-box_w/2` (1), `box_w/2 - lw` (2), or left + spare width per gap (3) | code |
| uib Text | `valign` `center`/`top`/`bottom` | +0x12c; `FUN_005301a0` places the line block centred (0), from `-box_h/2` (1), ending at `box_h/2` (2) | code |
| uib Text | `string` | i32 → `FUN_005233f0` → `FUN_00530910` (set text); -1 gives "" | code |
| uib Text | `box_size` | +0x10c/+0x110 (<<10), used by `FUN_005301a0` and as the hit box (slot 0x78) | code |
| uib Text | `newline_to_space` | +0x114: `FUN_00530910` starts a new line on `\n` only when it is 0; `FUN_0052f710` turns `\n` into ' ' when set | code |
| uib Text | `color`, `alpha` | `FUN_005317e0` reduces each channel to 5 bits, packed RGB555 at +0x116/+0x118; alpha → +0x11c = `(a*100/255)*30/100 + 1` | code |
| uib Text | `caret` (was `spacing_enabled`), `caret_blink` (was `spacing`) | `if (byte) FUN_0052eef0(font+4, u32)`: creates a caret quad from resource `font+4`, stores it at +0x14c, `FUN_00526c90(u32)` sets its blink period (+0xf8), `FUN_00526c10` toggles its `drawn` flag each period, `FUN_0052e700` moves it to the cursor. Set only on the write-mode `writeBox`es (period 45) | code |
| uib Text | `focusable` (was `show_caret`) | +0x144; slot 0x44 `FUN_0052f310` returns `+0x144 && caret`, slot 0x78 (0x0052c400) hit-tests `box_size` only then | code |
| uib Text | `autoscroll`, `scroll_pause`, `scroll_speed` | +0x130, +0x134/+0x138, +0x140 = speed·4096/60; used by slot 0x84 `FUN_005306b0` | code |
| uib Slider | `vertical` (was `disabled`) | `FUN_0052bcc0` stores `byte == 0` at +0xe8; `FUN_0052bd50`/`FUN_0052bed0` use the thumb's x when +0xe8 is set, y otherwise. Data: the tall `hueBar`/`scaleBar` have 1, horizontal scrollbars 0 | code |
| uib Slider | `travel` (was `length`) | +0xfc (<<10); `FUN_0052bd50`: min = thumb − travel, max = min + 2·travel | code |
| uib Frame | `texture`, `inner_uv[4]`, `border` (was `slices[5]`) | `FUN_00523810` reads texture + 5 fx12 (·1/4096); `FUN_0053a2d0` uses `inner_uv` as the nine-slice UVs and `(1-right)/left`, `(1-bottom)/top` as border ratios; `border` → `FUN_005397e0`, which offsets the inner grid lines by it (pixels) | code |
| uib Frame | `unused` | `FUN_00523810` reads 24 bytes and advances by 28 | unused |
| uib Animation | `looping` | `FUN_005325e0` stores it at +0x2c | code |
| uib Key | `ticks` (-1 = immediate) | `FUN_00533340`: `if (ticks == -1)` builds, applies and destroys the key at once | code |
| uib Key | kinds 0-6, 9, 10, 12-14 (`wait`, `move`, `alpha`, `scale`, `rotate`, `enable`, `disable`, `play`, `play_on`, `spawn_effect`, `remove_child`, `sound`) | `FUN_00533340` switch → ctors `FUN_005359f0`, `FUN_00534820`, `FUN_00534430`, `FUN_00535550`, `FUN_00535290`, `FUN_005342c0` (slot 0x64(0)), `FUN_00534150` (slot 0x64(1)), `FUN_00534cf0`, `FUN_00534fa0`, `FUN_00533c30`/`FUN_00533d80`, `FUN_00533f10`/`FUN_00533fd0`, `FUN_00534b40` (`FUN_00534be0`: `FUN_0051d120(sound, 0x10010)`); move sets the position each tick (`FUN_005349f0`) | code |
| uib Key | kinds 7 `check`, 8 `uncheck` (were `kind7`/`kind8`) | ctors `FUN_00533a70`/`FUN_00535870`; apply `FUN_00533b60`/`FUN_00535960` set toggle +0x134 to 1/0 when the target's type is 5 | code |
| uib Key | `easing` `linear`/`ease_in`/`ease_out` | `FUN_00532ce0(easing, progress)`: 0 → t, 1 → t³, 2 → 1 − (1−t)³, else 1 | code |
| uib Key | alpha 0..31 | the engine's 5-bit alpha (element default 0x1f, e.g. text ctor `FUN_0052f1b0` sets +0x11c = 0x1f; `FUN_00530fb0` scales 0..255 to 1..31); data holds 1, 4, 16, 31 | code |

## Event scripts (`crates/fmt-ui/src/event.rs`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| event | record table (`index`, `offset`), record = `script * 7 + language` | `FUN_00459f90` reads the u32 at `(script * 7 + lang) * 6 + 4`, lang = `*(DAT_008a82fc + 4)` | code |
| event | `declared_count` | kept only when it disagrees with the records (no shipped file since the empty placeholders became text tables); not read by `FUN_00459f90` | data-only |
| event | `missing_offset` (0xabcd) | never reached: the language slot is only read for the current language; the event editor writes 0xabcd for unexported languages | data-only |
| event | `Raw` slots | fallback only: 0 shipped scripts (the 91 former "placeholder scripts" are empty text tables, see below) | code |
| event | opcode dispatch | `FUN_00459d10` switch: `+` `FUN_00459720`, `@` `FUN_004589e0`, `C` `FUN_00458760`, `E` `FUN_00458920`, `G` `FUN_00458bb0`, `I` `FUN_00459a90`, `M` `FUN_00458640`, `N` inline, `T` `FUN_00458890`, `W` `FUN_00459210`, `Z` `FUN_00458a90`, `/` return 0, `|` return 1; other bytes are skipped (default case) | code |
| event | `before_bar` / `after_bar` | `FUN_00459d10` returns 1 at `|`; its only caller `FUN_00459f90` calls it again, which resets the actor (`DAT_00829c70`) and the track chain — both parts build the same sequencer. The split is the event editor's layout: in 13 561 of 13 725 scripts only text boxes come before it and none after | code |
| event Track | `actor` | `FUN_00458920` resolves the entity; `E` starts a new track (`FUN_00457f40`) | code |
| event `C` | `x`, `y`, `duration` | `FUN_00458760` → `FUN_006d50a0(x<<12, y<<12, anchor, duration)`; duration added to the script length (+0x1c) | code |
| event `C` | `anchor` (was `easing`) `center`/`top_left`/`top_right`/`bottom_left`/`bottom_right` | `FUN_006d50a0` switch 0-4 (default = 1) picks `FUN_006d4c20` (x − w/2·zoom, y − h/2·zoom), `FUN_006d4cc0` (x, y), `FUN_006d4ce0` (x − w·zoom), `FUN_006d4d20` (y − h·zoom), `FUN_006d4d70` (both) | code |
| event `Z`, `M`, `T`, `N` | `zoom`/`duration`, move, wait, delay | `FUN_00458a90` → `FUN_006d52d0(zoom, d)`; `FUN_00458640` → `FUN_006d5440`; `FUN_00458890` → node 0x1d; `N` only adds to +0x1c | code |
| event `G` | `wave`, `duration` | `FUN_00458bb0` → sequencer node 0x20 (+0x20 = wave), run by `FUN_006d6420` → `FUN_00703450` | code |
| event `W` | `advance_round`, `filter`, `unused_flag_bits` | `FUN_00459210`: bit 0 → node 0x1f +0x1c; bit 1 → `FUN_006778d0` parses a filter that is dropped; bits 2-7 not tested | code / unused |
| event `I` | `title`, `pages`, `duration` | `FUN_00459a90`: non-zero title byte → `FUN_00459640` + `FUN_00456f40`; lines → `FUN_006d5fa0`; duration added to +0x1c | code |
| event `I` | `unused_style`, `unused_style_args`, `unused_align`, `unused_align_args`, `unused_extra`, `unused_format` | `FUN_00459a90` only compares style with 'c'/'d' and align with 'u' to skip 2 bytes, skips the extra byte's operand, and skips every line's format byte (`*param_3 += 1`) | unused |
| event `+` | `function`, `args` | `FUN_00459720` reads a name (`FUN_00459640`) and looks it up (`FUN_004593a0`) | code |
| event `@` | action body | `FUN_004589e0`: `FUN_0064e1f0(type)` then slot 9 (shared schema `fmt_object::behaviour::ACTIONS`) | code |
| event `@` | `unused_trailer` (was `trailing`) | `FUN_004589e0` ends with `*param_2 += 4` without reading | unused |

## Text tables and small formats (`text.rs`, `sfb.rs`, `misc.rs`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| text table | layout `n`, `offset[lang * n + i]` | `FUN_0049a710`: `if (i < *table) str = table + table[1 + lang * n + i]`, else "" | code |
| text table | language order english, dutch, french, german, italian, spanish (Mexico), portuguese (Brazil) | `FUN_0073c880` maps the same global index 0..6 to `details_file_english` (0x1e25), `_dutch` (0x1e24), `_french` (0x1e28), `_german` (0x1e29), `_italian` (0x1e2a), `_spanish_mexico` (0x1e2e), `_portuguese_brazil` (0x1e2c) | code |
| text table | `unused_before` (was `junk_before`) | strings are reached only through their offsets (`FUN_0049a710`); the 11 cases are fragments of the editor stub `… cd ab 00 00 7c 54 01 00 00 00 42` | unused |
| text table | empty tables + `unused_trailer` (were 91 "placeholder event scripts" with `Raw` slots) | first u32 = 0 = `n`; `FUN_0049a710` returns "" for every index, so the rest (a whole event-editor stub: count 0, 7 records, `|T 1 B`) is never read. 22 are named by `.uib` `text` fields, 64 are `…_$merits` tables, 5 are menu tables of unreferenced multiplayer layouts (`multiplayerfind`, `player_icons`, …) | unused |
| sfb | reader | `FUN_006ef060` (loaded via `FUN_006efd80`) | code |
| sfb | `frame_flags` (was `unknown`) | `FUN_006ef060`: frame stride 4 u16, 6 if bit 0, 5 if bit 1, 7 if both, kept at +0x1a and used by all frame accesses | code |
| sfb | `cells` | `FUN_006ef330` reads cell `[x, y, w, h]` of the current frame | code |
| sfb | `frames` `cell`, `duration`, `origin` | `FUN_006ef330` (cell), `FUN_006ef6c0` (`duration << 12` added to the timer), `FUN_006ef9d0` (origin) | code |
| sfb | `Frame.extra` | the u16s added by `frame_flags`; no shipped file sets the flags and the accessors above read u16 0-3 only | data-only |
| sfb | `animations` `looping`, `first_frame`, `last_frame` | `FUN_006ef6c0` steps from first towards last (either direction); at the end it stops if `looping == 0`, else restarts at first | code |
| swc | `enabled`, `unused_flag_bits` (was `flags`) | `FUN_006e43e0`: `if ((*data & 1) != 0)` — no other bit tested | code / unused |
| swc | `colors`, `slot`, `color` | loop of `count` (u32 at +1) entries: `table[slot * 16] = rgba * DAT_00829ac8` | code |
| swc | `unused_entries` (was `unused`) | entries past `count`, never reached by the loop | unused |
| stl | `tags` | `FUN_007122b0` reads count at byte 1 and `count` u32 ids from byte 2 | code |
| stl | `unused` (was `unknown`) | `FUN_007122b0` never reads byte 0 | unused |
| stl | `test.stl` `unused_cpf_body` | 5CPF container as written by `FUN_007af2a0` (magic `"5CPF\r\n\x1a\n"`, `u32 size` + `"DATA"`, `"REFS"`, `u32 0` + `"FEND"`); the function has no callers and no code compares the magic; resource 0x20a5 is referenced nowhere. The shipped bytes have LF → CRLF damage | unused |
| fasttravel | layout, `name`, `level`, `links` | `FUN_00489910` (resource 0x24d3) | code |
| fasttravel link | `direction` | link record +0/+4 (`FUN_00488b60` record `{dx, dy, kind, level, name[64]}`) | code |
| fasttravel link | `kind` 0 = level link | `FUN_00486f50` / `FUN_004872b0` count kind-0 links and test the target level in the profile unlock bitset (`FUN_00646ff0() + 0x3b`) | code |
| fasttravel link | `kind` 2 = named target | the loader reads a name for it; no consumer found (one instance, empty name) | data-only |
| uit | whole format, `utf16_bom` (was `prefix`) | index 0x64e8 appears nowhere in code; layout inferred from the text | data-only |
| credits | `wb.txt` lines and style codes | index 0x1e8a appears in no instruction or data word; codes read off the text | data-only |

## `fmt-effects` (`crates/fmt-effects/src/`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| aaf | `AudioMetaData.AudioItem`, `ToolTown\AA\AudioItem.pb.cpp` | strings at 0x00825298 / 0x00825210; parser `FUN_00441850` | text |
| aaf | enum names `SoundType`, `SpeakerDestination`, ranges 0..4 / 0..18 | setters `FUN_00441160` (`CHECK failed: AudioMetaData::SoundType_IsValid`), `FUN_00441210`/`004412c0`/`00441370`/`00441420`/`004414d0`/`00441580`/`00441630` (`SpeakerDestination_IsValid`, `0x12 < v`) | text |
| aaf | member layout and defaults | `FUN_00441850`/`FUN_00441e30` (+4 type, +8 volume, +0xc f3, +0x10..+0x28 f4..10, +0x30 has-bits); clear `FUN_00440a60`: type 0, volume 1.0, f3 0.2, f4..10 = 18 | code |
| aaf | `sound` (map key) | `FUN_0051e180` reads u32 index, u32 size, protobuf; inserts via `FUN_0051c2a0`; looked up by `FUN_0051bda0` | code |
| aaf | map readers | `DAT_008ad7f0` is used only by `FUN_0051e180`, `FUN_0051c5c0`, `FUN_0051d120` | code |
| aaf | `volume` | `FUN_0051d120`: voice +0xf0 = item +8 (used as volume by `FUN_0051e5d0`); `FUN_0051c5c0` scales by screen distance and clamps 0..1 | code |
| aaf | `sound_type` 1 = `interface` | `FUN_0051d120`: `if (type == 1) flags |= 0x2008000` — the bits UI callers pass (0x10); 0x8000 exempts the voice from the fades in `FUN_0051fea0` | code |
| aaf | `sound_type` 0 `effect`, 2 `music`, 3 `ambience` | only `== 1` is tested anywhere; names from the data: all 134 type-2 items are `audio\music\mus_*`, the one type-3 is `mus_forest_woods_ambience`, untyped items are world `sfx` | data-only |
| aaf | `speaker_destination_6` | `FUN_0051d120`: item +0x18 == 1 → flags |= 0x8000000; `FUN_00517bb0` then routes via `AIL_set_sample_channel_levels` (src {0,1,0,1} → dst {0,1,3,3}, levels {1,1,0.3,0.3}); set on 13 jingle/UI sounds | code |
| aaf | `unused_float_3`, `unused_speaker_destination_{4,5,7..10}` | `FUN_0051d120` reads only +4, +8, +0x18 of its copy; `FUN_0051ced0`/`FUN_00519d80`/`FUN_00519a80`/`FUN_0051e5d0` never dereference the item; `FUN_0051c5c0` reads the volume only | unused |
| exf | `resources` | `FUN_00674390` loads by index and keeps the count; `FUN_00674420` reads u16 at 4+2i for i < count; `FUN_006744c0` picks a random entry (callers `FUN_00547b30`, `FUN_00723150`, `FUN_0063af30`, `FUN_00703b90`) | code |
| exf | `unused_trailer` | accessors never index at or past the count (`FUN_00674420`, `FUN_006744c0`); 0 in all 359 files | unused |
| asset | AssetRef `index`, `kind`, `path` | reader `FUN_00609450` (stream slot +0x60) | code |
| asset | kinds `program`=1, `texture`=2, `system`=3 | table 0x008a34d0 = {"<undefined>", "<program>", ".[TEXTURE]", "<system>"} used by `FUN_007a3130` | text |
| effects stream | slots +0x04 string, +0x2c vec2, +0x34 f32, +0x40/+0x44 u32, +0x48 u16, +0x58 u8, +0x5c bytes, +0x60 AssetRef | vtable 0x0083d9d8: +0x44 `FUN_006092c0` (4 bytes), +0x48 0x609270 (2 bytes), +0x58 0x60a230 (calls +0x5c with 1), +0x5c `FUN_006091b0` (memcpy), +0x60 `FUN_00609450` | code |
| gps | `version` gates | `FUN_007b0d60` (`< 6`), `FUN_007aa2b0` (`> 2`, `> 6`, `> 4`); saved-particle size from `FUN_007b1680` (100 or 104 bytes) | code |
| gps Material | `id` | `FUN_007a8d80` keys the material map by it; emitter lookup asserts `iter != map.end()` (System.cpp:0x130) | code |
| gps Material | `unused_capacity` (was `capacity`) | `FUN_007b0d60` reads it into `local_24`, never used | unused |
| gps Material | `unused_saved_particles` (was `saved_particles`) | `FUN_007b0d60` reads n·`FUN_007b1680(v)` bytes into a buffer and frees it. Layout corrected: v0/1/4 = 25 floats, v2/3 = 1 leading float (always 0) + 25, v5 = 25 + `lookup_pos`; float names from `FUN_007aa530`'s vertex and `determined.gp` | unused |
| gps Material | `simulation` `Determined`/`Discreet` | type lookup `FUN_007a3890`; registered by `FUN_00604dc0` (`FUN_00604390`, `FUN_00604300`); CPU stepping `FUN_00602760`/`FUN_00602460` | text + code |
| gps Material | `program`, `texture`, `lookup_texture` | `FUN_007b0d60` 3 AssetRefs; shader names `detailTexture`/`lookupTexture` in determined.gp | code + text |
| gps Emitter | `material`, `name` | `FUN_007a8d80` map lookup; `FUN_007aa2b0` slot +4 | code |
| gps Emitter | `discrete` (+0x18) | `FUN_007aa530` skips the force call `FUN_007a3860` and integrates on the CPU; pairs with `Discreet` materials 27/27 | code |
| gps Emitter | `looping` (+0x19) | `FUN_007ab280` stops when clear; bit 31 of the sort key (`FUN_007a8d80`) | code |
| gps Emitter | `prewarm` (+0x1a) | `FUN_007a6220` emits from −duration when looping && prewarm; bit 30 of the sort key | code |
| gps Emitter | `draw_order` (+0x1c) | `FUN_007a8d80`: +0xdc = `u16 order | prewarm<<30 | looping<<31` | code |
| gps Emitter | `max_particles` (+0x20) | default 0x40 (`FUN_007ab7d0`); passed to `FUN_007b0210` by `FUN_007a8d80` | code |
| gps Emitter | `unscaled_motion` (+0x1b) | `FUN_007a99f0` sets motion factor +0x34 = 1/scale; `FUN_007aa530` multiplies gravity, wind and speed by it | code |
| gps Emitter | `duration` (+0x28) | `FUN_007aa530` key index = (t − start)/duration, clamped 0..7 | code |
| gps Curve | 8 keys; stored key count | `FUN_007a9b80` reads the count into a local and always reads 8 min/max pairs (all 13 872 curves store 8) | code / unused |
| gps curves | `emission_rate` | curve 0 (+0x34), read by `FUN_007ab280` | code |
| gps curves | `gravity_scale`, `wind_scale` | `FUN_007aa530` → mass[0..1]; force callback `FUN_00600dc0` (environment `FUN_00604dc0`, vtable 0x0083d85c +0x10) returns `m0·(0, 9.8) + m1·(5, 0)` | code |
| gps curves | `unused_mass_z`, `unused_mass_w` (were `mass_z`/`mass_w`) | sampled into mass[2..3] (+0x100, +0x144) but `FUN_00600dc0` reads m[0], m[1] only; `vertexMass` is declared but unreferenced in determined.gp / simulated.gp | unused |
| gps curves | `offset_x/y`, `speed`, `direction`, `rotation`, `angular_velocity`, `size`, `growth`, `lifetime` | `FUN_007aa530` samples +0x188, +0x1cc, +0x210, +0x254 (cos/sin), +0x2dc, +0x320, +0x3a8, +0x3ec, +0x474 into the vertex `determined.gp` evaluates | code |
| gps curves | `drag`, `angular_drag`, `growth_drag` | clamped ≥ 0.01 by `FUN_007aa160` (+0x294, +0x360, +0x42c); divisors `vertexUnused.z`, `vertexRotation.w`, `vertexScale.w` in determined.gp | code |
| gps Emitter | `uv_offset`, `uv_scale`, `lookup_row` | `FUN_007aa2b0` slots +0x2c/+0x48; `FUN_007aa530` copies +0x4b4..+0x4c0 to `vertexTexcoord`; `(row % rows + 0.5)/rows` → `vertexLookupPos` | code |
| gec | `version` | writer `FUN_007a2e00` writes 1; reader `FUN_007a3370` reads it into a local it overwrites (1 in all 312 files) | unused |
| gec | `system`, `emitters[].emitter`, `position` | `FUN_007a3370`: slot +0x60 (kind 3), then +0x04 string and +0x2c vec2 per entry; writer asserts `types.size() == positions.size()` | code |
| trns | `emitters` (`emitter`, `position`) | `FUN_007ad400`: +0x44 count, then +0x04 string and 2×+0x34 per entry | code |
| trns | `program`, `front_image`, `back_image` | `FUN_007ad400` 3×+0x60; `frontImage`/`backImage` bound in `FUN_007acc70` | code + text |
| trns | `time_start` +0xac, `wipe_speed` +0x94, `fade_time` +0x98, `fade_start` +0x9c | f32 reads in `FUN_007ad400`; shader names `timeStart`/`speedWipe`/`timeFade`/`timeFadeStart` in `FUN_007acc70`; `FUN_007ac3e0` overwrites +0xac at start; `FUN_007ac1d0` sets them each frame | code + text |
| trns | `emitter_stop_delay` +0xa0, `outro_duration` +0xa4 | `FUN_007ac1d0`: emitters stop when `+0xa0 < now − outro_start`; ends when `1 − (now − outro_start)/+0xa4 < 0` | code |

## Still data-only, and why

* **aaf `sound_type` 0/2/3 names**: every reader of the metadata map was checked (`FUN_0051e180`,
  `FUN_0051c5c0`, `FUN_0051d120`); only `== 1` is tested, so the other names come from the
  sound paths. The meaning of the individual `SpeakerDestination` values (19 values, default
  18) is likewise not decided by code beyond "field 6 == 1".
* **sfb `Frame.extra`**: no shipped file sets `frame_flags`, and no accessor reads past u16 3.
* **fast-travel link kind 2**: read by the loader, never consumed by the code found
  (`FUN_00486f50`, `FUN_004872b0` test kind 0 only); a single empty instance.
* **legacy `.uib` layouts** (`legacy_data`): the engine cannot read them, so their byte layout
  is inferred from the data alone and kept as bytes.
* **`.uit` and `wb.txt`**: no code references their resources.
* **event `missing_offset` / `declared_count`**: editor artefacts, not read for the shipped data.
