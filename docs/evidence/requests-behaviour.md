# Requests for the shared behaviour schema (fmt_object::behaviour)

Appended by other codec owners; the fmt-object owner applies or rejects them.

## From fmt-map (`.sod` scene scripts)

| Schema entry | Current name | Proposed | Evidence |
|---|---|---|---|
| action 0x31 `ai_request`, `request` enum (`AI_REQUESTS`) | numeric `30`, `31` (no name; 110 and 168 uses in 33/37 `.sod` files, always with only `target`) | name them once confirmed, e.g. 30 = `path_move_to_target`?, 31 = `stop_path_move`? | No `REQUEST_TYPE_*` debug name: the name switch at `0x6546f0` (jump table `0x654c60`) sends 27..32 to the default. The action's run method `FUN_0054cc50` treats 0x12/0x1e/0x1f alike (need a target entity, passes `target+0x154` to `FUN_00655c80`). The AI request dispatcher `FUN_0065e0e0` maps 0x1e -> AI mode 0x27 with `local_b0 |= 2` and the target position (`local_b8 = *param_4`), 0x1f -> mode 0x28 with `local_b0 &= ~1`; the AI mode name table (`0x654e80`, jump table `0x655598`) names mode 0x27 `AIMODE_PATHMOVEMENT` if its numbering is the same (0x28 is past its end). `FUN_0065c250` classifies both as 2 (like 0x1d `move_to_target`). |
| action 0x08 `change_mood`, `mood` (`MOODS` 0..6) | numeric `7` (1 use: `data\mapdata\sandbox\_sea\s_skycastle.sod`) | a name for mood 7 if the mood code has one | Only data so far; check the mood switch reached from the `change_mood` class (parser `0x549950`). |
| `fmt_object::record::entity_to_json` (`{"object": n}` entities of scripts, merit rules, hints) | doc: "placed object index" | doc (or value shift): n is the engine **instance id**, = `.sod` `objects` index **minus 1** | `FUN_004bdab0` creates placed object k with instance id k-1 (`FUN_006997d0(obj, ..., loop_index - 1)`, asm 0x4be4a6 `dec edx; push edx`); the stage entry (always `objects[0]` in all 122 scenes) has none. Entities with high byte 0 resolve through `FUN_006a1770`/`FUN_006a1610` -> `FUN_0047a390` (instance table `DAT_008a5bc0`). Checked on fmt-map's raw object numbers, which use the same ids: every rope attachment's `rope`+1 is a rope/chain (20/20), link pairs read right only with +1 (hat -> maxwell), joint anchors equal `objects[a+1]` positions. fmt-map documents its fields as instance ids and keeps raw values to stay consistent with `{"object": n}`; if you shift entities by one, tell fmt-map to shift too. |

## Responses (fmt-object owner)

| Request | Decision | Evidence |
|---|---|---|
| `ai_request` requests 30 / 31 | **Kept numeric** (the proposed path-movement names are contradicted by the code). | `FUN_006531a0` (the request entry called by the run method `FUN_0054cc50`) handles 30/31 itself: it clears / sets bit 6 of object `+0x68c` and returns without queuing an AI task. The same bit is set while a player controls a creature (`FUN_004a0940`) and read by the AI idle code (`FUN_00659970`); no text or debug string names it. |
| `change_mood` mood 7 | **Kept numeric** (no such mood). | `static_aimood` has 7 entries (0-6); the parse `FUN_00549950` only remaps 2 -> 5 and the run method `FUN_00549880` acts only on 3 (sleepy) and 4 (sick), so 7 does nothing. Noted in the `MOODS` doc. |
| `entity_to_json` `{"object": n}` doc | **Applied as a doc change** (values stay raw, consistent with fmt-map). | Verified: `FUN_004bdab0` calls `FUN_006997d0(obj, ..., k - 1)` (asm `0x4be4b3 dec edx; push edx`), which stores the id at `obj+0xc`. Docs of `record::entity_to_json`, `refs::Entity` and the `behaviour` module now say "instance id = `.sod` `objects` index - 1". |
