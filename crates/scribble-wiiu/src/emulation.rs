//! Data stand-ins for the Wii U-only engine code the Nintendo items rely on.
//!
//! The Wii U build attaches per-frame helpers ("UpdateObject" types 7-9, created by object
//! init `0x23cbb40` from the object id; the PC factory `FUN_006c5d10` stops at type 6):
//!
//! * **Super star** (`C_UOSuperStar`, update `0x2325228`): `vx = ±1.0` every frame, starting
//!   rightwards; reversed on a wall contact; `vy = -5.5` on every floor contact (a bounce of
//!   constant height).
//! * **Fire flower fireball** (`C_UOFireFlowerBall`, `0x2324f58`): on a terrain contact while
//!   falling `vy *= -0.8`; killed by a wall contact. Object init also forces its collision class
//!   to 6 (`0x23dfc84`), the class PC gives *intangible* objects (`FUN_0069c710`), so it passes
//!   through objects it does not burn.
//!
//! PC data can get close with engine features it already has:
//!
//! * the `bouncy` property (0x43: physics flags `0x40100`): the contact solver (`FUN_005c2de0`)
//!   makes a bouncy body leave every contact at its incoming normal speed clamped to 4.0-7.0
//!   (`0x4000`..`0x7000`), so it keeps bouncing at a steady height — the star's -5.5 is inside
//!   that range;
//! * the `intangible` property (0x40: `+0x24e`, collision class 6), which does nothing else;
//! * `apply_force`, which adds `2.5 x force` to the velocity (`FUN_0053e440`, `force_y`
//!   quartered for non-character bodies), for the star's initial `vx = +1.0`.
//!
//! Each object gets a hidden adjective with those properties (new ids after the highest
//! adjective id) applied when it is created. What stays different: the star keeps no constant
//! horizontal speed (friction and walls act on it: a bouncy wall contact sends it back at 4.0 or
//! more instead of 1.0), the fireball keeps bouncing instead of losing 20% per bounce and
//! ricochets off walls instead of dying there (it still expires after its 120 frames), and it
//! is put out by water (PC `FUN_006a4540` kills fire-material objects entering liquid; Wii U
//! `0x23e405c` exempts the fireball).

use scribble_core::{bail, Context, Result, ResultExt as _};
use scribble_formats::{handler_for, Handler};
use serde_json::{json, Value};

pub const STAR: &str = "data\\_game\\scribbleobjects\\easteregg_nintendo_object_superstar.so";
pub const FIREBALL: &str = "data\\_game\\scribbleobjects\\weapon_projectile_magic__fireball.so";
/// The hidden adjectives this adds, in id order.
pub const ADJECTIVES: [&str; 2] =
    ["data\\_game\\scribbleadjectives\\_wiiuport_superstar.sa", "data\\_game\\scribbleadjectives\\_wiiuport_fireball.sa"];
/// An existing hidden gameplay/other adjective the new ones are modelled on.
pub const TEMPLATE: &str = "data\\_game\\scribbleadjectives\\_superstar.sa";

/// The adjective id stored in a `.sa` (u16 at byte 8).
pub fn adjective_id(sa: &[u8]) -> Option<u16> {
    sa.get(8..10).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn set_bool(property: &str) -> Value {
    json!({"type": "set_property", "property": property, "value_type": "bool", "operator": "set", "value": 1})
}

fn codec(path: &str, data: &[u8]) -> Result<&'static dyn scribble_core::Codec> {
    match handler_for(path, data) {
        Some(Handler::Codec(c)) => Ok(c),
        _ => bail!("no codec for {path}"),
    }
}

/// The two adjectives, built from `template` (the `_superstar.sa` bytes for PC) with ids
/// `first_id` and `first_id + 1`.
pub fn adjectives(template: &[u8], first_id: u16, ctx: &Context) -> Result<Vec<Vec<u8>>> {
    let c = codec(TEMPLATE, template)?;
    let base = c.decode_json(template, ctx)?;
    let prefix = base["id"].as_str().and_then(|s| s.rsplit_once('/')).map(|(p, _)| p.to_string()).context("template adjective id")?;
    let modifiers = [vec![set_bool("bouncy")], vec![set_bool("bouncy"), set_bool("intangible")]];
    let mut out = Vec::new();
    for (i, m) in modifiers.into_iter().enumerate() {
        let mut v = base.clone();
        v["id"] = json!(format!("{prefix}/{}", first_id + i as u16));
        v["flags"] = json!(["hidden"]);
        v["budget_cost"] = json!(0);
        v["effects"] = json!([{"modifiers": m}]);
        out.push(c.encode_json(v, ctx)?);
    }
    Ok(out)
}

/// Add the creation behaviour to the star or the fireball (`path`; bytes for PC).
pub fn patch_object(path: &str, data: &[u8], ctx: &Context) -> Result<Vec<u8>> {
    let c = codec(path, data)?;
    let mut v = c.decode_json(data, ctx)?;
    let apply = |adjective: &str| {
        json!({"type": "apply_adjectives", "target": "self", "flags": ["unused_bit0", "hide_names", "unused_bit3"], "adjectives": [adjective]})
    };
    let actions = match path {
        // 0x666 / 0x1000 (0.4 is not exact in 20.12 fixed point) x 2.5 = the Wii U star's
        // initial vx of +1.0.
        STAR => json!([apply(ADJECTIVES[0]), {"type": "apply_force", "target": "self", "force_x": 0.39990234375, "force_y": 0, "spin": 0}]),
        FIREBALL => json!([apply(ADJECTIVES[1])]),
        _ => bail!("no emulation for {path}"),
    };
    let behaviours = v["body"]["behaviours"].as_array_mut().with_context(|| format!("{path} has no behaviours"))?;
    behaviours.insert(0, json!({"type": "on_created", "once": true, "actions": actions}));
    c.encode_json(v, ctx)
}
