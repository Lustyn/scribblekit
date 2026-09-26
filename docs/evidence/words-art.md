# Evidence ledger: words and art (`fmt-dictionary`, `fmt-anim`, `fmt-vec`, object renderer)

Kinds as in [README.md](README.md): `code`, `unused`, `text`, `data-only`. Addresses are
`re/decompiled.c` functions unless marked as data (`DAT_…`) or resource indices (manifest).

## Object renderer (`crates/scribble-studio/src/object_scene.rs`)

| Convention | Claim | Evidence | Kind |
|---|---|---|---|
| Draw order | Within an object, render objects get increasing depths; higher depth = in front | `FUN_004c5b10` assigns `*param_2` to each render object's `+0xe` and bumps the low 12 bits after each; `FUN_004cbde0(depth)` turns it into z = (1 − depth/0x7fff)·128 (`DAT_0082d950` = 128.0) stored at `+0x10`; engine passes use depth 0 first and 0x7fff last (`FUN_0062db10`); an overlay is placed at depth + 500 (end of `FUN_004c5b10`) | code |
| Draw order | Mesh parts draw in **ascending `MeshPart::part`** (node `+0x58`), not by `layer` | `FUN_004c5b10` collects the mesh-part nodes (`FUN_0069a060(5, …)`), `qsort(…, &LAB_0041ec95)` → `FUN_004c4a00`: `if (a+0x58 < b+0x58) return -1; return b+0x58 < a+0x58` — ascending; then assigns depths in that order and stores `depth − base` in part record `+0x1a` | code |
| Draw order | Ties | Never occur: no shipped rig repeats a `part` value (survey of 1,516 atlas objects); `qsort` is not stable, the renderer keeps tree order | data-only |
| Draw order | `.sao` mesh parts | `FUN_006e8100` case 5 gives the render object depth `base (+0x14) + part` — same order | code |
| Draw order | Effect nodes: layer byte `< 0` → before (behind) all of the object's art, `>= 0` → after (in front); only the sign matters; tree order among them | `FUN_004c5b10`: first loop over `FUN_0069a060(9, …)` with `*(char *)(node + 0x5a) < '\0'`, later loop with `-1 < *(char *)(node + 0x5a)`; `+0x5a` is the layer byte (`FUN_006809f0`: 8th byte → `+0x5a`). Guarded by object flag `+0x272 & 8`, set by the parser for every effect node (`FUN_006b6c00` case 9) | code |
| Draw order | The renderer previously used the effect's *animation* byte as its layer | `EffectNode::layer` in `fmt_object` is the flipbook animation (`FUN_006809f0` → `FUN_006ef830(uVar7, …)`); the draw layer is `EffectNode::param` (JSON `layer`). Fixed | code |
| Draw order | Linked child objects (`+0x184`, flag `+0x1a4 & 8`) and joint partners (`+0x6e0`) get depths before the object; `FUN_0069a0e0(0xc/0, …)` attachments after it | `FUN_004c5b10` recursion order | code |
| Draw order | Equipped items: right after the mesh part (or main vector) whose `equip_slot` hotspot holds them; slots 2,4,5,8,9,12,14,15 (1/13 conditionally) first, then 3,6,7,10,11 | `FUN_004c5b10` loops over hotspot children with `+0x50 == 7`, `+0x54 == 5`, testing `+0x70`; `FUN_004c58d0` assigns the held object | code |
| Draw order | Riders split the part list at the `rider_split` part (node `+0x5c`) and the root part (`+0x5d == 0`) | `FUN_004c5b10` `param_4` passes 1/2/3 | code |
| Draw order | Whole-art vector nodes: the main vector (`+0x6c4`) gets the object's depth; extra non-atlas vector nodes are drawn in tree order | `FUN_004c5b10` non-atlas branch sets only `piVar2[5]`; no depth writer found for other vector nodes (167 objects have several, none mixes them with mesh parts) | data-only (tree order) |
| `MeshPart::layer` | Not a draw layer: body-part class (1 body, 3 head, 4 arm, 5 leg) | stored at node `+0x60` (`FUN_006b6c00` case 5, `piVar22[0x18] = flags & 0x3f`); read by `FUN_00477450` (attachment part by class 1/3/4), `FUN_00667f10` (class 5 / 1 bounds), `FUN_00726cb0` (counts class 5), `FUN_006e9960`; never by `FUN_004c5b10` | code (owned by `fmt-object`) |
| `.vec` bone of a mesh part | = the part's tree-order index among the object's mesh parts (not `MeshPart::part`) | `FUN_0069ea20` numbers nodes from `FUN_0069a060(5, …)`: `node+0x5d = i`, `render+0x548 = i`, `FUN_005afce0` → `FUN_00735a30(…, vec, node, i)` → `FUN_007359c0` / `FUN_007355e0(vec, i)` look the vec part map up by `i` (`FUN_007349d0`); `FUN_005afd20` draws `FUN_00731970(vec, i)` | code |
| `.anim` part index | Tracks index the same tree-order part list | `FUN_006ec550` reads the rest pose from `part_list + track.part * 0x28`; `FUN_006eca70` writes through the same records | code |
| `MeshPart::part` | Draw rank; also copied to part record `+0x1a` as the initial depth offset | `FUN_006b6c00` case 5 → node `+0x58`/`+0x5a` and render object depth (`FUN_00628fe0` `+0xe`); `FUN_0069ea20` `record+0x1a = node+0x58` | code |
| Units | `.vec` art is 4 art px per world px | mesh-part corners ×4 (`FUN_005afed0`: `v * 0x4000 >> 12`), bounds ×4 (`FUN_006b6c00` case 5); whole art drawn at `(W >> 2) << 12` (`FUN_005b6d20`) | code |
| Units | Art size is the texture size `floor(W/4)*4 × floor(H/4)*4` art px | `FUN_00734ae0`: `(raw >> 3) * 4` → vec `+0x16/+0x14`; `FUN_00629760` copies the resource's size getters (`FUN_00713790`/`FUN_007137a0`) to render object `+0x46/+0x44` | code |
| Whole-art vectors | Centred on the node | `FUN_005afa80` builds the render object with position `(0, 0)` (`local_8 = local_4 = 0`); vertices are normalised `[-0.5, 0.5]` (`FUN_00733ea0`) | code |
| Atlas UV | `u` right from the atlas' left edge; `v` from the bottom edge, negative upward | `FUN_005b18a0`: `u / W` and `-v / H` normalise the corners; translation `x_min/W + 0.5 − u_min/W`, `y_min/H + max(−v/H) − 0.5` | code |
| Atlas placement | Part triangles are translated (no scale/rotation) so atlas `(u_min, v_min)` lands on `(min(x1,x3), min(y1,y3))` | `FUN_005b18a0` (identity 3×3, translation only; corners 1 and 3 give the quad minimum) — the renderer used a 3-point affine before, identical up to rounding for shipped quads | code |
| Transforms | translate, then rotate; angles accumulate | `FUN_0071ad60`: world angle `+0x30` = parent `+0x30` + local `+0x1c` (negated when mirrored `+0xc`), world position `+0x28/+0x2c` = parent + R(parent)·(local·scale) | code |
| Transforms | positive angle = clockwise on screen (y down) | `R = [cos −sin; sin cos]` in `FUN_0071ad60`; sine table `DAT_0086f338` via `FUN_004f0530` (angle 0 → (0, 4096), 0x4000 → (4096, 0)) | code |
| Transforms | node angle is degrees, converted `angle * 182.04 >> 12` | `FUN_006b6c00` (`__allmul(…, 0x477d1a9)`) | code |
| Pose | animated = rest + key offset | `FUN_006ec550` adds `record+0x18` (angle) / `+0x08`, `+0x10` (x, y) to every key; `FUN_006ed5c0` resets parts to rest each tick; `FUN_006eca70` blends `cur + (v − cur)·w >> 12` | code |
| Pose | `.sao` spin rule gives an absolute angle | `FUN_006ecdd0` (see `.anim` below) | code |
| Vector variants | Renderer draws the node's `vector`, else the first variant | engine picks a variant at random per object (`fmt_object` docs) | data-only (viewer choice) |

## `.anim` (`fmt-anim`)

| Field / claim | Evidence | Kind |
|---|---|---|
| `looping` (byte 0) | `FUN_006ec550`: `+0x2a = *p != 0`; `FUN_006eca70`/`FUN_006ecdd0` wrap when set, clamp otherwise | code |
| `part_count` (byte 1) | skipped: loader reads byte 0, byte 2, then tracks from offset 3 | unused |
| track count (byte 2) | `+0x28` | code |
| `Channel` 0x20 rotation / 0x40 translation | loader: key stride `(ch == 0x40) * 8 + 12`, rest from `+0x18` vs `+0x08/+0x10`; evaluators test `== 0x20` / `== 0x40` | code |
| `Track::part` | index into the part list (`FUN_0069ea20`), `record = list + part * 0x28` | code |
| `loop_independently` (track byte 2) | evaluators: `if (track[1] == 1)` wrap time by the track's last key | code |
| key `frame` | `u16 << 12` | code |
| rotation key (`Angle`, 65536/turn) | added to the rest angle (u16 `record+0x18`), result cast to `short` | code |
| translation key (`Fx12`) | added to rest x/y (20.12) | code |
| interpolation, slope rounding | loader: `k = ftol(1/dt · 4096 ± 0.5)`, `slope = Δv·k >> 12`; evaluator `v + slope·(t − t0) >> 12` | code |
| clip length | max last key (`+0x14`), constructor starts at `0xfff` (`FUN_006ed0f0`) | code |
| `FRAMES_PER_SECOND` = 60 | main loop `FUN_006f6650`: ticks = elapsed ms / 16.666666 (`DAT_00846fac`, `DAT_00846fa0`), 1–5 update ticks per frame, then sleeps out the 16.67 ms; anim time `+0x18 += speed (+0x1c)` once per evaluation, speed 0x1000 (`FUN_006ed730`) | code |
| events, stored per kind | loader: `*(u16 *)(inst + 0x30 + 2 * kind) = frame`; `+0x30 = -1`, `+0x34 = 0xffff` init kinds 0–2 | code |
| `EventKind::Action` (0) | read via the `.so` table copy `FUN_00665d20(slot, 0)` (attack, eat, throw, pickup…, shoot level aim) | code |
| `EventKind::AimDown` (1) / `AimUp` (2) — **swapped** from the previous naming | `FUN_00544460`: `a = atan2(dy, dx)` (`FUN_00484640`, fixed-point radians, mirrored when `dx < 0`); `a > 0` (target below, y down) uses kind 1, `a <= 0` uses kind 2. Data: `bipedanimation_shoot` kind 1 = frame 0 with the arm hanging down (quad centre direction (−0.1, 8.2)), action 10 forward (8.1, −0.4), kind 2 = frame 20 raised (−4.5, −6.8) | code + data |
| `.so` table event order | file `[action, aim_up, aim_down]`; `FUN_00665c00` stores `[p5, p7, p6]` = kind order (policeman: file `[10, 20, 0]`, `.anim` kind 1 = 0, kind 2 = 20). `fmt_object::so` documents the file order as `[action, aim_down, aim_up]` — reported to its owner | code |
| `EventKind::Other(3)` | 12 clips (`maxwell_shoot`, `dog_throw`, `dog_swimidle`, nine `*_swimairattack`); loader writes `+0x36`; no reader of the instance event copy found | unused |
| background evaluator / spin rule | `.sao` objects (vtable `0x846560` slot 1 → `0x6e7e80` → `FUN_006ed710`) use `FUN_006ecdd0`: two-key rotation track, clip length `0x258000` → angle `±(i16)ftol(f32(t / 2457600.0) · 65535.0)` (constants `DAT_008466f8`, `DAT_008466e8`), sign of the interpolated offset, rest ignored. Only `ferriswheeldowntown_spin` qualifies. Implemented: `Animation::sample_background`, `PartPose::rotation_is_absolute` | code |
| `KO DERF` cheat | `FUN_006ec300` toggles `DAT_008b3622`; loader then computes rotation slopes on 16-bit-wrapped keys except resources 13019/13035 (`maxwell_idle`, `maxwell_run`). Not reproduced | code |
| `SLOT_NAMES` 0–43 | game text `data\events\[region]\static_action_animationtype` (resource 8865; neighbour of `static_action_animation` = 8864 returned by `FUN_0058d850`, the `play_animation` action): `ATTACK` … `SCARED SWIM` in slot order. Renamed to that text: 4 `climb_ladder`, 12 `get_up` (was `useobjectthrow`), 15 `idle_2` (was `extendedidle`), 17 `jump_idle`, 26 `scared_run`, 29 `sticky_walk`, 31 `swim_idle`, 35 `fly_idle`, 36 `poke` (was `surprised`), 37 `special_1` (was `flashlight`), 38 `special_2` (was `push`), 39/40 `swim_/fly_extended_idle`, 42/43 `scared_fly/swim` | text |
| slot numbers used by code | 14 idle (`FUN_00665b80` default, `play_animation` ctor), 27 shoot (`FUN_00544460`), 41 sit (`FUN_00665d50`), 60 = none (`0x3c`), table of 60 (`0xf0 / 4`, `FUN_00665c00`) | code |
| `SLOT_NAMES` 44–59 | no text; the file suffix every `.so` table uses for the slot | data-only |
| `slot_for_file` aliases | file suffixes seen in the tables (`swi`, `victory`, `useobjectthrow`, …) | data-only |

## `.vec` (`fmt-vec`)

| Field / claim | Evidence | Kind |
|---|---|---|
| flags bit 7 = bone bounds, bit 6 = quantized | `FUN_00734ae0`: `(char)flags < 0`, `flags >> 6 & 1` | code |
| `version` (flags bits 0–5, always 2) | never tested by the loader | unused |
| `width`, `height` (half pixels) | loader `(raw >> 3) * 4` → `+0x16/+0x14` (texture size) | code |
| `palette_count` | loader only checks bytes 5–6 for non-zero; colour count re-read from `color_count` | code (as emptiness test) |
| `mesh_count` (byte 7) | never read (vertices start at byte 10); stored value differs from the derived one only in `conveyorbelt.vec` | unused |
| `vertex_count`, vertex runs | `u16` at byte 8; quantized: runs of `u16 n, u16 color_slot, n × (i16, i16)`; float: `(f32, f32, u16)` | code |
| `QUANT_SCALE` = 1/8196 | `DAT_0084a1d8` (float 1/8196 widened), used by `FUN_00734ae0` | code |
| GPU 8191 | `FUN_00733ea0` multiplies by `DAT_0084a1b8` = 8191.0 (shorts for the vertex buffer); `uv = pos + 0.5` (`DAT_00824a10`). Different constant, packs the decoded float only | code |
| triangles | `u16` index list → `FUN_0072a290` | code |
| `bone_bounds` | `u16 bone, 4 × f32` × count, converted `ftol(v · 4096 ± 0.5)` (`DAT_00824a20`) and stored per bone (`FUN_00732190`, `FUN_00730140`); no consumer traced | code (load) / data-only (meaning: world units) |
| `Part::bone`, `triangle_range` | `u16 bone, u16 first, u16 end` into a `std::map` keyed by bone (`FUN_005b3a50`); bone = mesh part tree index (renderer section) | code |
| `outline_ranges`, `outline_loops` | per-bone maps (`FUN_007349d0`); `FUN_007355e0` builds a part's outline from them and takes the first outline vertex's colour (fill slot if its alpha is 0) as the vec's outline colour `+0xb8` (`FUN_00735580` likewise) | code |
| colours (`0xAARRGGBB`, fill/edge pairs) | loaded to `+0x8bc`, pristine copy `+0x18bc` (0x400 entries); a slot with alpha 0 stands for the fill slot before it (`FUN_00735580`, `FUN_00565c40`) | code |
| edge colour = fill with alpha 0, G/B swapped | holds for all 258,762 pairs in shipped files; the engine, when it recolours, writes `rgb \| 0xff000000` and plain `rgb` (no swap) to the pair (`FUN_00565f20`, `FUN_006788d0`, `FUN_0057e6e0`) — exporter artifact | data-only |
| `region` | paint group: colour tool `FUN_00565f20` picks the region under the cursor (`+0x28bc[slot]`), `FUN_00733310` finds its dominant colour, every pair of that region is shifted; `FUN_00728d90` restores a region from `+0x18bc`; `FUN_00728ca0` sets a per-colour attribute by region. Region 0 has no special case (it is mostly the black outline colour: 41,005 of 50,962 region-0 fills are `#000000`) | code |
| `edge_region` | stored per slot like `region` (`+0x28bc`); equal to the fill's region in every shipped pair, kept for losslessness | data-only |
| per-part drawing | `FUN_00731970(vec, bone)` draws one bone's triangles | code |

## Dictionaries (`fmt-dictionary`)

| Field / claim | Evidence | Kind |
|---|---|---|
| word file container (alphabet, count, prefix table, records) | `FUN_00741d10` reads `n`, the alphabet, `prefix[i*n + j]` (`uVar10 + (i*n + j)*4 + 5`), then scans records | code |
| `prefix[i][0]` = first record of letter i (fuzzy search) | `FUN_00741d10`: `j` is masked to 0 when the fuzzy flag is set | code |
| `Layout::Current` kind byte | lookup compares record kind with the requested kind (`local_25d != local_248`), type 8 matches `kind & mask` | code |
| `Layout::Legacy` | danish/finnish/norwegian/swedish object dictionaries and `scribbleadjectives\dictionary\*`; the engine only ever selects 7 languages (`FUN_0073c430`: 0x1e3b english, 0x1e3a dutch, 0x1e3e french, 0x1e3f german, 0x1e40 italian, 0x1e44 spanish_mexico, 0x1e42 portuguese_brazil) | code |
| `WordKind` 1/2/4 (and 8) | `FUN_0073c430(kind)`, `FUN_0073c5c0(kind)`, `FUN_0073c720(kind)` switch on 1, 2, 4 (8 = single word, `FUN_007472a0`) | code |
| `word_id` | lookup result `+0` (`*puVar15 = record[…]`); word-id tables `FUN_0073c720` / `FUN_0073d0b0` | code |
| `resource`, `id` per meaning | lookup pushes them to result vectors (`+0x04`, `+0x14`); read by `FUN_00745950` callers (`+0x8`, `+0x18`) | code |
| `cost`, `cost_multiplier` | lookup copies them to result `+0x28` / `+0x38`; only `FUN_00740f30` (cost + Σ mult·adj.cost) and `FUN_00740fe0` (mult + Σ mult·adj.mult) read them, and nothing calls those two (their thunks `0x413e21`, `0x402db0` are unreferenced) | unused |
| `random_gender` (flags bit 0) | lookup pushes `flags & 1` to result `+0x44`; no reader found | data-only (name from which objects carry it) |
| `gender` (flags bits 1–2) | lookup pushes `flags >> 1 & 3` to result `+0x54` (`FUN_0073f740`); no reader found | data-only (1 male: GRANDPA; 2 female: BRIDE, COW; 3 either) |
| `unused_flags` (bits 3–7, renamed from `unknown_flags`) | masked off by the lookup; never set | unused |
| accent folding incl. `0xB5`–`0xBA` | `FUN_00741d10` fuzzy switch: `0xB5`→A, `0xB6`→E, `0xB7`→I, `0xB8`/`0xBA`→O, `0xB9`→U next to `À`…→A etc., `ß`→B. Shipped words use `0xB5`–`0xB9` as the font's macron vowels (`BµMUK¹HEN`, `GY¸ZA`, `N¶N¶`, `KAK·`); JSON keeps the Windows-1252 characters | code |
| trimming / ignored `' ' ? '` | `FUN_00741d10` trims them at both ends and drops them after the first two characters | code |
| jump tables (`Target::name`) | `FUN_0073cd60(id, buf, size, kind)`: `offset = jumptable[id]`, copies the record's word; object exceptions 0xb31/0x1927 → `MAXWELL`, 0x1bf6/0x1c7b → `LILY`, 0x1c70 → `EDGAR`, 0x1c71 → `JULIE`, 0x1520 → name of 0x291. `FUN_0073d3d0` returns the record's word id and the meaning's resource | code |
| word-id tables | `FUN_0073c720(kind)` resources, `FUN_0073d0b0` | code |
| `SingleWordIndex` | resource `FUN_0073c430(8)` loaded by `FUN_007472a0`; lookup type 8 | code |
| `Details` | `FUN_007472a0` loads `FUN_0073c880()` (0x1e25 english …) | code |
| `RelatedObjectsLists` | `FUN_006bfdc0` reads `related_objects_jumptable` (0x1e6b) then `related_objects_dictionary` (0x1e6a) at the offset | code |
| `.dtm` tables 0/1 | `FUN_0073cac0` (table 0, per-language resource 0x1e32 …), `FUN_0073cbc0(…, 0)` table 1 for adjective lists (`FUN_0053e890`, `FUN_004339c0`, `FUN_004bc0f0`, `FUN_00545270`), binary search `FUN_0073ca00`, `0xFFFF`/`0xFFFE` pass through | code |
| `.dtm` table 2 → `apply_adjectives_words` (renamed from `secondary_adjective_words`) | only `FUN_0053d590` (run of the `apply_adjectives` action) calls `FUN_0073cbc0(…, 1)`, for its `word_ids` | code |
| `KindSet` bit names | as `WordKind` | code |
| model-only fields (`object_id_slots`, `tag_id_slots`, `adjective_id_slots`, `single_word_index`, `extra_details`) | sizes/presence of the derived files, kept to regenerate them byte-exactly | data-only |
