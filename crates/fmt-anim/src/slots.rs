//! Animation slots: how the engine picks which `.anim` to play.
//!
//! The engine never looks animations up by file name. Each scribble object (`.so`) carries an
//! animation table of `(u8 slot, u32 .anim resource index, u8 has_events, [u16 action,
//! u16 aim_up, u16 aim_down])` entries (parsed by `FUN_006677a0`, stored by `FUN_00665c00`,
//! which reorders the events to kind order `[action, aim_down, aim_up]`). At
//! runtime the table is an array of 60 (`0xf0 / 4`) resource indices indexed by slot, `-1` =
//! empty; slot `0x3c` (60) is the "none" value of the controller. `FUN_00665d50` plays
//! `table[slot]` through `FUN_006eda70`, `FUN_00665d20(slot, kind)` reads the per-slot event
//! copy. Behaviour code asks for a slot: `FUN_00665b80` starts every object in slot 14 (idle),
//! `FUN_00544460` aims with slot 27 (shoot), the `play_animation` action defaults to 14.
//!
//! Several objects share rigs, so a table may point at another creature's files
//! (`bipedanimation_*`, `genrun`, ...) or reuse one clip for several slots (a creature without a
//! `run` clip uses its `walk` for slot 24).
//!
//! # Names
//!
//! Slots 0-43 are named by the game itself: the level editor's option list for the
//! `play_animation` action's slot, `data\events\[region]\static_action_animationtype`
//! (resource 8865, next to the action's own text table `static_action_animation` = 8864 that
//! `FUN_0058d850` returns), lists `ATTACK`, `CELEBRATE`, ... `SCARED SWIM` in slot order; the
//! names below are that English text in snake case. The order is confirmed by the data: for 39
//! of those 44 slots the clip files the `.so` tables put there are named after the same action
//! (`<rig>_climbladder.anim` in slot 4 = `CLIMB LADDER`, ...). The other five hold, in the
//! shipped tables: 12 `GET UP` -> `*_useobjectthrow`, 15 `IDLE 2` -> `*_extendedidle*`,
//! 36 `POKE` -> `*_surprised`, 37 `SPECIAL 1` -> `*_sit`, `*_flashlight`, `*_firebreath`, ...,
//! 38 `SPECIAL 2` -> `*_handtruck`, `*_push`.
//!
//! Slots 44-59 have no game text; their names are the file-name suffix every `.so` table uses
//! for them (data only). Slots 48-50 are never used.

/// Name of each slot index (`None` = no text and no clip ever uses the slot).
pub const SLOT_NAMES: [Option<&str>; 60] = [
    Some("attack"),             // 0  ATTACK
    Some("celebrate"),          // 1  CELEBRATE (files also "victory")
    Some("cheer"),              // 2  CHEER
    Some("climb"),              // 3  CLIMB
    Some("climb_ladder"),       // 4  CLIMB LADDER
    Some("dance"),              // 5  DANCE
    Some("death"),              // 6  DEATH
    Some("dig"),                // 7  DIG
    Some("drive"),              // 8  DRIVE
    Some("eat"),                // 9  EAT
    Some("fiddle"),             // 10 FIDDLE
    Some("follow"),             // 11 FOLLOW
    Some("get_up"),             // 12 GET UP (files: "useobjectthrow")
    Some("hurt"),               // 13 HURT
    Some("idle"),               // 14 IDLE
    Some("idle_2"),             // 15 IDLE 2 (files: "extendedidle...")
    Some("jump"),               // 16 JUMP
    Some("jump_idle"),          // 17 JUMP IDLE
    Some("kick"),               // 18 KICK
    Some("land"),               // 19 LAND
    Some("laydown"),            // 20 LAYDOWN
    Some("melee"),              // 21 MELEE
    Some("pickup"),             // 22 PICKUP
    Some("ride"),               // 23 RIDE
    Some("run"),                // 24 RUN
    Some("scared"),             // 25 SCARED
    Some("scared_run"),         // 26 SCARED RUN
    Some("shoot"),              // 27 SHOOT
    Some("sleep"),              // 28 SLEEP
    Some("sticky_walk"),        // 29 STICKY WALK
    Some("swim"),               // 30 SWIM (files usually "_swi")
    Some("swim_idle"),          // 31 SWIM IDLE
    Some("throw"),              // 32 THROW
    Some("walk"),               // 33 WALK
    Some("fly"),                // 34 FLY
    Some("fly_idle"),           // 35 FLY IDLE
    Some("poke"),               // 36 POKE (files: "surprised")
    Some("special_1"),          // 37 SPECIAL 1 (files: "sit", "flashlight", "firebreath", ...)
    Some("special_2"),          // 38 SPECIAL 2 (files: "handtruck", "push")
    Some("swim_extended_idle"), // 39 SWIM EXTENDED IDLE (files: "extendedidleswi")
    Some("fly_extended_idle"),  // 40 FLY EXTENDED IDLE
    Some("sit"),                // 41 SIT
    Some("scared_fly"),         // 42 SCARED FLY (almost always filled with the "fly" clip)
    Some("scared_swim"),        // 43 SCARED SWIM (almost always filled with the "swi" clip)
    Some("swimairattack"),      // 44 (no text; file suffix)
    Some("swimairattack2"),     // 45 (same clip as 44 in every table)
    Some("usestarite"),         // 46
    Some("idlecine"),           // 47
    None,                       // 48
    None,                       // 49
    None,                       // 50
    Some("usespyglass"),        // 51
    Some("spyglassloop"),       // 52
    Some("pet"),                // 53
    Some("petdog"),             // 54
    Some("itch"),               // 55
    Some("shrug"),              // 56
    Some("whistle"),            // 57
    Some("plank"),              // 58
    Some("handtruck"),          // 59
];

/// Name of an animation slot.
pub fn slot_name(slot: u8) -> Option<&'static str> {
    SLOT_NAMES.get(slot as usize).copied().flatten()
}

/// Best-guess slot for an `.anim` file name (`"cow_walk.anim"`, `"maneatingplant_swi"`), from
/// the text after the last `_` (file suffixes are the slot names without `_`, plus the aliases
/// the shipped tables use). Useful for a viewer without the object's animation table.
pub fn slot_for_file(name: &str) -> Option<u8> {
    let stem = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let stem = stem.strip_suffix(".anim").unwrap_or(stem).to_ascii_lowercase();
    let suffix = stem.rsplit('_').next().unwrap_or(&stem);
    let slot = match suffix {
        "victory" => 1,
        "fidlde" => 10,
        "useobjectthrow" => 12,
        "extendedidle" => 15,
        "runscared" => 26,
        "swi" => 30,
        "surprised" => 36,
        "flashlight" => 37,
        "push" => 38,
        "extendedidleswim" | "extendedidleswi" | "extendedswimidle" => 39,
        "extendedidlefly" => 40,
        "scaredswi" => 43,
        s => return SLOT_NAMES.iter().position(|n| n.is_some_and(|n| n.replace('_', "") == s)).map(|i| i as u8),
    };
    Some(slot)
}
