# Placeholder art (`gift.vec`) and ropes

Some `.so` objects draw a present (`[platform]\datavector\misc\paper\gift.vec`, resource 22403)
or nothing at all in a naive viewer. This ledger records how the game actually renders them and
what Scribble Studio shows instead (`crates/scribble-studio/src/object_resolve.rs`).

## Census

| Set | Count | How counted |
|---|---|---|
| `.so` containing the bytes of u32 22403 | 88 | raw byte search; one is a coincidence (`human_other_wedding_bridesmaid.so`, not a reference) |
| `.so` referencing `gift.vec` (node tree or dependency list) | 87 | decoded JSON |
| `.so` whose node tree draws only `gift.vec` | 68 | walk of `body`/`female_body` vector nodes (`vector` + `variants`) |
| ... minus the real present `misc_paper_thick_gift.so` | **67** | `object_resolve::is_gift_placeholder`; checked by `tests/placeholders.rs` |

The other 19 (ghost, coffin, urn, funeral home, shaman, cloning machine/gun, teleporter,
seesaw board... and the `human_developer_*` characters) draw their own art; `gift.vec` is only
in their dependency (preload) list, next to the other things they can spawn or turn into.

The 67 placeholders:

| Category | Files | Game behaviour | Studio |
|---|---|---|---|
| Avatars | 48 `human_player_avatars__*` (4891-4939 minus `scribblenaut`, which has real art) | never spawned; legacy of the avatar feature | the NPC the same word spawns |
| Rope pieces | `tool_rope_pieces__ballandchain`, `ballandchainend`, `flossend`, `lassoend`, `nunchuk` | never spawned; legacy piece-by-piece ropes | one segment of the matching textured rope (or the object that draws the art) |
| Engine stand-ins | `_adjective_adjective_adjective1..3`, `_self_self_me`, `_stage_stage_stageobject` (and `_self_self_myobject`, which has no vector) | never spawned; relation/filter targets | nothing, with a note |
| Unreferenced leftovers | `misc_wood_splitparts__{archleft,archright,entertainmentcenterleft,entertainmentcenterright}`, `entertainment_wood_huge__seesawboard`, `gameplay_gameonly_editor__{clipart,goal}`, `gameplay_gameonly_level__chainobject`, `gameplay_stone_normal__hauntedhousebookshelf` | no code or data reference found: if spawned, the gift is what the engine draws | the gift, with a note |

Besides placeholders, every **rope object** (e.g. `tool_rope_other_rope.so`,
`tool_restraint_other_chain.so`, Christmas lights, whip, leash: 27 textured ropes) has no `.vec`
at all and drew nothing before; they and **rope anchors** (ball and chain, tow truck, crane
helicopter, rope/tire swings) are now drawn with their rope textures.

## Ropes

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|
| `.so` vector node | `rope_texture` set => the node builds a rope renderer, not a `.vec` | `FUN_006b6c00` case 4: flag bit 2 read as `local_129`; when set, the `else` branch calls `thunk_FUN_0060f100(&desc)` and stores the result at `+0x6d4`; no `FUN_006b4e40`/`FUN_006b4ea0` vector is built | code |
| rope renderer | length = `rope_segments * 16` world pixels | same branch: `local_c4 = *(uVar31 + 500)` (`+0x1f4` = `general.rope_segments`), `local_d8 = (float)(local_c4 << 4)`; `FUN_0060ee10(length, ..., count)` divides by the count (`param_2 / (float)iVar3`) | code |
| rope renderer | the texture is the node's `rope_texture`, 3 bands | `desc+0x18` = texture, `desc+0x1c` = 3 -> `FUN_0060c740(tex, 3)` -> `FUN_004cd6a0(tex)` and `FUN_006100c0(..., 3)` -> `FUN_0060fc50(3)`: band (scale, offset) pairs `(0.2, 0)`, `(0.2, 0.4)`, `(0.2, 0.8)` (`DAT_008250a4` = 0.2, `DAT_00831fb8` = 0.4, `DAT_00835748` = 0.8) | code |
| rope shader | `v = (1 - v) * texscaling.x + texscaling.y` selects a band; `u` spans cap + segment + cap | `[platform]\gpuprograms\rope.gp` (`FUN_0061d9c0` loads `GPUPrograms/rope.gp`): `d = 1/(2*a_scale.x + L)`, `u = d*(a_param1.z*a_scale.x + a_param1.w*(a_scale.x + L))`; each segment is a Catmull-Rom span between control points 1 and 2 | code |
| `*landr` textures | band 0 (v 0-0.2) = repeating middle, band at 0.4 = start (frayed/capped left), band at 0.8 = end (capped right) | all 27 `_scribeffects\rop\*landr` DDS are 512 x 640 with content only in those three 128-px rows; the frayed ends are drawn at the left of the 0.4 band and the right of the 0.8 band. Which physical segment uses which band is inferred from the art (not traced in code). | data-only |
| `*landr` textures | cap length ~ 0.2 x segment | the middle band's body runs from texel ~75 to ~440 of 512 in every texture, i.e. `cap / (2 cap + L) = 0.146`; the studio uses `cap = 16 * 75/362` and keeps the band's 4:1 aspect (`object_resolve::ROPE_CAP`). The descriptor's `+4` = 2.5 (`DAT_008294b4`) would leave gaps between links; it is probably the physics radius. | data-only |
| rope node (type 12) | holds the renderer; its transform is the rope's frame | `FUN_00680150` stores `FUN_005d7130(..., obj+0x6d4)` at `+0x54`. `furniture_appliances_light_christmaslights.so` rotates the vector node 180 and its rope node -180; only the rope node's frame gives hanging bulbs | code |
| `rope_anchor` hotspot | spawns `rope` there with `segments` (0 = the rope's own count) | `fmt_object::tree::HOTSPOTS` (`FUN_004d2870`); the rope leaves along the hotspot's local -y: ball and chain (-90) to the left, crane helicopter (180) down, tow truck (-120) down-left, tire swing (0) up | code (payload) / data-only (direction) |
| `.so` rope objects | `width`/`height` = 512 x 640 | every textured rope object stores its texture size as its body size | data-only |

Studio: `object_resolve::tree_ropes` draws textured rope nodes as a gently sagging rope of
`rope_segments` segments along the rope node's x axis, and rope anchors as the anchored rope
hanging from the hotspot; both are textured quads (`TexQuad`) painted behind the object's art.
In game the ropes are simulated (verlet nodes, `FUN_0060ee10`), so the shape is only a preview.

## Rope pieces (`tool_rope_pieces__*`, 6774-6836)

| Claim | Evidence | Kind |
|---|---|---|
| The pieces are leftovers of piece-by-piece ropes; SU ropes are single textured objects | 45 pieces have a rope node with `rope_texture` = none (`0xFFFFFFFF`) and small DS-era sizes (e.g. 24 x 8), while the real ropes carry a `*landr` texture; orphan piece art (`lassopiece.vec`, `nunchuckspiece.vec`, `leashpiece.vec`, ...) is still shipped | data-only |
| The gift pieces are never spawned | no `.so`/`.sa`/`.sod`/`.dps` references `ballandchain(end)`, `flossend`, `lassoend` or `nunchuk` (decoded tree grep); their resource indices / word ids appear in no code constant. Words are hidden `@` words (`@BALL AND CHAIN`) | data-only |
| The engine spawns pieces by index only for grapple heads | `FUN_0067b8a0`: word 0x9de (fishing pole) -> 0x1a54 `tool_restraint_other__fishinghook.so`, 0x9df (grappling hook) -> 0x1a8a `tool_rope_pieces__grapplinghook.so`, 0x1ca0 -> 0x1a8b `grapplinghookend` (both have real art) | code |
| Some pieces are reused for other purposes | `naked.sa` equips `tool_rope_pieces__ropeend.so` (censor bar); `tool_rope_pieces__ropeladder.so` uses `shoelaceend` as a wheel | data |

Studio: a placeholder or art-less piece `X` is drawn as one middle segment of the textured rope in
`object_resolve::ROPE_PIECES` (`ballandchain` -> `tool_restraint_other_chain.so`, the rope that
`tool_rope_other_ballandchain.so`'s anchor spawns), `Xend` as its end segment;
`lassoend` as `tool_rope_grapple_lasso.so` (which draws `lassoend.vec`). Pieces with no textured
rope in the game (`rein`, `reinend`) show nothing, with a note.

## Avatars (`human_player_avatars__*`, 4891-4939)

| Claim | Evidence | Kind |
|---|---|---|
| The player-avatar table does not contain them | `FUN_00445ba0` fills the 44-entry table at `DAT_008a5690` (16 bytes: object resource, custom id, name word, scale 0x1000/0xb33): 0x1374 Maxwell, 0x134c-0x1373 `human_player_brothers_*`, 0x105e/0x105a/0x105d Lily/Edgar/Julie. Accessors `FUN_00446620` (resource), `FUN_00446830` (count), `FUN_004466d0`/`FUN_00446680` (lookup) | code |
| The avatar menu picks from that table or from custom objects | `FUN_00596540` lists `FUN_00446620(i)` for unlocked entries, then custom objects (`FUN_0057b390`) tagged `| 0x8000`; `FUN_005963f0` passes the choice to `FUN_0055de50(resource, 1)`, which stores it as the player object (`+0x58`/`+0x5c`) and respawns; `FUN_00596140` resets to 0x1374 (Maxwell) | code |
| No code or data spawns an avatar object | none of the 49 resource indices or word ids occurs as a code constant (the only numeric matches are a protobuf line number and colour-conversion arithmetic); besides the dictionaries and `scribbleobject.odt`, avatars appear only as relation/filter targets (`human_athletes_support_groupie.so` follows `human/player/avatars/_rockstar` and the NPC rockstar; `mummified.sa`, `buildingobjects_buildings_other_lair.so`, `s_mine.sod`) | data-only |
| Their words are hidden | every avatar word starts with `@` (`@BALLET DANCER`); `FUN_0069c970` blanks an object's display name when it contains `@` or `$` (except Maxwell 0xb31 and Edgar/Julie/Lily) | code |
| They were cloned from the NPC of the same name | 39 avatars share 30-37 of their ~36 animation-table entries with the object that the plain word spawns, including species-specific sets (vampire, cyclops, god, hero, robot, clown, zombie); 9 (admiral, artist, Benjamin Franklin, Cleopatra, doppelganger, goth, illuminati, president, redcoat) carry Maxwell's `maxwell_*.anim` table instead. The budget block is identical in all 48 (cost 25, 258 nodes, 11800 vector bytes) - a template, not measured art | data-only |
| The one avatar with art is Maxwell's rig | `human_player_avatars__scribblenaut.so` draws `maxwellblank_texture.vec` with 6 mesh parts | data |

So the game has no appearance for these objects beyond the gift (there is no avatar atlas such
as `maxwell_<avatar>_texture.vec`, and `[platform]\_game\_challenge\textures\human\player\` holds
only `maxwell_texture_maska`). Studio: the avatar is drawn as the object its plain word spawns
(`@BALLET DANCER` -> `BALLET DANCER` -> `human_entertainment_dancer_dancer.so`; a candidate with the
avatar's own object name wins, so `@HERO` is the superhero and not the sandwich; `@HAIR DRESSER`,
which has no plain twin, falls back to the object named `hairdresser`), animated with the
avatar's own table, starting on the female body when that object's `default_gender` is female.

## Engine stand-ins

| Object | Meaning | Evidence | Kind |
|---|---|---|---|
| `_adjective_adjective_adjective1..3` (words 0x1574-0x1576) | relation targets for the object's adjective slots (`investigate _/adjective/adjective/adjective1` is in almost every object) | `FUN_00658180`: relation kinds 1-3 set the target word to 0x1574 / 0x1575 / 0x1576 | code |
| `_self_self_me` (0x18d1) | "the owner itself" in filters | `FUN_006766b0`: word 0x18d1 matches when `param_2 == param_3` | code |
| `_self_self_myobject` (0x18d2, resource 7681) | "the owner's type" in filters and spawn actions | `FUN_006766b0`: 0x18d2 compares the objects' type words (`+0x160`); `FUN_0053e890` (also `FUN_00545270`, `FUN_0054ff00`, `FUN_00550d20`): a spawn of resource 0x1e01 is replaced by the owner's own resource (`+0x16c`) before anything is created | code |
| `_stage_stage_stageobject` (0xd1d) | relation to a stage position, not an object | `FUN_00655d60`: a relation whose word is 0xd1d targets the stored position `+0xe4` | code |

None is spawned as itself (resource indices 7676-7680 and 7695 appear in no code constant; 7681
is always substituted, see above), so the studio draws nothing and shows the meaning.

## Still open

* Which segment of a rope uses the start vs. end band is read from the art, not traced
  through the vertex buffer (`FUN_006100c0` / `FUN_00797580`); the cap/thickness are inferred from
  the texture layout. The shape of a rope at rest is a preview (the game simulates it).
* The 9 unreferenced leftovers (split parts, seesaw board, editor clip-art/goal, chain object,
  haunted-house bookshelf) have no known consumer; if the object editor or a script spawns them
  by a path not found here, their appearance is still unknown.
* Rope pieces with `rope_texture` = none and no matching rope (`rein`, `reinend`): what the
  renderer does with a missing texture (`FUN_004cd6a0(0xFFFFFFFF)`) was not traced.
