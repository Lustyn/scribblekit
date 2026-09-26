//! A small schema interpreter for the many little polymorphic records in objects and
//! adjectives: behaviours (triggers), actions and adjective modifiers.
//!
//! The engine instantiates one C++ class per record kind (via a factory switch) and lets the
//! class's virtual `parse` method read its fields from the byte stream. There are well over a
//! hundred such classes, most of them reading a handful of bytes, so instead of one Rust struct
//! per class every kind has a static [`Field`] list transcribed from its parse method, and a
//! decoded record is an ordered map of named JSON values. The same schema drives decoding and
//! encoding, so records stay lossless while remaining editable by name.

use crate::refs::{AdjectivePath, Attitude, Filter, ObjectPath, ObjectRef, RefList};
use crate::util::{from_opt16, opt16};
use scribble_core::{anyhow, bail, ensure, ns, Context, Fx12, Fx16, Hex, NamedId, Reader, ResRef, Result, Writer};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// One element of a record schema.
#[derive(Clone, Copy, Debug)]
pub enum Field {
    U8(&'static str),
    I8(&'static str),
    U16(&'static str),
    I16(&'static str),
    U32(&'static str),
    I32(&'static str),
    /// u32 where `0xFFFFFFFF` means "none" (`null`).
    OptU32(&'static str),
    /// i32 16.16 fixed point.
    Fx(&'static str),
    /// i32 20.12 fixed point (the engine's world scale, `0x1000` = 1.0).
    Fx12(&'static str),
    /// A byte the engine reads past without using; omitted from the JSON when 0.
    Unused8(&'static str),
    /// u8 omitted from the JSON when 0.
    U8Z(&'static str),
    /// A byte that always has this value (it selects the record's layout); not in the JSON.
    Const(u8),
    /// A u8 split into bit fields, each printed as its own key (see [`Part`]). The masks must
    /// cover all 8 bits.
    Split(&'static [Part]),
    /// u16 pmindex resource index where 0 means none (`null`).
    Res16Z(&'static str),
    /// u8 trigger (behaviour) type id, printed as the trigger's name.
    TriggerType(&'static str),
    /// u8 action type id, printed as the action's name.
    ActionType(&'static str),
    /// A byte the engine skips that always holds the given value in shipped files; omitted
    /// from the JSON when it has that value.
    Skip8(&'static str, u8),
    /// u32 reference to a scene entity (see [`entity_to_json`]); `null` = none.
    Entity(&'static str),
    /// Action target (`FUN_0064f630`): u8 kind (see [`TARGETS`]); kind 4 is followed by a u32
    /// scene entity. Printed as the kind's name or `{"entity": ...}`.
    Target(&'static str),
    /// u16 merit id (named from the merit databases), `0xFFFF` = `null`.
    Merit(&'static str),
    /// u8 that must be 0 or 1.
    Bool(&'static str),
    /// u32 pmindex resource index (`0xFFFFFFFF` = `null`).
    Res32(&'static str),
    /// u16 pmindex resource index (`0xFFFF` = `null`).
    Res16(&'static str),
    /// u16 id, `0xFFFF` = `null`.
    Id16(&'static str),
    /// u8 bit flags printed as names (`bit_N` for unnamed bits).
    Flags(&'static str, &'static [&'static str]),
    /// u8 enumeration printed as a name when known.
    Enum(&'static str, &'static [(u8, &'static str)]),
    /// u8 animation slot, printed with its animation name (`fmt_anim::slots`).
    AnimSlot(&'static str),
    /// NUL-terminated Latin-1 string.
    CStr(&'static str),
    /// Latin-1 string with a u8 length prefix.
    Str8(&'static str),
    /// Fixed-size opaque bytes.
    Bytes(&'static str, usize),
    /// Fields present only when the condition holds.
    If(Cond, &'static [Field]),
    /// Fields depending on a condition.
    IfElse(Cond, &'static [Field], &'static [Field]),
    /// u8 count, then that many items, each an object with the given fields.
    List(&'static str, &'static [Field]),
    /// u8 count, then that many scalars of the given single-field type (name ignored).
    ListOf(&'static str, &'static Field),
    /// Items whose count is the length of an earlier list (by name); no count byte.
    ListLike(&'static str, &'static str, &'static Field),
    /// 4 x u16 object id path.
    ObjectPath(&'static str),
    /// 3 x u16 adjective id path.
    AdjectivePath(&'static str),
    /// Object + adjective reference (7 x u16).
    ObjectRef(&'static str),
    /// [`RefList`].
    RefList(&'static str),
    /// [`Filter`].
    Filter(&'static str),
    /// u8 count + count x (u16 `.sa` resource, u16 display word) (`FUN_0053d8d0`); see
    /// [`read_adjectives`].
    Adjectives(&'static str),
    /// u8 count + count x [`Attitude`] (`FUN_0043f190`).
    Attitudes(&'static str),
    /// A nested behaviour entry (type byte, fields, action list).
    Behaviour(&'static str),
    /// u8 count + behaviour entries.
    Behaviours(&'static str),
    /// u8 count + actions (type byte + fields each).
    Actions(&'static str),
    /// u8 count + action chains: u8 length, then that many actions.
    ActionChains(&'static str),
    /// Include another field list inline.
    Inline(&'static [Field]),
}

/// One bit field of a [`Field::Split`] byte.
#[derive(Clone, Copy, Debug)]
pub enum Part {
    /// `(byte & mask) >> shift` as a number.
    Num(&'static str, u8),
    /// A single bit as a bool (omitted when false).
    Bool(&'static str, u8),
    /// `(byte & mask) >> shift` printed through a name table.
    Enum(&'static str, u8, &'static [(u8, &'static str)]),
    /// `(byte & mask) >> shift` as a number, omitted when 0 (bits with no known use, or
    /// counts where 0 means "none").
    Rest(&'static str, u8),
}

impl Part {
    fn name(&self) -> &'static str {
        match *self {
            Part::Num(n, _) | Part::Bool(n, _) | Part::Enum(n, _, _) | Part::Rest(n, _) => n,
        }
    }
    fn mask(&self) -> u8 {
        match *self {
            Part::Num(_, m) | Part::Bool(_, m) | Part::Enum(_, m, _) | Part::Rest(_, m) => m,
        }
    }
}

fn enum_value(names: &[(u8, &str)], v: u8) -> Value {
    match names.iter().find(|(k, _)| *k == v) {
        Some((_, n)) => Value::String(n.to_string()),
        None => v.into(),
    }
}

fn enum_parse(names: &[(u8, &str)], v: &Value, field: &str) -> Result<u8> {
    match v {
        Value::String(s) => names.iter().find(|(_, k)| k == s).map(|(k, _)| *k).ok_or_else(|| anyhow!("field {field:?}: unknown value {s:?}")),
        _ => v.as_u64().and_then(|x| u8::try_from(x).ok()).ok_or_else(|| anyhow!("field {field:?}: expected a name or a small number, got {v}")),
    }
}

/// Condition on a previously read numeric field of the same record.
#[derive(Clone, Copy, Debug)]
pub enum Cond {
    /// `(field & mask) != 0`
    Bit(&'static str, u32),
    /// `(field & mask) == 0`
    NoBit(&'static str, u32),
    /// `field == value`
    Eq(&'static str, u32),
    /// `(field & mask) == value`
    MaskEq(&'static str, u32, u32),
    /// `field != 0`
    NonZero(&'static str),
}

/// Numeric values of already-processed fields, for conditions.
#[derive(Default)]
struct Scope {
    vars: HashMap<&'static str, u64>,
}

impl Scope {
    fn get(&self, name: &str) -> Result<u64> {
        self.vars.get(name).copied().ok_or_else(|| anyhow!("schema condition refers to unknown field {name:?}"))
    }
    fn eval(&self, c: &Cond) -> Result<bool> {
        Ok(match *c {
            Cond::Bit(n, m) => self.get(n)? & m as u64 != 0,
            Cond::NoBit(n, m) => self.get(n)? & m as u64 == 0,
            Cond::Eq(n, v) => self.get(n)? == v as u64,
            Cond::MaskEq(n, m, v) => self.get(n)? & m as u64 == v as u64,
            Cond::NonZero(n) => self.get(n)? != 0,
        })
    }
}

/// Hooks for the nested polymorphic records (implemented by `behaviour.rs`).
pub trait Nested {
    fn read_behaviour(r: &mut Reader, ctx: &Context) -> Result<Value>;
    fn write_behaviour(v: &Value, w: &mut Writer, ctx: &Context) -> Result<()>;
    fn read_action(r: &mut Reader, ctx: &Context) -> Result<Value>;
    fn write_action(v: &Value, w: &mut Writer, ctx: &Context) -> Result<()>;
}

fn json<T: serde::Serialize>(v: &T) -> Result<Value> {
    Ok(serde_json::to_value(v)?)
}

fn from_json<T: serde::de::DeserializeOwned>(v: &Value, what: &str) -> Result<T> {
    serde_json::from_value(v.clone()).map_err(|e| anyhow!("{what}: {e}"))
}

/// Decode a record's fields according to `schema`.
pub fn read_fields<N: Nested>(schema: &'static [Field], r: &mut Reader, ctx: &Context) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    let mut scope = Scope::default();
    read_into::<N>(schema, r, ctx, &mut out, &mut scope)?;
    Ok(out)
}

/// Encode a record's fields according to `schema`.
pub fn write_fields<N: Nested>(schema: &'static [Field], m: &Map<String, Value>, w: &mut Writer, ctx: &Context) -> Result<()> {
    let mut scope = Scope::default();
    write_from::<N>(schema, m, w, ctx, &mut scope)
}

fn read_scalar(f: &Field, r: &mut Reader, ctx: &Context) -> Result<(Value, u64)> {
    Ok(match *f {
        Field::U8(_) => {
            let v = r.u8()?;
            (v.into(), v as u64)
        }
        Field::I8(_) => {
            let v = r.i8()?;
            (v.into(), v as i64 as u64)
        }
        Field::U16(_) => {
            let v = r.u16()?;
            (v.into(), v as u64)
        }
        Field::I16(_) => {
            let v = r.i16()?;
            (v.into(), v as i64 as u64)
        }
        Field::U32(_) => {
            let v = r.u32()?;
            (v.into(), v as u64)
        }
        Field::I32(_) => {
            let v = r.i32()?;
            (v.into(), v as i64 as u64)
        }
        Field::OptU32(_) => {
            let v = r.u32()?;
            (if v == u32::MAX { Value::Null } else { v.into() }, v as u64)
        }
        Field::Fx(_) => {
            let v = r.i32()?;
            (json(&Fx16(v))?, v as i64 as u64)
        }
        Field::Fx12(_) => {
            let v = r.i32()?;
            (json(&Fx12(v))?, v as i64 as u64)
        }
        Field::Unused8(_) | Field::U8Z(_) => {
            let v = r.u8()?;
            (v.into(), v as u64)
        }
        Field::Entity(_) => {
            let v = r.u32()?;
            (entity_to_json(v), v as u64)
        }
        Field::Target(_) => {
            let k = r.u8()?;
            let v = match k {
                4 => {
                    let mut m = Map::new();
                    m.insert("entity".into(), entity_to_json(r.u32()?));
                    Value::Object(m)
                }
                _ => match TARGETS.iter().find(|(i, _)| *i == k) {
                    Some((_, n)) => Value::String(n.to_string()),
                    None => k.into(),
                },
            };
            (v, k as u64)
        }
        Field::Merit(_) => {
            let v = r.u16()?;
            (named16(v, ns::MERIT, ctx), v as u64)
        }
        Field::Res16Z(_) => {
            let v = r.u16()?;
            let j = if v == 0 { Value::Null } else { json(&ResRef::from_index(v as u32, ctx))? };
            (j, v as u64)
        }
        Field::Skip8(_, _) => {
            let v = r.u8()?;
            (v.into(), v as u64)
        }
        Field::ActionType(_) => {
            let v = r.u8()?;
            let j = match crate::behaviour::ACTIONS.iter().find(|d| d.id == v) {
                Some(d) => Value::String(d.name.into()),
                None => v.into(),
            };
            (j, v as u64)
        }
        Field::TriggerType(_) => {
            let v = r.u8()?;
            let j = match crate::behaviour::BEHAVIOURS.iter().find(|d| d.id == v) {
                Some(d) => Value::String(d.name.into()),
                None => v.into(),
            };
            (j, v as u64)
        }
        Field::Bool(_) => {
            let v = r.bool()?;
            (v.into(), v as u64)
        }
        Field::Res32(_) => {
            let v = r.u32()?;
            (json(&crate::util::res32(v, ctx))?, v as u64)
        }
        Field::Res16(_) => {
            let v = r.u16()?;
            (json(&crate::util::res16(v, ctx))?, v as u64)
        }
        Field::Id16(_) => {
            let v = r.u16()?;
            (json(&opt16(v))?, v as u64)
        }
        Field::Flags(_, names) => {
            let v = r.u8()?;
            (json(&crate::util::flag_names(v as u32, 8, names))?, v as u64)
        }
        Field::Enum(_, names) => {
            let v = r.u8()?;
            let j = match names.iter().find(|(k, _)| *k == v) {
                Some((_, n)) => Value::String(n.to_string()),
                None => v.into(),
            };
            (j, v as u64)
        }
        Field::AnimSlot(_) => {
            let v = r.u8()?;
            (serde_json::to_value(crate::so::AnimSlot(v))?, v as u64)
        }
        Field::CStr(_) => {
            let s = r.cstr()?;
            let n = s.len() as u64;
            (s.into(), n)
        }
        Field::Str8(_) => {
            let s = r.str_u8()?;
            let n = s.len() as u64;
            (s.into(), n)
        }
        Field::Bytes(_, n) => (json(&Hex(r.bytes(n)?.to_vec()))?, 0),
        Field::ObjectPath(_) => (json(&ObjectPath::read(r, ctx)?)?, 0),
        Field::AdjectivePath(_) => (json(&AdjectivePath::read(r, ctx)?)?, 0),
        Field::ObjectRef(_) => (json(&ObjectRef::read(r, ctx)?)?, 0),
        _ => bail!("field {f:?} is not a scalar"),
    })
}

fn write_scalar(f: &Field, v: &Value, w: &mut Writer, ctx: &Context) -> Result<u64> {
    fn int(v: &Value, name: &str) -> Result<i64> {
        v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)).ok_or_else(|| anyhow!("field {name:?}: expected an integer, got {v}"))
    }
    fn range<T: TryFrom<i64>>(v: i64, name: &str) -> Result<T> {
        T::try_from(v).map_err(|_| anyhow!("field {name:?}: value {v} out of range"))
    }
    Ok(match *f {
        Field::U8(n) => {
            let x: u8 = range(int(v, n)?, n)?;
            w.u8(x);
            x as u64
        }
        Field::I8(n) => {
            let x: i8 = range(int(v, n)?, n)?;
            w.i8(x);
            x as i64 as u64
        }
        Field::U16(n) => {
            let x: u16 = range(int(v, n)?, n)?;
            w.u16(x);
            x as u64
        }
        Field::I16(n) => {
            let x: i16 = range(int(v, n)?, n)?;
            w.i16(x);
            x as i64 as u64
        }
        Field::U32(n) => {
            let x: u32 = range(int(v, n)?, n)?;
            w.u32(x);
            x as u64
        }
        Field::I32(n) => {
            let x: i32 = range(int(v, n)?, n)?;
            w.i32(x);
            x as i64 as u64
        }
        Field::OptU32(n) => {
            let x: u32 = if v.is_null() { u32::MAX } else { range(int(v, n)?, n)? };
            w.u32(x);
            x as u64
        }
        Field::Fx(n) => {
            let x: Fx16 = from_json(v, n)?;
            w.i32(x.0);
            x.0 as i64 as u64
        }
        Field::Fx12(n) => {
            let x: Fx12 = from_json(v, n)?;
            w.i32(x.0);
            x.0 as i64 as u64
        }
        Field::Unused8(n) | Field::U8Z(n) => {
            let x: u8 = range(int(v, n)?, n)?;
            w.u8(x);
            x as u64
        }
        Field::Entity(n) => {
            let x = json_to_entity(v).map_err(|e| anyhow!("field {n:?}: {e}"))?;
            w.u32(x);
            x as u64
        }
        Field::Target(n) => {
            let k = match v {
                Value::String(s) => TARGETS.iter().find(|(_, t)| t == s).map(|(i, _)| *i).ok_or_else(|| anyhow!("field {n:?}: unknown target {s:?}"))?,
                Value::Object(m) if m.len() == 1 && m.contains_key("entity") => 4,
                _ => {
                    let k: u8 = range(int(v, n)?, n)?;
                    ensure!(k != 4 && !TARGETS.iter().any(|(i, _)| *i == k), "field {n:?}: target {k} must be written by name");
                    k
                }
            };
            w.u8(k);
            if k == 4 {
                w.u32(json_to_entity(&v["entity"]).map_err(|e| anyhow!("field {n:?}: {e}"))?);
            }
            k as u64
        }
        Field::Merit(n) => {
            let x = from_named16(v, ns::MERIT, ctx).map_err(|e| anyhow!("field {n:?}: {e}"))?;
            w.u16(x);
            x as u64
        }
        Field::Res16Z(n) => {
            let x = match from_json::<Option<ResRef>>(v, n)? {
                None => 0,
                Some(r) => {
                    let i = r.to_index(ctx)?;
                    ensure!(i != 0, "field {n:?}: resource 0 is written as null");
                    u16::try_from(i).map_err(|_| anyhow!("field {n:?}: resource index {i} does not fit in 16 bits"))?
                }
            };
            w.u16(x);
            x as u64
        }
        Field::Skip8(n, _) => {
            let x: u8 = range(int(v, n)?, n)?;
            w.u8(x);
            x as u64
        }
        Field::ActionType(n) => {
            let x = match v {
                Value::String(s) => crate::behaviour::ACTIONS.iter().find(|d| d.name == s).map(|d| d.id).ok_or_else(|| anyhow!("field {n:?}: unknown action type {s:?}"))?,
                _ => range(int(v, n)?, n)?,
            };
            w.u8(x);
            x as u64
        }
        Field::TriggerType(n) => {
            let x = match v {
                Value::String(s) => crate::behaviour::BEHAVIOURS.iter().find(|d| d.name == s).map(|d| d.id).ok_or_else(|| anyhow!("field {n:?}: unknown trigger type {s:?}"))?,
                _ => range(int(v, n)?, n)?,
            };
            w.u8(x);
            x as u64
        }
        Field::Bool(n) => {
            let b = v.as_bool().ok_or_else(|| anyhow!("field {n:?}: expected a bool"))?;
            w.bool(b);
            b as u64
        }
        Field::Res32(n) => {
            let x: Option<ResRef> = from_json(v, n)?;
            let i = crate::util::from_res32(&x, ctx)?;
            w.u32(i);
            i as u64
        }
        Field::Res16(n) => {
            let x: Option<ResRef> = from_json(v, n)?;
            let i = crate::util::from_res16(&x, ctx)?;
            w.u16(i);
            i as u64
        }
        Field::Id16(n) => {
            let x: Option<u16> = from_json(v, n)?;
            let i = from_opt16(x);
            w.u16(i);
            i as u64
        }
        Field::Flags(n, names) => {
            let list: Vec<String> = from_json(v, n)?;
            let b = crate::util::flag_bits(&list, 8, names)?;
            w.u8(b as u8);
            b as u64
        }
        Field::Enum(n, names) => {
            let b = match v {
                Value::String(s) => names.iter().find(|(_, k)| k == s).map(|(k, _)| *k).ok_or_else(|| anyhow!("field {n:?}: unknown value {s:?}"))?,
                _ => range(int(v, n)?, n)?,
            };
            w.u8(b);
            b as u64
        }
        Field::AnimSlot(n) => {
            let s: crate::so::AnimSlot = from_json(v, n)?;
            w.u8(s.0);
            s.0 as u64
        }
        Field::CStr(n) => {
            let s = v.as_str().ok_or_else(|| anyhow!("field {n:?}: expected a string"))?;
            w.cstr(s)?;
            s.len() as u64
        }
        Field::Str8(n) => {
            let s = v.as_str().ok_or_else(|| anyhow!("field {n:?}: expected a string"))?;
            w.str_u8(s)?;
            s.len() as u64
        }
        Field::Bytes(n, len) => {
            let h: Hex = from_json(v, n)?;
            ensure!(h.0.len() == len, "field {n:?}: expected {len} bytes, got {}", h.0.len());
            w.bytes(&h.0);
            0
        }
        Field::ObjectPath(n) => {
            from_json::<ObjectPath>(v, n)?.write(w, ctx).map_err(|e| anyhow!("field {n:?}: {e}"))?;
            0
        }
        Field::AdjectivePath(n) => {
            from_json::<AdjectivePath>(v, n)?.write(w, ctx).map_err(|e| anyhow!("field {n:?}: {e}"))?;
            0
        }
        Field::ObjectRef(n) => {
            from_json::<ObjectRef>(v, n)?.write(w, ctx).map_err(|e| anyhow!("field {n:?}: {e}"))?;
            0
        }
        _ => bail!("field {f:?} is not a scalar"),
    })
}

/// The JSON key of a named field (`None` for structural fields).
pub fn field_name_of(f: &Field) -> Option<&'static str> {
    field_name(f)
}

fn field_name(f: &Field) -> Option<&'static str> {
    use Field::*;
    match *f {
        U8(n) | I8(n) | U16(n) | I16(n) | U32(n) | I32(n) | OptU32(n) | Fx(n) | Fx12(n) | Unused8(n) | U8Z(n) | Entity(n) | Target(n) | Merit(n)
        | Bool(n) | Res32(n) | Res16(n) | Id16(n)
        | Flags(n, _) | Enum(n, _) | AnimSlot(n) | CStr(n) | Str8(n) | Bytes(n, _) | List(n, _) | ListOf(n, _) | ListLike(n, _, _)
        | ObjectPath(n) | AdjectivePath(n) | ObjectRef(n) | RefList(n) | Filter(n) | Adjectives(n) | Attitudes(n) | Behaviour(n)
        | Behaviours(n) | Actions(n) | ActionChains(n) => Some(n),
        Res16Z(n) | TriggerType(n) | ActionType(n) | Skip8(n, _) => Some(n),
        If(..) | IfElse(..) | Inline(_) | Const(_) | Split(_) => None,
    }
}

fn len_key(name: &'static str) -> &'static str {
    // Leak one small string per list name so it can live in the scope map; names are static
    // schema constants, so this is bounded.
    use std::sync::Mutex;
    static CACHE: Mutex<Vec<(&'static str, &'static str)>> = Mutex::new(Vec::new());
    let mut c = CACHE.lock().unwrap();
    if let Some((_, k)) = c.iter().find(|(n, _)| *n == name) {
        return k;
    }
    let k: &'static str = Box::leak(format!("{name}#len").into_boxed_str());
    c.push((name, k));
    k
}

/// Values that are left out of the JSON and restored on encode: `null`, `false`, `[]`, `{}`
/// (and 0 for [`Field::Unused8`]).
fn is_default_for(f: &Field, v: &Value) -> bool {
    match f {
        Field::Unused8(_) | Field::U8Z(_) => v.as_u64() == Some(0),
        Field::Skip8(_, d) => v.as_u64() == Some(*d as u64),
        _ => is_default(v),
    }
}

fn is_default(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// The value a field has when it is missing from the JSON (only for fields whose default is
/// omitted by [`is_default`]).
fn default_value(f: &Field) -> Option<Value> {
    use Field::*;
    Some(match f {
        OptU32(_) | Res32(_) | Res16(_) | Res16Z(_) | Id16(_) | Entity(_) | Merit(_) => Value::Null,
        Unused8(_) | U8Z(_) => Value::from(0),
        Skip8(_, d) => Value::from(*d),
        Bool(_) => Value::Bool(false),
        Flags(..) | List(..) | ListOf(..) | ListLike(..) | Adjectives(_) | Attitudes(_) | Behaviours(_) | Actions(_) | ActionChains(_) => {
            Value::Array(vec![])
        }
        RefList(_) | Filter(_) => Value::Object(Map::new()),
        _ => return None,
    })
}

fn put(out: &mut Map<String, Value>, name: &str, v: Value) {
    if !is_default(&v) {
        out.insert(name.to_string(), v);
    }
}

fn read_into<N: Nested>(schema: &'static [Field], r: &mut Reader, ctx: &Context, out: &mut Map<String, Value>, scope: &mut Scope) -> Result<()> {
    for f in schema {
        match f {
            Field::If(c, fs) => {
                if scope.eval(c)? {
                    read_into::<N>(fs, r, ctx, out, scope)?;
                }
            }
            Field::IfElse(c, a, b) => {
                let fs = if scope.eval(c)? { a } else { b };
                read_into::<N>(fs, r, ctx, out, scope)?;
            }
            Field::Inline(fs) => read_into::<N>(fs, r, ctx, out, scope)?,
            Field::Split(parts) => {
                let b = r.u8()?;
                for p in parts.iter() {
                    let v = (b & p.mask()) >> p.mask().trailing_zeros();
                    scope.vars.insert(p.name(), v as u64);
                    let j = match *p {
                        Part::Num(..) => Some(Value::from(v)),
                        Part::Bool(..) => (v != 0).then_some(Value::Bool(true)),
                        Part::Enum(_, _, names) => Some(enum_value(names, v)),
                        Part::Rest(..) => (v != 0).then(|| Value::from(v)),
                    };
                    if let Some(j) = j {
                        out.insert(p.name().to_string(), j);
                    }
                }
            }
            Field::Const(v) => {
                let at = r.pos();
                let got = r.u8()?;
                ensure!(got == *v, "expected byte {v:#04x} at {at:#x}, found {got:#04x}");
            }
            Field::List(n, fs) => {
                let count = r.u8()?;
                let mut items = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let mut item = Map::new();
                    let mut s = Scope::default();
                    read_into::<N>(fs, r, ctx, &mut item, &mut s)?;
                    items.push(Value::Object(item));
                }
                scope.vars.insert(len_key(n), count as u64);
                put(out, n, Value::Array(items));
            }
            Field::ListOf(n, item) => {
                let count = r.u8()?;
                let items = (0..count).map(|_| read_scalar(item, r, ctx).map(|x| x.0)).collect::<Result<Vec<_>>>()?;
                scope.vars.insert(len_key(n), count as u64);
                put(out, n, Value::Array(items));
            }
            Field::ListLike(n, other, item) => {
                let count = scope.get(len_key(other))?;
                let items = (0..count).map(|_| read_scalar(item, r, ctx).map(|x| x.0)).collect::<Result<Vec<_>>>()?;
                put(out, n, Value::Array(items));
            }
            Field::RefList(n) => {
                put(out, n, json(&RefList::read(r, ctx)?)?);
            }
            Field::Filter(n) => {
                put(out, n, json(&Filter::read(r, ctx)?)?);
            }
            Field::Adjectives(n) => {
                let count = r.u8()?;
                let items = (0..count).map(|_| read_adjective(r, ctx)).collect::<Result<Vec<_>>>()?;
                scope.vars.insert(len_key(n), count as u64);
                put(out, n, Value::Array(items));
            }
            Field::Attitudes(n) => {
                let count = r.u8()?;
                let items = (0..count).map(|_| json(&Attitude::read(r, ctx)?)).collect::<Result<Vec<_>>>()?;
                put(out, n, Value::Array(items));
            }
            Field::Behaviour(n) => {
                put(out, n, N::read_behaviour(r, ctx)?);
            }
            Field::Behaviours(n) => {
                let count = r.u8()?;
                let items = (0..count).map(|_| N::read_behaviour(r, ctx)).collect::<Result<Vec<_>>>()?;
                put(out, n, Value::Array(items));
            }
            Field::Actions(n) => {
                let count = r.u8()?;
                let items = (0..count).map(|_| N::read_action(r, ctx)).collect::<Result<Vec<_>>>()?;
                put(out, n, Value::Array(items));
            }
            Field::ActionChains(n) => {
                let count = r.u8()?;
                let mut chains = Vec::new();
                for _ in 0..count {
                    let len = r.u8()?;
                    chains.push(Value::Array((0..len).map(|_| N::read_action(r, ctx)).collect::<Result<Vec<_>>>()?));
                }
                put(out, n, Value::Array(chains));
            }
            scalar => {
                let (v, num) = read_scalar(scalar, r, ctx)?;
                let name = field_name(scalar).unwrap();
                scope.vars.insert(name, num);
                if !is_default_for(scalar, &v) {
                    out.insert(name.to_string(), v);
                }
            }
        }
    }
    Ok(())
}

static EMPTY_ARRAY: Value = Value::Array(Vec::new());

fn get<'a>(m: &'a Map<String, Value>, name: &str) -> Result<&'a Value> {
    m.get(name).ok_or_else(|| anyhow!("missing field {name:?}"))
}

/// A field's value, or its default when it was left out.
fn get_or_default(m: &Map<String, Value>, f: &Field) -> Result<Value> {
    let name = field_name(f).unwrap();
    match m.get(name) {
        Some(v) => Ok(v.clone()),
        None => default_value(f).ok_or_else(|| anyhow!("missing field {name:?}")),
    }
}

fn array<'a>(m: &'a Map<String, Value>, name: &str) -> Result<&'a Vec<Value>> {
    match m.get(name) {
        None => Ok(EMPTY_ARRAY.as_array().unwrap()),
        Some(v) => v.as_array().ok_or_else(|| anyhow!("field {name:?}: expected an array")),
    }
}

fn count(w: &mut Writer, n: usize, name: &str) -> Result<()> {
    crate::util::count8(w, n, name)
}

fn write_from<N: Nested>(schema: &'static [Field], m: &Map<String, Value>, w: &mut Writer, ctx: &Context, scope: &mut Scope) -> Result<()> {
    for f in schema {
        match f {
            Field::If(c, fs) => {
                if scope.eval(c)? {
                    write_from::<N>(fs, m, w, ctx, scope)?;
                }
            }
            Field::IfElse(c, a, b) => {
                let fs = if scope.eval(c)? { a } else { b };
                write_from::<N>(fs, m, w, ctx, scope)?;
            }
            Field::Inline(fs) => write_from::<N>(fs, m, w, ctx, scope)?,
            Field::Const(v) => {
                w.u8(*v);
            }
            Field::Split(parts) => {
                let mut b = 0u8;
                for p in parts.iter() {
                    let name = p.name();
                    let v = match (*p, m.get(name)) {
                        (Part::Bool(..), x) => x.and_then(Value::as_bool).unwrap_or(false) as u8,
                        (Part::Rest(..), None) => 0,
                        (Part::Enum(_, _, names), Some(x)) => enum_parse(names, x, name)?,
                        (_, Some(x)) => x.as_u64().and_then(|x| u8::try_from(x).ok()).ok_or_else(|| anyhow!("field {name:?}: expected a small number"))?,
                        (_, None) => bail!("missing field {name:?}"),
                    };
                    let shift = p.mask().trailing_zeros();
                    ensure!(((v as u32) << shift) & !(p.mask() as u32) == 0, "field {name:?}: value {v} does not fit its bits");
                    b |= v << shift;
                    scope.vars.insert(name, v as u64);
                }
                w.u8(b);
            }
            Field::List(n, fs) => {
                let items = array(m, n)?;
                count(w, items.len(), n)?;
                for it in items {
                    let obj = it.as_object().ok_or_else(|| anyhow!("{n:?}: items must be objects"))?;
                    let mut s = Scope::default();
                    write_from::<N>(fs, obj, w, ctx, &mut s)?;
                }
                scope.vars.insert(len_key(n), items.len() as u64);
            }
            Field::ListOf(n, item) => {
                let items = array(m, n)?;
                count(w, items.len(), n)?;
                for it in items {
                    write_scalar(item, it, w, ctx)?;
                }
                scope.vars.insert(len_key(n), items.len() as u64);
            }
            Field::ListLike(n, other, item) => {
                let items = array(m, n)?;
                let want = scope.get(len_key(other))?;
                ensure!(items.len() as u64 == want, "{n:?} must have as many items as {other:?} ({want})");
                for it in items {
                    write_scalar(item, it, w, ctx)?;
                }
            }
            Field::RefList(n) => from_json::<RefList>(&get_or_default(m, f)?, n)?.write(w, ctx).map_err(|e| anyhow!("field {n:?}: {e}"))?,
            Field::Filter(n) => from_json::<Filter>(&get_or_default(m, f)?, n)?.write(w, ctx).map_err(|e| anyhow!("field {n:?}: {e}"))?,
            Field::Adjectives(n) => {
                let items = array(m, n)?;
                count(w, items.len(), n)?;
                for it in items {
                    write_adjective(it, w, ctx).map_err(|e| anyhow!("field {n:?}: {e}"))?;
                }
                scope.vars.insert(len_key(n), items.len() as u64);
            }
            Field::Attitudes(n) => {
                let items = array(m, n)?;
                count(w, items.len(), n)?;
                for it in items {
                    from_json::<Attitude>(it, n)?.write(w, ctx).map_err(|e| anyhow!("field {n:?}: {e}"))?;
                }
            }
            Field::Behaviour(n) => N::write_behaviour(get(m, n)?, w, ctx)?,
            Field::Behaviours(n) => {
                let items = array(m, n)?;
                count(w, items.len(), n)?;
                for it in items {
                    N::write_behaviour(it, w, ctx)?;
                }
            }
            Field::Actions(n) => {
                let items = array(m, n)?;
                count(w, items.len(), n)?;
                for it in items {
                    N::write_action(it, w, ctx)?;
                }
            }
            Field::ActionChains(n) => {
                let chains = array(m, n)?;
                count(w, chains.len(), n)?;
                for c in chains {
                    let c = c.as_array().ok_or_else(|| anyhow!("{n:?}: each chain must be an array"))?;
                    count(w, c.len(), n)?;
                    for it in c {
                        N::write_action(it, w, ctx)?;
                    }
                }
            }
            scalar => {
                let name = field_name(scalar).unwrap();
                let num = write_scalar(scalar, &get_or_default(m, scalar)?, w, ctx)?;
                scope.vars.insert(name, num);
            }
        }
    }
    Ok(())
}

/// Default JSON value for a field (used when building new records in an editor).
pub fn default_fields(schema: &'static [Field]) -> Map<String, Value> {
    let mut m = Map::new();
    fn go(schema: &'static [Field], m: &mut Map<String, Value>) {
        for f in schema {
            match f {
                Field::If(..) => {}
                Field::IfElse(_, _, b) => go(b, m),
                Field::Inline(fs) => go(fs, m),
                Field::OptU32(n) | Field::Res32(n) | Field::Res16(n) | Field::Res16Z(n) | Field::Id16(n) | Field::Entity(n) | Field::Merit(n) => {
                    m.insert(n.to_string(), Value::Null);
                }
                Field::Bool(n) => {
                    m.insert(n.to_string(), false.into());
                }
                Field::Target(n) => {
                    m.insert(n.to_string(), TARGETS[0].1.into());
                }
                Field::Skip8(n, d) => {
                    m.insert(n.to_string(), (*d).into());
                }
                Field::Split(parts) => {
                    for p in parts.iter() {
                        match p {
                            Part::Num(n, _) => {
                                m.insert(n.to_string(), 0.into());
                            }
                            Part::Enum(n, _, names) => {
                                m.insert(n.to_string(), enum_value(names, 0));
                            }
                            _ => {}
                        }
                    }
                }
                Field::Flags(n, _) | Field::List(n, _) | Field::ListOf(n, _) | Field::ListLike(n, _, _) | Field::Adjectives(n) | Field::Attitudes(n)
                | Field::Behaviours(n) | Field::Actions(n) | Field::ActionChains(n) => {
                    m.insert(n.to_string(), Value::Array(vec![]));
                }
                Field::CStr(n) | Field::Str8(n) => {
                    m.insert(n.to_string(), "".into());
                }
                Field::Bytes(n, len) => {
                    m.insert(n.to_string(), serde_json::to_value(Hex(vec![0; *len])).unwrap());
                }
                Field::ObjectPath(n) => {
                    m.insert(n.to_string(), serde_json::to_value(ObjectPath::any()).unwrap());
                }
                Field::AdjectivePath(n) => {
                    m.insert(n.to_string(), serde_json::to_value(AdjectivePath::any()).unwrap());
                }
                Field::ObjectRef(n) => {
                    m.insert(n.to_string(), serde_json::to_value(ObjectRef::default()).unwrap());
                }
                Field::RefList(n) => {
                    m.insert(n.to_string(), serde_json::to_value(RefList::default()).unwrap());
                }
                Field::Filter(n) => {
                    m.insert(n.to_string(), serde_json::to_value(Filter::default()).unwrap());
                }
                Field::Behaviour(n) => {
                    m.insert(n.to_string(), Value::Null);
                }
                other => {
                    if let Some(n) = field_name(other) {
                        m.insert(n.to_string(), 0.into());
                    }
                }
            }
        }
    }
    go(schema, &mut m);
    m
}


// ---------------------------------------------------------------------------------------------
// Shared value encodings

/// Names of the action target kinds (read by `FUN_0064f630`, resolved by `FUN_0064f570`'s
/// switch): 0 `self` = the owner (`+0x18`), 1 `trigger_target` = the object the firing trigger
/// recorded (owner `+0x9f0[trigger]`), 2 `carrier` = the root of the owner's attachment chain
/// (`FUN_006a17c0`), 3 `none` = no object (falls to the null handle `DAT_00829c70`; the
/// editor's `static_trigact_action_target` calls it TERRAIN); kind 4 is `{"entity": ...}`
/// (u32 at `+0x28`, `FUN_006a1770`).
pub const TARGETS: &[(u8, &str)] = &[(0, "self"), (1, "trigger_target"), (2, "carrier"), (3, "none")];

/// Scene entity reference: `null` (0xFFFFFFFF, none/any), `{"object": n}` (high byte 0: the
/// placed object's engine **instance id** `obj+0xc`, which is its `.sod` `objects` index minus 1 —
/// `FUN_004bdab0` creates placed object k with `FUN_006997d0(obj, ..., k - 1)`, asm `0x4be4b3
/// dec edx; push edx`, and the stage entry `objects[0]` has none; resolved by
/// `FUN_006a1770`/`FUN_006a1610` -> `FUN_0047a390`), `{"group": n}` (0xFF0000nn, object group n of the scene) or
/// `{"handle": n}` (any other value). Reference lists (`FUN_00675350`) treat every non-zero
/// high byte as a group id in the low bytes; `handle` keeps other high bytes lossless.
pub fn entity_to_json(v: u32) -> Value {
    let mut m = Map::new();
    if v == u32::MAX {
        return Value::Null;
    } else if v >> 24 == 0 {
        m.insert("object".into(), v.into());
    } else if v >> 24 == 0xff {
        m.insert("group".into(), (v & 0xff_ffff).into());
    } else {
        m.insert("handle".into(), v.into());
    }
    Value::Object(m)
}

/// Inverse of [`entity_to_json`].
pub fn json_to_entity(v: &Value) -> Result<u32> {
    let bad = || anyhow!("bad entity reference {v}, expected null, {{\"object\": n}}, {{\"group\": n}} or {{\"handle\": n}}");
    match v {
        Value::Null => Ok(u32::MAX),
        Value::Object(m) if m.len() == 1 => {
            let (k, n) = m.iter().next().unwrap();
            let n = n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(bad)?;
            match k.as_str() {
                "object" if n >> 24 == 0 => Ok(n),
                "group" if n < 0xff_ffff => Ok(0xff00_0000 | n),
                "handle" if n >> 24 != 0 && n >> 24 != 0xff => Ok(n),
                _ => Err(bad()),
            }
        }
        _ => Err(bad()),
    }
}

/// A u16 id from a named namespace, `0xFFFF` = `null`.
pub fn named16(v: u16, namespace: &str, ctx: &Context) -> Value {
    if v == u16::MAX {
        return Value::Null;
    }
    serde_json::to_value(NamedId::from_id(v as u32, namespace, None, ctx)).unwrap()
}

/// Inverse of [`named16`].
pub fn from_named16(v: &Value, namespace: &str, ctx: &Context) -> Result<u16> {
    if v.is_null() {
        return Ok(u16::MAX);
    }
    let id = from_json::<NamedId>(v, namespace)?.to_id(namespace, None, ctx)?;
    u16::try_from(id).map_err(|_| anyhow!("id {id} does not fit in 16 bits"))
}

/// An adjective list (`FUN_0053d8d0`): `u8 count; count x (u16 adjective, u16 word)`. Each
/// entry prints as the adjective resource, or `{"adjective": ..., "word": key | "hidden"}` when
/// it is displayed under another word.
pub fn read_adjectives(r: &mut Reader, ctx: &Context) -> Result<Vec<Value>> {
    let count = r.u8()?;
    (0..count).map(|_| read_adjective(r, ctx)).collect()
}

/// Inverse of [`read_adjectives`].
pub fn write_adjectives(items: &[Value], w: &mut Writer, ctx: &Context) -> Result<()> {
    crate::util::count8(w, items.len(), "adjectives")?;
    for it in items {
        write_adjective(it, w, ctx)?;
    }
    Ok(())
}

/// One entry of an adjective list (`FUN_0053d8d0`): a `.sa` resource and the word it is
/// displayed as. `word` is a key of the language's `.dtm` secondary adjective words
/// (`FUN_0073cbc0`); 0xFFFF = the adjective's own name (printed as just the adjective; the
/// name builder `FUN_0064ff80` falls back to the adjective id when `+0x10 == -1`), 0xFFFE =
/// hidden (`"word": "hidden"`; `FUN_00650010` sets the adjective's hidden flag `+0x34` for
/// word -2).
fn read_adjective(r: &mut Reader, ctx: &Context) -> Result<Value> {
    let a = json(&crate::util::res16(r.u16()?, ctx))?;
    Ok(match r.u16()? {
        0xffff => a,
        w => {
            let mut m = Map::new();
            m.insert("adjective".into(), a);
            m.insert("word".into(), if w == 0xfffe { "hidden".into() } else { w.into() });
            Value::Object(m)
        }
    })
}

fn write_adjective(v: &Value, w: &mut Writer, ctx: &Context) -> Result<()> {
    let (a, word) = match v {
        Value::Object(m) => {
            let word = match m.get("word") {
                Some(Value::String(s)) if s == "hidden" => 0xfffe,
                Some(x) => x.as_u64().and_then(|x| u16::try_from(x).ok()).filter(|&x| x < 0xfffe).ok_or_else(|| anyhow!("bad adjective word {x}"))?,
                None => 0xffff,
            };
            (m.get("adjective").cloned().unwrap_or(Value::Null), word)
        }
        other => (other.clone(), 0xffff),
    };
    let a: Option<ResRef> = from_json(&a, "adjective")?;
    w.u16(crate::util::from_res16(&a, ctx)?).u16(word);
    Ok(())
}
