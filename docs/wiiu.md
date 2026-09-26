# The Wii U build and the Wii U content port

The Wii U release (title `00050000-1010B200`, USA, v21521) has content the PC release lacks:
the Nintendo easter eggs. `crates/scribble-wiiu` reads the Wii U build and ports that content
into a PC install. `wiiu-port-installer` builds it from the player's own Wii U dump, so no Nintendo
assets are distributed.

## Getting the Wii U files

The tools take a decrypted dump in the usual `code/ content/ meta/` layout, or just its `content`
folder. Dumping tools for your own console write this layout, and so does decrypting a disc image
with its keys. Only `content/index.bin`, `content/1s` and the `.p` packs are read.

```sh
# Unpack the Wii U build into the same kind of tree as `scribble unpack` (manifest "platform": "wiiu")
scribble unpack-wiiu <dump> wiiu/extracted --pc extracted
scribble decode-all wiiu/extracted wiiu/decoded

# Install the Nintendo content into a PC install (also: --uninstall)
wiiu-port-installer --game <PC game folder> --wiiu <dump>
cargo build --release --target x86_64-pc-windows-gnu -p scribble-wiiu   # Windows .exe
```

## How the builds differ

| | PC | Wii U |
|---|---|---|
| `index.bin` | little-endian | big-endian (`IndexBin::read_be`), same layout |
| pack header `+4` | 0 | 1 on `first.p`, `sfx.p`, `objects.p`, `1.p`: load the whole pack into RAM (`FUN_004944a0`) |
| `pmindex*.xml` | present | absent. Paths are rebuilt from `1s` symbols (`wiiu::PathGuesser`) |
| `1s` | little-endian | little-endian |
| packs | objects, 1, 2, audio, ui, _aux | first, sfx, objects, 1, music, dummy |
| text tables / event scripts | 7 languages | 4: English, French, Portuguese, Spanish (`text::WIIU_LANGUAGES`) |
| word dictionaries | 13 | the same 13; the Nintendo words are in all 9 current-layout ones |
| textures | DDS | DDS |

Everything else is byte-compatible. Of the 28,924 resources with data in both builds, 16,577 are
identical and all but 5 of the rest decode with the PC codecs. The Wii U build inserted its new
resources in the middle of the index, so most of the "changed" files differ only in renumbered
references. The 5 that fail are main-menu `.uib` layouts using element types 97 and 110, which PC
lacks. The event-script language count cannot be read from the file, so manifests of Wii U trees
carry `"platform": "wiiu"`, which reaches the codecs as `Context::platform`.

Stale indices in the Wii U data: the `launch_point` hotspot with `particles` set, and a few
`spawn_object`/`fire_projectile` objects, store particle **stream ids** (`FUN_00474f60`: 0xb35 water,
0xb3f fire, 0x13bd snow, …). These were never renumbered and must stay as they are. The hotspot
codec now reads them as `stream_type`. `port::transcode` refuses to renumber any `STREAM_IDS` value.

## What the port installs

The PC release shipped with the content stripped but traces left behind:
- empty `_lm7`/`_lm8` adjectives
- a grappling-hook end with no art (`0xFFFFFFFF`)
- 71 sound-table slots with no sound
- `firebreathing` still naming Bowser's taxonomy id
- `luigihat.sfb` still in `1.p`

All Wii U taxonomy ids (subcategory 7299 `nintendo`, groups 7300/7301, objects up to 7351, adjectives
2061–2063) are unused on PC, so they are kept as they are.

* **372 new resources**, appended after the last PC index (31029–31400, below the user-object bit
  0x8000):
  * 42 `.so` (33 characters and 9 items in `easteregg_nintendo_*`)
  * `weapon_projectile_magic__fireball`
  * 3 adjectives (`_fireflower`, `_fireflower2`, `_superstar`)
  * 55 `.vec` (`[platform]\datavector\_nin\`)
  * 201 `.anim` (`data\meshanim\_nin\`)
  * 71 voice/SFX `.wav` (Bink Audio, as on PC)
* **Replaced** by their Wii U version (`port::REPLACED`). Each is the PC file plus Nintendo parts:
  * `_lm7`: Super Star — invincible, flashing, destroys enemies on touch, double speed, music loop
  * `_lm8`: Super Mushroom — growth flicker, then `super`
  * `super`: growth effect under `_lm8`/`_lm9`
  * `_dead`: flattened Goomba
  * `_voodoo`: chickens attack Link
  * `tool_rope_pieces__grapplinghookend`: Hookshot hook art
* **Merged**:
  * **Dictionaries.** The 9 dictionaries the engine reads get:
    * the Nintendo words
    * Nintendo meanings added to shared words (`PEACH` → fruit or princess, with choice labels)
    * the Wii U's clarifying spellings of the old meanings (`FOOD PEACH`, `MONEY COIN`)
    * english_uk's `BAND` moved off `_lm7`, as on Wii U

    Other dictionary differences between the builds are localisation revisions and are left alone.
    The Wii U build reassigned some stray english_uk words (`PIPELINE` → `_superstar`); these are
    left too.
  * **`scribbleobject.odt`:** the 42 new objects' entries.
  * **`audiometadata.aaf`:** the 71 empty slots.

The install writes `wiiu_content.p` and rewrites `index.bin`, `pmindex.xml`, `pmindex_for_code.xml`
and `1s`. The originals go in `wiiu_port_backup/`, and uninstalling restores them byte-for-byte.
If Steam's file verification resets the game, run the installer again.

Verified on the patched install:
- every resource round-trips
- the 372 new files and 6 replaced files decode to exactly the Wii U JSON
- every other file decodes to exactly the PC JSON, apart from the merged tables
- every dictionary word the port changes equals the Wii U word

## What still differs: Wii U-only engine code

The Wii U executable (RPX, PowerPC) was compared with PC's `Scribble.exe`.

**Parsers match.** The action factory (PC `FUN_0064e1f0` / Wii U `0x2330544`), the behaviour
factory (`FUN_006d36e0` / `0x23370bc`) and the modifier factory (`FUN_0068d970` / `0x22099c4`)
accept the same type ids. So the Wii U data cannot use an action, behaviour or modifier PC lacks.

**The "UpdateObject" family.** These are per-frame helper classes (`C_UO*`, factory PC
`FUN_006c5d10` / Wii U `0x2329cc8`). Types 0–6 exist in both builds: EventRep, Balloon,
ContainerInpterolation, Rope, RumbleBomb, RumbleWeaponPickup, MovementSFX. Types 7–9 exist only on
Wii U. They are created by object init (Wii U `0x23cbb40`, PC `FUN_006b5f10`) from the object's
taxonomy id:

| Wii U only | object | what it adds | without it on PC |
|---|---|---|---|
| `C_UOSuperStar` (7) | superstar (7323) | every frame: `vx = ±1.0` starting rightwards, reversed on a wall contact; `vy = −5.5` on a floor contact (update `0x2325228`) | the star lies still; its power-up works |
| `C_UOSuperMushroom` (8) | supermushroom (7334) | slides at `vx = ±1.0`; at a wall it reverses with a `vy = −1.0` hop (`0x232544c`) | the mushroom lies still; its power-up works |
| `C_UOFireFlowerBall` (9) | `_fireball` (7349) | on a terrain contact while falling `vy *= −0.8`; killed when the contact normal has `x < −0.5` (`0x2324f58`) | the fireball rolls to a stop instead of bouncing |

Other id checks only the Wii U build has:
- **Fireball (7349):**
  - Object init forces its physics collision class (`body+0x82`, PC `+0x86`) to 6 (`0x23dfc84`). On
    PC `FUN_0069c710` gives class 6 to *intangible* objects. The `intangible` property (`+0x24e`)
    has no other gameplay use, so the fireball passes through objects it doesn't burn.
  - It is exempt from "fire-material objects die on entering liquid" (Wii U `0x23e405c`, PC
    `FUN_006a4540`), so on PC it fizzles in water.
- **Luigi (7315):** his cap's `luigihat.sfb` flipbook uses `C_ScribbleFrameSFAnimationFlipped`
  (`0x23a2b50`), which mirrors it when he turns. With PC's plain class (`FUN_006b6c00`, node
  case 9) the "L" turns upside-down.
- **Chain Chomp (7306):** grab amount forced to 0 (`0x2385e1c`).
- **Goomba (7311):** skips animation state 0x10 (`0x2395190`).
- **Subcategory 7299 (all Nintendo objects):** 8 functions restrict them. They reject adjectives
  unless forced (`0x2376420`) and are excluded from AI targeting (`0x2371ea4`), possession by
  extra Wii Remote players, some trigger filters and an adjective reset. These look like licensing
  rules; without them the objects are simply less restricted on PC.
- **Hookshot (7328):** handled like the grappling hook, lasso and fishing pole. **PC has this code
  already** (`FUN_0067b8a0`, `FUN_006b6c00`, `FUN_006b5310`, `FUN_00554850`).

No Wii U code references the new adjectives or the Nintendo groups, and nothing gates the content
behind an unlock, amiibo or Miiverse (the RPX imports no `nn_olv`/`nn_nfp`).

## Emulating the item physics in data

By default the installer adds data stand-ins for the star and the fireball (`--no-emulation`
leaves them out; see `crates/scribble-wiiu/src/emulation.rs`). Each object applies a new hidden
adjective when it is created. The adjectives are `_wiiuport_superstar` and `_wiiuport_fireball`,
with the next free adjective ids (2064, 2065). The adjectives work on every language's dictionary
without entries, as the shipped Nintendo ones do. The stand-ins use three PC engine features:

* **`bouncy`** (property 0x43: physics flags `0x40100`). For a bouncy body, the contact solver
  (`FUN_005c2de0`) sets the target separation speed to the incoming normal speed, clamped to
  4.0–7.0 (`0x4000`–`0x7000`, doubled between two bodies with mass). A bouncy object therefore
  keeps bouncing at a steady height, and the star's −5.5 is inside that range.
* **`intangible`** (property 0x40): collision class 6, as the Wii U forces for the fireball.
* **`apply_force`** adds `2.5 × force` to the velocity (`FUN_0053e440`; `force_y` is quartered
  for non-character bodies; with `relative` the force follows the source's facing and angle). The
  star gets `force_x = 0x666/0x1000`, which gives the Wii U's initial `vx = +1.0`.

| | Wii U | PC with the stand-ins |
|---|---|---|
| star, vertical | bounces at 5.5 | bounces at 4.0–7.0, steady |
| star, horizontal | 1.0 every frame; turns at walls | starts at 1.0 to the right; floor friction slows it; walls send it back at ≥ 4.0 |
| fireball | bounces, 20% lower each time; dies at walls | bounces steadily; ricochets off walls; still expires after its 120 frames |
| fireball vs objects | passes through the ones it doesn't burn | same (intangible) |

Not verified in game: how much horizontal speed floor friction takes from a bouncing star; that
class-6 objects still fire `on_collide` for objects (they must, since the Wii U fireball
damages what it touches); that `obj+0x800`, which `FUN_0069c710` requires before class 6, is set
for these objects. The mushroom's slide is not emulated.
