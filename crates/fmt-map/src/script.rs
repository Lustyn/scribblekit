//! Scene scripts: the triggers (with their actions) attached to placed objects and object
//! groups.
//!
//! These are the very same engine classes as the behaviours and actions of scribble objects —
//! triggers are built by `FUN_006d36e0(type)`, actions by `FUN_0064e1f0(type)` — so they are
//! decoded with the shared schema of [`fmt_object::behaviour`] ([`BEHAVIOURS`], [`ACTIONS`]):
//! the same trigger or action has the same name and field names in `.so`, `.sod` and event
//! scripts. See that module for the JSON shape (`{"type": ..., ...fields, "actions": [...]}`).
//!
//! Scene scripts address other things in the scene through *entities*: `{"object": n}` is the
//! n-th placed object, `{"group": n}` the n-th object group (see
//! [`fmt_object::record::entity_to_json`]).
//!
//! [`BEHAVIOURS`]: fmt_object::behaviour::BEHAVIOURS
//! [`ACTIONS`]: fmt_object::behaviour::ACTIONS

use fmt_object::Behaviour;
use scribble_core::{ns, Context, NamedId, Reader, Result, Writer};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// A trigger of a placed object or object group, with the actions it runs.
///
/// ```text
/// [scene has merit rules] u16 merit     (0xFFFF = none)
/// Behaviour                              type byte, fields, u8 n, n x action
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct SceneTrigger {
    /// The merit (from the level's `.mdb`) this trigger belongs to; stored only when the scene
    /// has merit rules. `FUN_004bdab0` looks the id up among the scene's merit rules
    /// (`FUN_0071eee0`, rule list at +0x1fc, matched on the rule's u16 id) and adds the trigger
    /// and its action chain to that rule's progress nodes (`FUN_006d6960`, `FUN_00462060`,
    /// `FUN_00462150`). JSON key `for_merit`, right after `type`.
    pub merit: Option<NamedId>,
    pub behaviour: Behaviour,
}

impl SceneTrigger {
    pub fn read(r: &mut Reader, ctx: &Context, merit_ids: bool) -> Result<Self> {
        let merit = if merit_ids { Some(r.u16()?).filter(|&m| m != u16::MAX).map(|m| NamedId::from_id(m as u32, ns::MERIT, None, ctx)) } else { None };
        Ok(SceneTrigger { merit, behaviour: Behaviour::read(r, ctx)? })
    }

    pub fn write(&self, w: &mut Writer, ctx: &Context, merit_ids: bool) -> Result<()> {
        match (&self.merit, merit_ids) {
            (Some(m), true) => {
                let id = m.to_id(ns::MERIT, None, ctx)?;
                w.u16(u16::try_from(id).map_err(|_| scribble_core::anyhow!("merit {id} does not fit in 16 bits"))?);
            }
            (None, true) => {
                w.u16(u16::MAX);
            }
            (None, false) => {}
            (Some(_), false) => scribble_core::bail!("trigger `for_merit` needs the scene's merit rules (`merits`)"),
        }
        self.behaviour.write(w, ctx)
    }
}

/// `u8 count; count x SceneTrigger`.
pub fn read_triggers(r: &mut Reader, ctx: &Context, merit_ids: bool) -> Result<Vec<SceneTrigger>> {
    let n = r.u8()?;
    (0..n).map(|_| SceneTrigger::read(r, ctx, merit_ids)).collect()
}

pub fn write_triggers(w: &mut Writer, triggers: &[SceneTrigger], ctx: &Context, merit_ids: bool) -> Result<()> {
    w.u8(u8::try_from(triggers.len())?);
    for t in triggers {
        t.write(w, ctx, merit_ids)?;
    }
    Ok(())
}

impl Serialize for SceneTrigger {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let Value::Object(fields) = serde_json::to_value(&self.behaviour).map_err(serde::ser::Error::custom)? else {
            return Err(serde::ser::Error::custom("behaviour is not an object"));
        };
        let mut m = Map::new();
        let mut merit = self.merit.as_ref().map(|m| serde_json::to_value(m).unwrap());
        for (k, v) in fields {
            if merit.is_some() && k != "type" && k != "once" {
                m.insert("for_merit".into(), merit.take().unwrap());
            }
            m.insert(k, v);
        }
        if let Some(v) = merit {
            m.insert("for_merit".into(), v);
        }
        m.serialize(s)
    }
}

impl<'de> Deserialize<'de> for SceneTrigger {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut m = Map::deserialize(d)?;
        let merit = match m.remove("for_merit").filter(|v| !v.is_null()) {
            Some(v) => Some(serde_json::from_value(v).map_err(serde::de::Error::custom)?),
            None => None,
        };
        let behaviour = serde_json::from_value(Value::Object(m)).map_err(serde::de::Error::custom)?;
        Ok(SceneTrigger { merit, behaviour })
    }
}
