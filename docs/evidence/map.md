# Evidence ledger: fmt-map (`.lvls` `.tle` `.stp` `.sod` `.mdb` `.plf` `.nbtc` `.dpd`)

Kinds as in [README](README.md): `code` (the engine reads and uses it as described), `unused`
(the loader skips it / nothing reads it; cited), `text` (the game's own string names it),
`data-only` (no code path; the reason for the name is given).

Scene scripts (trigger/action/modifier names and fields, filters, adjective lists, entity
references) are the shared schema of `fmt_object::behaviour` / `fmt_object::record` and are
ledgered by fmt-object (`object.md`); requests for it are in
[requests-behaviour.md](requests-behaviour.md). Only the scene-specific wrapper
(`for_merit`) is listed here.

**Instance ids.** `FUN_004bdab0` gives the placed object `objects[k]` the engine instance id
`k - 1` (`FUN_006997d0(obj, ..., loop_index - 1)`, asm 0x4be4a6 `dec edx; push edx`); the stage
entry (always `objects[0]`) has none. Every object number in links, contents, rope
attachments, joints, AI targets and `{"object": n}` entities is such an instance id, resolved
by `FUN_006a1610`/`FUN_006a1770` -> `FUN_0047a390` (table `DAT_008a5bc0`). Checked on all 122
scenes (every rope attachment's `rope` + 1 is a rope or chain, links read hat -> Maxwell, joint
anchors match `objects[a + 1]`).

## `.lvls` level table (`FUN_004e1300`, descriptor copy `FUN_004e1b10`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| LevelTable | `unused` (byte 0) | `FUN_004e1300` starts at byte 1: `DAT_008ad43c = *(byte *)(local_4c + 1)`, slots from byte 2 | unused |
| LevelTable | `levels` (slot index kept, `null` = empty) | `if (*(char *)(iVar2 + local_4c) == '\0') FUN_004e1b10(&DAT_0086f23c)` (empty descriptor); slots addressed by index | code |
| Level | `tile_map`, `scene`, `setup`, `merits` | `FUN_004e19e0` -> descriptor +0x00 / +0x04 / +0x08 / +0x0c; `FUN_004e18d0` finds a slot by scene (+4), `FUN_004e1950` by tile map (+0) | code |
| Level | `name` | u8 length + chars -> descriptor +0x15 (`memcpy(param_1 + 0x15, param_7, param_6)`), returned by `FUN_004e1950` | code |
| Level | `unlocks_rewatch` (flag bit 0 -> +0x14) | `FUN_004a2fd0`: `if (*(char *)(DAT_008a8700 + 0x7c8c) != 0)` (current descriptor +0x14) shows `mainmenu.rewatch` and `mainmenu.creditB`; `FUN_004e15e0` skips such slots in the budget scan (`*(char *)(iVar1 + 0x14) == '\0'`). Set on E2_RIVER, E2_SUBURBIA_01, E3_SUBURBIA_01 | code |
| Level | descriptor +0x10 (not stored) | `FUN_004e15e0` fills it from the scene's `budget_cost` (see `.sod`) | code |

## `.mdb` merit database (`FUN_004fa620`, entry `FUN_004f73d0`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| MeritDatabase | `text_table` | copied into every record +0x18 (`((undefined4 *)(iVar5 + iVar7))[6] = CONCAT13(...)`) | code |
| Merit | `id` (+0x04) | binary-searched by `FUN_004fa310`; save bitmap bit `DAT_008b3404+0x50` (`FUN_004f7490`, `FUN_004f77f0`) | code |
| Merit | `text_index` (+0x1c) | first of the merit's strings; the browser pages three per merit (`FUN_004f8360`: `iVar3 / 3`) | code |
| Merit | `icon` (+0x14) | texture index | code |
| Merit / MeritCategory | `category` (+0x08): living..misc, `none` = 0xFF | `FUN_004fa620`: `if (uVar6 < 8) param_1[uVar6 + 3]++`; `FUN_004f8360` lists a category (`*(int *)(iVar4 + 8) == *(int *)(param_1 + 0x3c)`); names `meritbrowse.globalcategory.*Text` | code + text |
| Merit | `unused` (2 bytes) | `FUN_004f73d0`: `*param_3 = *param_3 + 3` after the category byte | unused |
| Merit | `unused_kind` (+0x24) | written by `FUN_004f73d0`, copied by `FUN_004f7780`/`FUN_004f7ca0`, never read (searched all callers of `FUN_004fa310/380/3e0/450`, the record methods `FUN_004f7490`..`FUN_004f7a70`, stride-0x48 loops and the pending copy at +0x7f4). Data: 255 on the 78 quest merits, 3 on `firestarter`, 1 otherwise | unused |

## `.tle` tile map (`FUN_004b96d0`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| TileMap | `width`, `height` | tile counts; `n = width * height` grid bytes | code |
| TileMap | `layers` (file flag bit 0), `paths` (bit 1) | gated reads in `FUN_004b96d0` | code |
| TileMap / TileLayer | `alternate_pages` (bit 2) | skipped unread: `iVar6 = iVar6 + 1 + count*8`. Data: same textures on a ceil(W/204.8) grid (all 47 layers) - probably another platform's layout | unused |
| TileLayer | `pages` | only the first u32 of each 8-byte entry is read (`iVar6 += 8`; the `-1` word is never read); placed row-major on a 256 px grid by `FUN_006f34d0` (`(width*16+255) >> 8` columns) | code |
| CollisionGrid | `tileset_texture` | `FUN_006f2700` (renderer +0x54); `FUN_006f3700`: `if (*(layer+0x30) == 0 && tex) FUN_007175d0(tex, grid, 0xd20)` draws the grid when there are no pages (c_lava, c_stadium*) | code |
| CollisionGrid | `tileset` | `.nbtc` -> level+0x1ec, loaded by `FUN_005de450` | code |
| CollisionGrid | `tiles` | row-major tile indices into the tileset's `shape_of_tile` | code |
| CollisionGrid | `transforms`: bit 0 mirror x, bit 1 mirror y | `DAT_0083c418[shape*4 + t]`: t1 swaps 3<->4, 5<->6, 7<->8 ...; t2 swaps 1<->2, 5<->7, 6<->8 and slopes to ceiling slopes; t3 both. `FUN_005e2450` remaps directions the same way (t1 `-d & 7`, t2 `(-d-4) & 7`, t3 `(d-4) & 7`) | code |
| TilePath | `unused` (lead byte) | asm 0x4b9a04 `movzx edx, byte [esi+eax+1]; add esi, 2` | unused |
| TilePath | `points` | sorted right to left (`FUN_004b1490`/`FUN_004b13a0`) and pushed to level+0x2b4 (`FUN_004b8780`); no reader of that list found | code (store) / data-only (purpose: traces walkable surfaces) |

## `.nbtc` collision tileset (`FUN_005de450`, called only as `FUN_005d25c0(level+0x1ec, level+0x1f0)` from `FUN_004bfe00`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| CollisionTileset | `shapes`, TileInfo `shape` | first pass: tile count, tile -> shape table, shapes | code |
| Edge | `from`, `to` | pixel coordinates 0..16 of the edge | code |
| Edge | `normal` | index into `DAT_00897f88` ((x, y) 20.12 pairs): 1 (0,-1) floor, 2 down, 3 left, 4 right, 5-8 45° diagonals, 9-12 26.6° slopes; `FUN_005bcc90` walkable if y <= -0.6 (`<= -0x99b`), also `FUN_005c9330`, `FUN_005e01e0` | code |
| TileInfo | per-tile tables loaded only from a second resource | `if (param_3 != 0)` and `param_3` = level+0x1f0, only ever zeroed (`FUN_0049fd50`, `FUN_004bfe00`) | unused (on PC) |
| TileInfo | `indestructible` (-> +0x34) | `FUN_005e21b0`: `layer1[i] != 0 \|\| flag[tile] == 0 \|\| param_6`; both callers pass `param_6 = 1` | unused (meaning: code) |
| TileInfo | `neighbour_replacement` (8 x u8 -> +0x38) | `FUN_005e2450`: `*(byte *)((tile*8 \| dir) + *(param_1+0x38))` = tile this becomes when neighbour `dir` is carved (dirs from `FUN_005e21b0` calls); only caller `FUN_005e4d20` passes `param_7 = 0` (dead) | unused (meaning: code) |
| TileInfo | `neighbour_transform` (u16 -> +0x3c) | `FUN_005e2450`: `(mask >> dir*2) & 3 ^ transform` | unused (meaning: code) |
| CollisionTileset | `unused_trailer` (2560 bytes) | the second pass returns after the masks; never read. Shaped like a second replacement + transform table pair (256*8 + 256*2) | unused / data-only (structure) |

## `.plf` parallax layers (`FUN_006f3180`; strip `FUN_005b56e0`, sprite `FUN_005b5980`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| ParallaxLayer | `texture` (`null` = 0xFFFFFFFF) | passed to the strip / sprite | code |
| ParallaxLayer | `force_strip` (bit 0) | sprite path only when `(bVar1 & 1) == 0 && bVar6 != 0` | code |
| ParallaxLayer | `x` present (bit 1) = sprite | `local_8 = (short)x << 12` -> `FUN_005b5980` | code |
| ParallaxLayer | `disabled` (bit 2) | `if ((bVar1 >> 2 & 1) == 0) {...create...}`; the depth index still advances | code |
| ParallaxLayer | `scale` (bit 3) | sprite +0x14/+0x18, width/height multipliers in `FUN_005b5a10`; strips read and ignore it | code |
| ParallaxLayer | `unused_flags` (bits 4-7) | never tested | unused |
| ParallaxLayer | `strip_height` | `FUN_005af5e0(tex, ..., screen_w*2, height*4, ...)`; sprites discard it | code |
| ParallaxLayer | `distance` (20.12) | strips: `FUN_005b60b0` u = camera x / distance; sprites (+0x510): `FUN_005b5a10` follows the camera by f(distance) | code |
| ParallaxLayer | `y_offset` | strips: `*4` -> +0x514, subtracted in `FUN_005b60b0`; sprites: world y `y << 12` | code |

## `.stp` level setup (`FUN_004b24d0`; tests flag bits 0-2, 4-13, 15-18 only)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| LevelSetup | `unused_prefix` (bit 14) | `if ((bVar3 & 0x40) != 0) iVar7 = local_15c + 8;` (skipped) | unused |
| LevelSetup | `sky_color` (bit 0) | packed to +0xb8, floats +0xa8..+0xb4 at the end of the loader; `FUN_0061b000` binds them to `uSkyColor` (uniform table `FUN_0061f0c0`) | code + text |
| LevelSetup | `parallax` (bit 1) | `*(*(param_1+0x1c4+DAT_008a8705*4)+0x14) = uVar8` | code |
| LevelSetup | `unused_water_rows` (bit 2) | written to +0x7b88/+0x7b8a (== height -> 0xFFFF), reset in `FUN_0049fd50`, no reader in .text. Data: top 0 underwater, liquid top row on coasts; bottom = height | unused / data-only (meaning) |
| Playlist | `unused` lead byte | count read from byte 1: `uVar15 = *(byte *)(local_15c + 1 + ...)` | unused |
| Playlist | `tracks` | track 0 -> +0x194, others +0x198.., count clamped `if (8 < uVar15) *(+0x1b8) = 8` | code |
| LevelSetup | `exit_left/right/up/down` (bits 5/6/9/10) = exits 0..3 | `FUN_00641650`: exit 0 box at `x = 0x14000`, 1 at `width - 0x14000`, 2 at `y = 0x14000`, 3 at `height - 0x14000` (+0xe0 = 0..3); arrival `FUN_005f14a0` places the player at the opposite edge; up/down gated by `FUN_004a3e30` | code |
| Exit | `arrival_script` (bits 7/8/11/12) | trigger +0x11c; `FUN_00641eb0`: `if (+0x11c != -1) {transition+0x40 = 2; +0x48 = script}`; `FUN_005f14a0` runs `FUN_004afb10(script)` instead of edge placement | code |
| LevelLink | `scene, setup, tile_map, merits, dependencies, preload` | `FUN_004ae8a0` reads six u32 in this order; descriptor mapping via `FUN_004e1a50`; `dependencies`/`preload` streamed by `FUN_004958b0`, kept for the exit taken (`FUN_00641eb0`, others freed by `FUN_00640e10`); all 71 links have sod/stp/tle/mdb/dps/dpd in this order | code |
| LevelSetup | `top_left_aligned_exits` (bit 13, default all) | table +0x2c4; `FUN_00641eb0` passes `(flags >> exit) & 1` to `FUN_004ad1d0` (+0x4c); clear -> offset `height - y` / `width - x`, re-applied from the bottom/right edge by `FUN_005f14a0` (`iVar1 = iVar2 - iVar1`). c_beach clears `down`, c_reef `up` | code |
| LevelSetup | `unused_start_position` (bit 15) | written to +0x7cf8/+0x7cfc only (and reset `FUN_0049fd50`) | unused / data-only (meaning) |
| EnvironmentTextures | `unused` lead byte | `FUN_004dfbb0`: `*param_3 = *param_3 + 1` before the count | unused |
| EnvironmentTextures | `slots` | `FUN_004dfbb0` on background +0xb0: slot i -> `param_1[i+1]`, texture preloaded to `param_1[i+7]` (not for 5) | code |
| EnvironmentSlot | `unused_0`, `unused_1` | `if ((iVar8 == 0) \|\| (iVar8 == 1)) param_1[iVar8+1] = -1` | unused |
| EnvironmentSlot | `unused_2` | stored, but the accessors `FUN_004dfad0`/`FUN_004dfa70` are only called with 3, 4, 5 | unused |
| EnvironmentSlot | `water_mask` (3) | `FUN_00738080` binds it for water.gp with UV `1/(width*16)`, `1/(height*16)`; also `FUN_00739820`, `FUN_006decd0` | code |
| EnvironmentSlot | `reflection` (4) | `FUN_0061b000`: `FUN_004dfa70(4)` -> `uReflectionTexture` | code + text |
| EnvironmentSlot | `shadow_occluder` (5) | `FUN_00449340`: `FUN_004dfad0(5)` -> `FUN_005afa80(vec, 0x420)` with vectormeshoccluder.gp over the whole map | code + text |
| LevelSetup | `gravity` (bit 17, +0x7d00) | `FUN_004bfe00`: `DAT_00897f80 = gravity * 0x28f`; also `FUN_0068f300`, `FUN_0068de80`, `FUN_007268f0` | code |
| LevelSetup | `ground_color` (bit 18) | `/255` -> +0xc0..+0xc8; `FUN_0061b000`: `uGroundColor` | code + text |
| LevelSetup | `unused_title_transition` (bit 19) | the loader never tests bit 19; no other reader of the setup (level+0x7c80 only via `FUN_004bfe00` -> loader, `FUN_00644260`, `FUN_006734d0`); `.trns` from `env_transitions` | unused |

## `.dpd` preload list

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| PreloadList | `(kind, resource)` list, same as `.dps` | referenced as `preload` of level links and streamed by `FUN_004958b0`; layout/kinds ledgered with `fmt_common::Dependencies` | code |

## `.sod` scene (`FUN_004bdab0`)

### Header and top-level sections

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| Scene | `has_names` | `if (*param_2 == '\0') local_200 = 0`; gates object names (obj+0xb5c, `FUN_00479110`), group names (`FUN_006cb850`), merit markers (`FUN_004bbe00` param_3) | code |
| Scene | flag word order / section functions | dispatch in `FUN_004bdab0` (`local_231 = uVar16 >> 5 & 1`, ...); bits 18 / 22 (`FUN_004b3b50`, `FUN_004b7180`) never set, unsupported | code |
| Scene | `budget_cost` (bit 0) | skipped here (`if (flags & 1) local_280 = 7`); `FUN_004e15e0`: `*(uint *)(slot + 0x10) = *(ushort *)(sod + 5)`; `FUN_006831a0`/`FUN_006425b0` -> `FUN_006a7260(&x, 0xfffffff - cost)` trims carried objects (costs obj+0x284 summed by `FUN_0069fdd0`) | code |
| Scene | `event_id` (bit 1) | `*(int *)(**(int **)(param_1 + 0x1c) + 0x148) = ...`, or a save-slot lookup in type-6 levels (`FUN_00643840`); `FUN_004e15e0` reads it too | code |
| Scene | `persist_slot` (bit 2, +0x154) | `FUN_00454510`: `if (+0x17c && +0x154 < 0x34) FUN_00646fa0(+0x154 + 5, ...)`, `FUN_006c90b0` saves objects with obj+0x2a8 >= 0; +0x17c set on completion (`FUN_004c1550`). Values = `event_id` slots of s_downtown / s_museum / s_oasis | code |
| Scene | `liquids_offset` (bit 10 word) | read into `local_26c`; mode 1 re-reads only liquids: `FUN_004b3220(param_2, &local_26c, 1)` | code |
| Scene | `unused_markers` (bit 15) | `FUN_004af310` -> `FUN_00641b70`, a bare `ret 4` | unused |
| Scene | `sky_color` (bit 14) | RGB565 into level+0xb8, `FUN_004c5180` | code |
| Scene | `camera_bounds` (bit 12) | `FUN_004af1e0` -> `FUN_004505c0` | code |
| Scene | `intro_script` (bit 24) | stored at mode+0xc0; `FUN_00455720`/`FUN_00462d30`/`FUN_00463350`: `if (mode[0x30] != -1) FUN_0045a060(mode[0x30])`; all 102 are `*_intro` event files | code |
| Scene | `legacy_objects` / `unparsed_sections` (e1_dunes.sod) | objects without `name_word`; its hint section (`03 ff ff ff ff 09 ...`) would make `FUN_004af400` (shared by `FUN_004bdab0`/`FUN_004bc0f0`) read 255 progress bytes past the end; no PC reader, pmindex 9603 not referenced | unused (kept verbatim) |

### Placed objects (`FUN_004bdab0` loop; object loader `FUN_006bbd70`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| SceneObject | `object` (0xFFFF = stage), `name`, `triggers` | loop in `FUN_004bdab0`; name -> obj+0xb5c | code |
| SceneObject | `name_word` | `FUN_0073cac0` -> obj+0x162 | code |
| SceneTrigger | `for_merit` | u16 before each trigger when bit 3 is set (asm 0x4bf040..); `FUN_0071eee0` finds the merit rule by id, `FUN_006d6960`/`FUN_00462060`/`FUN_00462150` add trigger + actions to its progress nodes | code |
| OBJECT_BODY | `x`, `y` (16.16 tiles) | position | code |
| OBJECT_BODY | `load_behaviours` | load flag 1 (0 skips the .so behaviour block) | code |
| OBJECT_BODY | `load_flags` `spawn_contents`, `drop_contents_in_place` | load flags to `FUN_006bbd70`; contents placed via `FUN_00672060(obj, 1, 0)` | code |
| OBJECT_BODY | `load_relations` (bit 0), `initial_animation` (bits 1-7) | byte bit0 -> load flag 4; `(b >> 1) - 1` -> `FUN_006b5440(anim)` (asm 0x4be336) | code |
| OBJECT_BODY | `rope_segments` | overrides the .so rope segment count | code |
| OBJECT_BODY | `object_flags`: `immovable`, `mirrored`, `no_default_equipment`, `no_wander`, `draggable`, `always_sees_target`, `starts_inactive`, `starts_hidden` | `no_wander`: `FUN_006649a0` returns when obj+0x68c bit 0x40; `always_sees_target`: `FUN_00653b30` -> AI+0x14a, `FUN_005aa2e0` uses range 0x7fffffff; `starts_inactive` clears obj+0x270 bit 3 (active); `starts_hidden`: `FUN_006a7470(obj, 0, 1, 1)` takes it and its attachments out of the world (obj+0x272 bit 2) | code |
| OBJECT_BODY | `grabbable` (bits 0-1 obj+0x24d), `asleep` (body sleeps), `locked` (obj+0xb30), `drag_movable` (obj+0x252), `intangible` (obj+0x24e), `has_event` | extracted at asm 0x4be11c-0x4be163 | code |
| OBJECT_BODY | `unused_bit2` | the same asm extracts bits 0-1 and 3-7 only (163 objects set it) | unused |
| OBJECT_BODY | `optional_fields` `has_persist_id` (obj+0x2a8, `FUN_00454510`), `has_appearance_variant`, `has_wander_range` (`FUN_006639b0`) | gated reads | code |
| OBJECT_BODY | `unused_40e` (optional bit 2) | obj+0x40e only written (0x4be867, .so loader 0x6bc43f, `FUN_006ffdc0`/`FUN_00700230`) and serialised (`FUN_006b1d00`, `FUN_006c84a0`) | unused |
| OBJECT_BODY | `rotation` | 20.12 radians | code |
| OBJECT_BODY | `ai_target` (instance, 0xFF none), `ai_target_relation` (default protect) | deferred list, `FUN_006a1770(target)` -> `FUN_00653b70` (AI+0x80, AI+0xdc), asm 0x4bf59a-0x4bf5bc | code |
| OBJECT_BODY | `adjectives`, `attachments` | `FUN_0053d8d0` adjective list; `.so` attachments | code |
| OBJECT_BODY | `draw_layer`, `draw_order` | `if (b != -1) { obj+0x247 = b; obj+0x248 = order }` | code |
| EVENT (`FUN_00672c40`) | `event`, `event_id` | target scene; save completion bit (`FUN_00673060`) | code |
| EVENT | `preview` | `potalpreview.previews.PreviewSlot` texture (`FUN_004e07c0`, default 0x671b) | code + text |
| EVENT | `text_table`, `text_index` | `FUN_004e0540` -> `FUN_00499a30`/`FUN_0049a710` (mode+0x14c/+0x150), marker label in `FUN_00673aa0` | code |
| EVENT | `marker_kind`: `event`, `multiplayer`, `event_no_icon`, `multiplayer_3`, `completed`, `storefront`, `storefront_info`, `storefront_7` | `FUN_00673aa0` icons (sf_eventicon/sf_eventcompleted, multiplayicon, storefronticon, storefrontinfo); `FUN_00673e80` passes show = 0 for 2; `FUN_006734d0` modes 7 (1), 4 (2), 8 (3). 3 and 7 share the icons of 1 and 5 | code + text |
| EVENT | `margin_*` | proximity box around the object, tiles | code |
| EVENT | `camera_anchor`, `camera_x`, `camera_y`, `camera_zoom` | `FUN_006734d0` reads +0x34/+0x38, offsets by the anchor, `FUN_0044ff40`; `FUN_00673480` | code |

### Merit rules (`FUN_004bbe00`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| MeritRule | `merit` | rule id, found by `FUN_0071eee0`; `FUN_006f8920` looks up the merit | code |
| MeritRule | condition `{"entity": ...}` | `FUN_004f79c0` -> merit +0x2c list; matched by `FUN_004f7a70` | code |
| MeritRule | condition `{"zone": n}` | `FUN_004f7b60` -> merit +0x38; `FUN_004f7b80` checked by `FUN_004b0830` for merit zone n (bit 20); no shipped use | code |
| MeritRule / MeritMarker | `markers`, `editor_position` | inserted into rule+0x14 map by `FUN_004bb630`, no lookup (only destructor `FUN_006d6ab0`) | unused |

### Groups (`FUN_004b6830` / `FUN_006cb850`)

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| ObjectGroup | `manual_only` (bit 0 -> +4) | `FUN_006cba10`: `if (group+4 != param_3) return 0`; the automatic caller `FUN_004c5190` passes 0 | code |
| ObjectGroup | `has_member_setup` (bit 1) | gates the second blob | code |
| ObjectGroup | `unique_words` (bit 2 -> +0x39) | `FUN_006cba10` rejects a name word (obj+0x162) already in +0x40 | code |
| ObjectGroup | `filter`, `name` | membership filter; name with `has_names` | code |
| ObjectGroup | `triggers` (u32-length blob) | `FUN_006cb010`: count, u16 merit if group+5, trigger + actions; 1799/1799 decode (the old hex fallback removed) | code |
| MEMBER_SETUP | `load_flags` `no_default_equipment`/`load_relations`/`spawn_contents`/`load_behaviours` | byte 0 bits -> load flags 0x10/4/2/1 (`FUN_004b8cd0`) | code |
| MEMBER_SETUP | `object_flags` `draggable`, `drag_movable`, `locked`, `intangible`, `starts_hidden`, `asleep`, `immovable`, `no_wander` | `FUN_006cb310`: `FUN_006857f0`/`FUN_0069ee00`, `FUN_006c49b0`, obj+0xb30, obj+0x24e, `FUN_006a7470(obj,0,0,1)`, body+0x80 \|= 3, obj+0x20f/`FUN_006c44d0`, obj+0x68c bit 6 | code |
| MEMBER_SETUP | `grabbable`, `always_sees_target`, `drop_contents_in_place`, `has_persist_ids`, `has_more_flags` | byte 2: bits 0-1 obj+0x24d, bit 4 `FUN_00653b30`, bit 5 obj+0x1a4 bit 4, bit 6, bit 7 (`(char)bVar3 < 0`) | code |
| MEMBER_SETUP | `unused_bits` (byte 2 bits 2-3), `unused_bit0`/`unused_bits_2` (byte 3) | never tested in `FUN_006cb310` | unused |
| MEMBER_SETUP | `mirrored`, `has_wander_range` (byte 3 bits 1, 2) | `local_20 = bVar14 >> 1 & 1` (`FUN_006bf3d0`), `bVar14 >> 2 & 1` -> `FUN_006639b0(...)` | code |
| MEMBER_SETUP | `ai_target`, `ai_target_relation` | `FUN_00653b70` (AI+0x80, AI+0xdc) | code |
| MEMBER_SETUP | `persist_ids` (10) | n-th member gets `pbVar10[2 + group+0x38]` -> obj+0x2a8 | code |
| MEMBER_SETUP | `wander_left`, `wander_right` | `FUN_006639b0((b0 \| b1) << 12, (b3 \| b2) << 12)` (the engine ORs the bytes) | code |

### Object relations

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| OBJECT_LINK (bit 6) | `attached`, `carrier`, `kind` ride/held/equipped/stuck | `FUN_004aea80` -> `FUN_006af730(inst(a), inst(b), kind)`; data: hats -> Maxwell (equipped), balloons -> clowns (stuck) | code |
| CONTENTS (bit 7) | `contents`, `container` | `FUN_004aeb10`: `container.FUN_00672060(contents, 1, 1)` (asm 0x4aeb56-0x4aeb6c); moneybag -> safe | code |
| ROPE_ATTACHMENT (bit 8) | `end` (1 start = rope+0x6cc first segment, 0 end = +0x6d0), `rope`, `object` | asm 0x4aebd6; `FUN_005ebf50`; `FUN_004d3980(end_segment, inst(object))` | code |
| JOINT (bit 9) | `type` pin/weld/elastic/motor/motor_limited; `none_1/4/5` | `FUN_005cec50` switch; 1, 4, 5 fall to `default:` (no joint) | code |
| JOINT | `anchor_x/y` (16.16 tiles), `stiffness` (%, >= 100 rigid), `motor_speed`, `min/max_angle` | `FUN_005cec50`; anchors equal the objects' positions | code |
| JOINT | `object_a`, `object_b` (instances, -1 = player `DAT_008a8700+0x184`) | `FUN_0066d4e0` | code |

### Level feature sections

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| LIQUID (`FUN_004b3220`) | `kind` water/lava (low nibble) | `if ((b & 0xf) == 0) local_40 = 2; else if ((b & 0xf) == 1) local_40 = 3`; `FUN_00739820` `+0x4d = type == 3` | code |
| LIQUID | `unused_bit4` | never tested | unused |
| LIQUID | `styled`, `has_flow`, `full_width` | gated reads; full width: x 0, map width, height - y (`local_38 = b >> 7`) | code |
| LIQUID | `flow_speed` | body+0xe4, `FUN_00739de0` `+0x80 += speed` | code |
| LIQUID | `*_texture` | `FUN_005e2d30` defaults 0x69ef watercolor, 0x69f0 waterdistortion, 0x69f1 waterfresnel, 0x69f2 watermeniscus, 0x69f5 water_center, 0x69f6 water_fade01 | code + text |
| LIQUID | `wave_speed` (+0x60), `wave_damping` (+0x64) | spring / damping terms in `FUN_00739de0` | code |
| LIQUID | `unused_wave_param` (+0x68) | only store at 0x7398be | unused |
| LIQUID | `surface_color` (+0x58), `body_color` (+0x5a) | `FUN_004ae190` skips a byte, packs rgb 555; `FUN_00738f50` top / lower vertices | code |
| LIQUID | `unused_*_color_byte` (x3) | skipped by `FUN_004ae190` (`*param_1 = *param_1 + 1`) | unused |
| LIQUID | `unused_color` (+0x5c) | copied (0x5e3078), never read | unused |
| LIQUID | `alpha` | `*31/255` -> +0x5e; `FUN_00737f00` `min(alpha, 2*x)` | code |
| RAIL_PATH (`FUN_005b7d20`) | `direction` forward_only/backward_only/both | `FUN_005bbd00`: `if (*piVar6 == 1 && +0xe8 != 0) return; if (*piVar6 == 2 && +0xe8 != count-1) return` | code |
| RAIL_PATH | `flags` `unused_bit0` / `has_easing` (+5, `FUN_005b7910`) / `has_start_box` / `has_end_box` / `has_ease_steps` | parser tests bits 1-4 only | code / unused |
| RAIL_PATH | `speed_*` (1..100 -> +0xc/+0x10), `anim_*` (+0x14/+0x18, `FUN_00665570` -> `FUN_00667e00`), `ease_in/out` (+6/+7) | `FUN_005b7d20` | code |
| Rails | `survival_paths` (`unused` byte + points) | `FUN_004aecf0` skips the byte; `FUN_007056e0` -> survival manager `+0x118[i]` | code / unused |
| Rails | `survival_defenders`: `defender_positions`, `has_unused_bytes`/`unused_byte` | `FUN_00705760` appends all points to +0x684, `FUN_00704a80` defender i at point i; bytes stored at mgr+0x5e8+i (0x7057d8), only other access zero-init 0x7080db | code / unused |
| DECORATION (`FUN_006f4220`) | `kind` object/vector | `if (cVar1 < 1) FUN_006f3810 else FUN_006f3f20` | code |
| DECORATION | kind-0 `mirrored` (bit 1) | `FUN_006e9010` param_5: `+0x14 = -scale` | code |
| DECORATION | `layer`, `depth` | key `depth & 0xfff \| layer << 12` (`FUN_006e9010`, `FUN_006f3f20`) | code |
| HINTS (`FUN_004af400`) | `unused` lead byte | `*param_2 = *param_2 + 1` | unused |
| HINTS | `progress_segments` | `FUN_00454010` -> `FUN_0047f020`, pieces per `__ufsprogress` segment | code + text |
| HINTS | `group_sizes` | `FUN_004d2260` -> +0x2c | code |
| HINTS | `unlock_delay` (s) | `FUN_004d2180`: `param_2 * 1000` | code |
| HINTS | condition `kind` entity/zone, `entity`, `zone` | `FUN_0047e600`; entity `FUN_0047e650`, zone `FUN_0047e690` (zone object +0x514) | code |
| AREA (`FUN_004b3830`, `FUN_004b39f0`) | `unused` lead byte | `*param_3 = *param_3 + 1` per entry | unused |
| Scene | `no_drop_zones` (bit 17) | `FUN_0053bb50`, `FUN_005e74a0` | code |
| Scene | `merit_zones` (bit 20) | `FUN_004b39f0` -> `FUN_004b0830`: `zonefillbox` (0x6999) at the centre, +0x514 = index | code |
| SURVIVAL_WAVE (`FUN_00702de0`) | `min/max_count`, `objects`, `weapon_chance`/`weapons`, `adjective_chance_N`/`adjectives_N`, `spawn_paths` | `FUN_00707430` spawns | code |
| SURVIVAL_WAVE | `health`, `damage` | copied (+0x1c/+0x1e); enemy vtable +0x84 (0x700090, body+0x1f8/+0x1fc), +0x90 (0x700100); defaults 25/10 | code |
| SURVIVAL_WAVE | `has_unused_58`, `unused_58` | stored wave+0x58 (0x702ea5), no read | unused |
| LIGHT (`FUN_004b6a30`) | `unused` (4 bytes) | `*param_3 = *param_3 + 4` | unused |
| LIGHT | `aperture_texture`, `attenuation_texture` | lightshaft.gp samplers `apertureTexture`, `attenuationTexture` | text |
| LIGHT | `x`, `y`, `direction_*`, `angle`, `start_distance`, `end_distance`, `brightness`, `shimmer_rate_*`, `color` (a r g b) | lightshaft.gp attributes `aPosition`, `aDirection`, `aParam` (x angle, y/z start/end distance, w brightness), `aRate` (aperture scroll), `aColor` | text |
| LIGHT / ATMOSPHERE / EFFECT | `layer`, `depth` | key `depth & 0xfff \| layer << 12` (lights clamp layer <= 3) | code |
| ATMOSPHERE (`FUN_004b7730`) | `unused` (4 bytes), detail `unused` byte | `*param_3 = *param_3 + 4`; detail texture read at +1 | unused |
| ATMOSPHERE | `mask_texture`, `brightness`, `color`, `detail_N` `texture`/`velocity_*_M`/`period_M` | atmosphere.gp uniforms `maskTexture`, `brightness`, `color`, `detail<N>texture`, `detail<N>velocity<M>`, `detail<N>period<M>` | text |
| EFFECT (`FUN_004bb8a0`) | `paused` | `FUN_00600150(effect, ~flags & 1, 0)` (playing state); bits 1-7 untested | code |
| EFFECT | `effect`, `x`, `y`, `rotation` | `.gec` placed at the position | code |
| DOOR (`FUN_004b3ca0`) | `x`, `y`, `width`, `height` | trigger box | code |
| DOOR | `scene`, `setup`, `tile_map`, `merits` | `FUN_004e1a50(tile_map, scene, setup, merits, 0)` | code |
| DOOR | `unused_dependencies`, `unused_preload` | record bytes 0x21-0x28 never read (loader continues at +0x29) | unused |
| DOOR | `preview` (+0x38), `script` (+0x48, `has_script`) | door trigger fields | code |
| DOOR | `decoration`, `open_sequence`, `close_sequence` | `FUN_004de700` shows the close sequence; `FUN_006ef830(+0x14[1])` on touch (state 3 -> 0), `+0x14[2]` on leaving (1 -> 2) | code |
| DOOR | `has_unused_offset`, `unused_offset` (8 bytes) | `if (local_8d != 0) *param_2 = *param_2 + 8` | unused (data: (0, -25) in 20.12) |
| DOOR | `open_sound`, `doorway_sound`, `close_sound` (-1 none) | `FUN_004de5f0` state 3 -> 0 `**(+0x18)`, `FUN_004de2f0` while in state 1, state 1 -> 2 `(+0x18)[2]` | code |

## Data-only claims (no code path)

* `.tle` paths trace walkable surfaces: stored at level+0x2b4, no reader found (searched `+0x2b4..+0x2bc` operands).
* `.tle` `alternate_pages` is a denser page layout: skipped by the loader; inferred from the grid sizes.
* `.nbtc` `unused_trailer` as a second pair of neighbour tables: never read; inferred from size and values.
* `.stp` `unused_water_rows` and `unused_start_position` meanings: never read; inferred from values.
* `.mdb` `unused_kind` classes: never read; inferred from which merits carry 255 / 3 / 1.
* `.sod` door `unused_offset` as an (x, y) pair: skipped; inferred from the values.
* `.sod` `unparsed_sections` of `e1_dunes.sod`: no PC code parses it; kept verbatim.
