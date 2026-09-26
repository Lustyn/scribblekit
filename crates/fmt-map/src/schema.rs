//! A small declarative schema language for the many small non-script records in scene
//! (`.sod`) files: placed-object bodies, links, joints, liquids, rails, decorations, hints,
//! lights, effects and doors. (Scripts use `fmt_object::record`, shared with objects.)
//!
//! Each reader is transcribed as a list of [`F`] field descriptors; one interpreter decodes
//! bytes into an ordered JSON object ([`Record`]) and encodes it back. Optional fields that the
//! engine reads only when a flag bit is set are omitted from the record when absent, and the
//! flag byte itself is kept (as a list of bit names) so the round trip is exact.

use crate::common::parse_hex_color;
use scribble_core::{bail, ensure, Context, Fx16, Hex, Reader, ResRef, Result, Writer};
use serde_json::{Map, Number, Value};

/// An ordered JSON object holding one decoded record.
pub type Record = Map<String, Value>;

/// Field descriptor. The `&str` is the JSON key.
#[derive(Clone, Copy)]
pub enum F {
    U8(&'static str),
    I8(&'static str),
    U16(&'static str),
    I16(&'static str),
    U32(&'static str),
    I32(&'static str),
    /// u8 omitted from the record when equal to the default.
    U8D(&'static str, u8),
    /// i8 omitted from the record when equal to the default.
    I8D(&'static str, i8),
    /// i32 20.12 fixed point, omitted when zero.
    Fx12Z(&'static str),
    /// u8 that is 0 or 1.
    Bool(&'static str),
    /// i32 16.16 fixed point.
    Fx16(&'static str),
    /// i32 20.12 fixed point.
    Fx12(&'static str),
    /// u16 pmindex index (objects `.so` and adjectives `.sa`), 0xFFFF = null.
    Res16(&'static str),
    /// u32 pmindex index, 0xFFFFFFFF = null.
    Res32(&'static str),
    /// u32 reference to a scene entity, see [`entity_to_json`].
    Entity(&'static str),
    /// u8 enumeration, serialized as the name at that index (a number when out of range or
    /// unnamed).
    Enum8(&'static str, &'static [&'static str]),
    /// u8 bit set, serialized as a list of the given bit names (`bit_N` when unnamed);
    /// omitted when no bit is set.
    Flags(&'static str, &'static [&'static str]),
    /// NUL-terminated string.
    Cstr(&'static str),
    /// u8 length + characters.
    Str8(&'static str),
    /// Fixed number of opaque bytes (hex).
    Bytes(&'static str, usize),
    /// Fixed number of u8 values (a JSON array of numbers, no count).
    ArrayU8(&'static str, usize),
    /// A u8 split into bit fields, each its own key (see [`P`]); the masks cover all 8 bits.
    Split(&'static [P]),
    /// 4 bytes shown as `#aabbccdd` in file order.
    Color4(&'static str),
    /// 3 bytes shown as `#rrggbb`.
    Color3(&'static str),
    /// u32 length + opaque bytes (hex).
    Blob32(&'static str),
    /// Fields present when `field & mask != 0` (field is an earlier U8/Flags of this record).
    If(&'static str, u32, &'static [F]),
    /// Fields present when `field & mask == 0`.
    IfNot(&'static str, u32, &'static [F]),
    /// Fields present when `field & mask == value`.
    IfMaskEq(&'static str, u32, u32, &'static [F]),
    /// u8 count + records.
    List(&'static str, &'static [F]),
    /// A nested record (no count).
    Group(&'static str, &'static [F]),
    /// u8 count + u8 values.
    ListU8(&'static str),
    /// u8 count + u16 resources.
    ListRes16(&'static str),
    /// u8 count + u32 resources.
    ListRes32(&'static str),
    /// u8 count + u32 entity references.
    ListEntity(&'static str),
    /// One u16 resource per element of the earlier list field (no count of its own).
    ParallelRes16(&'static str, &'static str),
    /// Adjective list (`FUN_0053d8d0`), shared with scripts: see
    /// [`fmt_object::record::read_adjectives`].
    Adjectives(&'static str),
}

/// One bit field of an [`F::Split`] byte. Conditions (`If`) can refer to part names.
#[derive(Clone, Copy)]
pub enum P {
    /// A single bit, printed as `true` (omitted when clear).
    Bool(&'static str, u8),
    /// `(byte & mask) >> shift`, omitted when equal to the default.
    Num(&'static str, u8, u8),
    /// Like `Num`, printed as the name at that index when there is one.
    Enum(&'static str, u8, &'static [&'static str], u8),
}

/// Numbers of the current record (and its enclosing records) that conditions may refer to.
#[derive(Default)]
struct Scope<'p> {
    nums: Vec<(&'static str, u32)>,
    parent: Option<&'p Scope<'p>>,
}

impl<'p> Scope<'p> {
    fn child(&'p self) -> Scope<'p> {
        Scope { nums: Vec::new(), parent: Some(self) }
    }
    fn set(&mut self, k: &'static str, v: u32) {
        self.nums.push((k, v));
    }
    fn get(&self, k: &str) -> Result<u32> {
        match self.nums.iter().rev().find(|(n, _)| *n == k) {
            Some((_, v)) => Ok(*v),
            None => match self.parent {
                Some(p) => p.get(k),
                None => bail!("schema: field {k:?} not read yet"),
            },
        }
    }
}

fn num<T: Into<Number>>(v: T) -> Value {
    Value::Number(v.into())
}

fn res_json(index: Option<u32>, ctx: &Context) -> Value {
    match index {
        None => Value::Null,
        Some(i) => match ResRef::from_index(i, ctx) {
            ResRef::Path(p) => Value::String(p),
            ResRef::Index(i) => num(i),
        },
    }
}

fn json_res(v: &Value, ctx: &Context) -> Result<Option<u32>> {
    Ok(match v {
        Value::Null => None,
        Value::String(p) => Some(ResRef::Path(p.clone()).to_index(ctx)?),
        Value::Number(n) => Some(n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| anyhow::anyhow!("bad resource index {n}"))?),
        v => bail!("expected resource path or index, got {v}"),
    })
}

pub use fmt_object::record::{entity_to_json, json_to_entity};

pub fn flag_list(v: u32, names: &[&str]) -> Value {
    Value::Array(crate::stp::flag_names(v, names, 8).into_iter().map(Value::String).collect())
}

fn list_flags(v: &Value, names: &[&str]) -> Result<u32> {
    let list: Vec<String> = serde_json::from_value(v.clone())?;
    crate::stp::flag_value(&list, names)
}

fn get<'a>(rec: &'a Record, k: &str) -> Result<&'a Value> {
    rec.get(k).ok_or_else(|| anyhow::anyhow!("missing field {k:?}"))
}

fn as_int(v: &Value, k: &str) -> Result<i64> {
    v.as_i64().ok_or_else(|| anyhow::anyhow!("field {k:?}: expected integer, got {v}"))
}

fn int_field<T: TryFrom<i64>>(rec: &Record, k: &str) -> Result<T> {
    let v = as_int(get(rec, k)?, k)?;
    T::try_from(v).map_err(|_| anyhow::anyhow!("field {k:?}: {v} out of range"))
}

fn str_field<'a>(rec: &'a Record, k: &str) -> Result<&'a str> {
    get(rec, k)?.as_str().ok_or_else(|| anyhow::anyhow!("field {k:?}: expected string"))
}

fn hex_field(rec: &Record, k: &str) -> Result<Vec<u8>> {
    let h: Hex = serde_json::from_value(get(rec, k)?.clone())?;
    Ok(h.0)
}

fn hex_value(b: &[u8]) -> Value {
    Value::String(Hex(b.to_vec()).to_string())
}

pub fn decode(fields: &[F], r: &mut Reader, ctx: &Context) -> Result<Record> {
    let mut rec = Record::new();
    let mut scope = Scope::default();
    decode_into(fields, r, ctx, &mut rec, &mut scope)?;
    Ok(rec)
}

fn decode_into(fields: &[F], r: &mut Reader, ctx: &Context, rec: &mut Record, scope: &mut Scope<'_>) -> Result<()> {
    for f in fields {
        match *f {
            F::U8(k) => {
                let v = r.u8()?;
                scope.set(k, v as u32);
                rec.insert(k.into(), num(v));
            }
            F::I8(k) => {
                rec.insert(k.into(), num(r.i8()?));
            }
            F::U8D(k, d) => {
                let v = r.u8()?;
                scope.set(k, v as u32);
                if v != d {
                    rec.insert(k.into(), num(v));
                }
            }
            F::I8D(k, d) => {
                let v = r.i8()?;
                if v != d {
                    rec.insert(k.into(), num(v));
                }
            }
            F::Fx12Z(k) => {
                let v = r.i32()?;
                if v != 0 {
                    rec.insert(k.into(), serde_json::to_value(crate::common::Fx12(v))?);
                }
            }
            F::U16(k) => {
                rec.insert(k.into(), num(r.u16()?));
            }
            F::I16(k) => {
                rec.insert(k.into(), num(r.i16()?));
            }
            F::U32(k) => {
                rec.insert(k.into(), num(r.u32()?));
            }
            F::I32(k) => {
                rec.insert(k.into(), num(r.i32()?));
            }
            F::Bool(k) => {
                let v = r.u8()?;
                ensure!(v <= 1, "field {k:?}: expected 0/1, got {v}");
                rec.insert(k.into(), Value::Bool(v == 1));
            }
            F::Fx16(k) => {
                rec.insert(k.into(), serde_json::to_value(Fx16(r.i32()?))?);
            }
            F::Fx12(k) => {
                rec.insert(k.into(), serde_json::to_value(crate::common::Fx12(r.i32()?))?);
            }
            F::Res16(k) => {
                let v = r.u16()?;
                rec.insert(k.into(), res_json((v != u16::MAX).then_some(v as u32), ctx));
            }
            F::Res32(k) => {
                let v = r.u32()?;
                rec.insert(k.into(), res_json((v != u32::MAX).then_some(v), ctx));
            }
            F::Entity(k) => {
                rec.insert(k.into(), entity_to_json(r.u32()?));
            }
            F::Split(parts) => {
                let b = r.u8()?;
                for p in parts {
                    let (k, mask) = match *p {
                        P::Bool(k, m) | P::Num(k, m, _) | P::Enum(k, m, _, _) => (k, m),
                    };
                    let v = (b & mask) >> mask.trailing_zeros();
                    scope.set(k, v as u32);
                    match *p {
                        P::Bool(..) if v != 0 => {
                            rec.insert(k.into(), Value::Bool(true));
                        }
                        P::Num(_, _, d) if v != d => {
                            rec.insert(k.into(), num(v));
                        }
                        P::Enum(_, _, names, d) if v != d => {
                            let val = match names.get(v as usize) {
                                Some(n) if !n.is_empty() => Value::String(n.to_string()),
                                _ => num(v),
                            };
                            rec.insert(k.into(), val);
                        }
                        _ => {}
                    }
                }
            }
            F::Flags(k, names) => {
                let v = r.u8()?;
                scope.set(k, v as u32);
                if v != 0 {
                    rec.insert(k.into(), flag_list(v as u32, names));
                }
            }
            F::Enum8(k, names) => {
                let v = r.u8()?;
                scope.set(k, v as u32);
                let val = match names.get(v as usize) {
                    Some(n) if !n.is_empty() => Value::String(n.to_string()),
                    _ => num(v),
                };
                rec.insert(k.into(), val);
            }
            F::Cstr(k) => {
                rec.insert(k.into(), Value::String(r.cstr()?));
            }
            F::Str8(k) => {
                rec.insert(k.into(), Value::String(r.str_u8()?));
            }
            F::Bytes(k, len) => {
                rec.insert(k.into(), hex_value(r.bytes(len)?));
            }
            F::ArrayU8(k, len) => {
                let items = r.bytes(len)?.iter().map(|b| num(*b)).collect();
                rec.insert(k.into(), Value::Array(items));
            }
            F::Color4(k) => {
                rec.insert(k.into(), serde_json::to_value(crate::common::Rgba(r.array()?))?);
            }
            F::Color3(k) => {
                rec.insert(k.into(), serde_json::to_value(crate::common::Rgb(r.array()?))?);
            }
            F::Blob32(k) => {
                let len = r.u32()? as usize;
                rec.insert(k.into(), hex_value(r.bytes(len)?));
            }
            F::If(k, mask, sub) => {
                if scope.get(k)? & mask != 0 {
                    decode_into(sub, r, ctx, rec, scope)?;
                }
            }
            F::IfNot(k, mask, sub) => {
                if scope.get(k)? & mask == 0 {
                    decode_into(sub, r, ctx, rec, scope)?;
                }
            }
            F::IfMaskEq(k, mask, value, sub) => {
                if scope.get(k)? & mask == value {
                    decode_into(sub, r, ctx, rec, scope)?;
                }
            }
            F::List(k, sub) => {
                let count = r.u8()?;
                let mut items = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let mut sub_rec = Record::new();
                    decode_into(sub, r, ctx, &mut sub_rec, &mut scope.child())?;
                    items.push(Value::Object(sub_rec));
                }
                scope.set(k, count as u32);
                rec.insert(k.into(), Value::Array(items));
            }
            F::Group(k, sub) => {
                let mut sub_rec = Record::new();
                decode_into(sub, r, ctx, &mut sub_rec, &mut scope.child())?;
                rec.insert(k.into(), Value::Object(sub_rec));
            }
            F::ListU8(k) => {
                let count = r.u8()?;
                let items = r.bytes(count as usize)?.iter().map(|b| num(*b)).collect();
                rec.insert(k.into(), Value::Array(items));
            }
            F::ListRes16(k) => {
                let count = r.u8()?;
                let mut items = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let v = r.u16()?;
                    items.push(res_json((v != u16::MAX).then_some(v as u32), ctx));
                }
                rec.insert(k.into(), Value::Array(items));
            }
            F::ListRes32(k) => {
                let count = r.u8()?;
                let mut items = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let v = r.u32()?;
                    items.push(res_json((v != u32::MAX).then_some(v), ctx));
                }
                rec.insert(k.into(), Value::Array(items));
            }
            F::ListEntity(k) => {
                let count = r.u8()?;
                let mut items = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    items.push(entity_to_json(r.u32()?));
                }
                rec.insert(k.into(), Value::Array(items));
            }
            F::ParallelRes16(k, of) => {
                let count = scope.get(of)?;
                let mut items = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let v = r.u16()?;
                    items.push(res_json((v != u16::MAX).then_some(v as u32), ctx));
                }
                rec.insert(k.into(), Value::Array(items));
            }
            F::Adjectives(k) => {
                let items = fmt_object::record::read_adjectives(r, ctx)?;
                scope.set(k, items.len() as u32);
                rec.insert(k.into(), Value::Array(items));
            }
        }
    }
    Ok(())
}

pub fn encode(fields: &[F], rec: &Record, w: &mut Writer, ctx: &Context) -> Result<()> {
    let mut scope = Scope::default();
    encode_from(fields, rec, w, ctx, &mut scope)
}

fn encode_from(fields: &[F], rec: &Record, w: &mut Writer, ctx: &Context, scope: &mut Scope<'_>) -> Result<()> {
    for f in fields {
        match *f {
            F::U8(k) => {
                let v: u8 = int_field(rec, k)?;
                scope.set(k, v as u32);
                w.u8(v);
            }
            F::I8(k) => {
                w.i8(int_field(rec, k)?);
            }
            F::U8D(k, d) => {
                let v: u8 = if rec.contains_key(k) { int_field(rec, k)? } else { d };
                scope.set(k, v as u32);
                w.u8(v);
            }
            F::I8D(k, d) => {
                let v: i8 = if rec.contains_key(k) { int_field(rec, k)? } else { d };
                w.i8(v);
            }
            F::Fx12Z(k) => {
                let v = match rec.get(k) {
                    Some(v) => serde_json::from_value::<crate::common::Fx12>(v.clone())?.0,
                    None => 0,
                };
                w.i32(v);
            }
            F::U16(k) => {
                w.u16(int_field(rec, k)?);
            }
            F::I16(k) => {
                w.i16(int_field(rec, k)?);
            }
            F::U32(k) => {
                w.u32(int_field(rec, k)?);
            }
            F::I32(k) => {
                w.i32(int_field(rec, k)?);
            }
            F::Bool(k) => {
                let b = get(rec, k)?.as_bool().ok_or_else(|| anyhow::anyhow!("field {k:?}: expected bool"))?;
                w.u8(b as u8);
            }
            F::Fx16(k) => {
                let v: Fx16 = serde_json::from_value(get(rec, k)?.clone())?;
                w.i32(v.0);
            }
            F::Fx12(k) => {
                let v: crate::common::Fx12 = serde_json::from_value(get(rec, k)?.clone())?;
                w.i32(v.0);
            }
            F::Res16(k) => {
                let v = json_res(get(rec, k)?, ctx)?.unwrap_or(0xffff);
                w.u16(u16::try_from(v).map_err(|_| anyhow::anyhow!("field {k:?}: resource index {v} does not fit in 16 bits"))?);
            }
            F::Res32(k) => {
                w.u32(json_res(get(rec, k)?, ctx)?.unwrap_or(u32::MAX));
            }
            F::Entity(k) => {
                w.u32(json_to_entity(get(rec, k)?)?);
            }
            F::Enum8(k, names) => {
                let v = match get(rec, k)? {
                    Value::String(s) => names.iter().position(|n| !n.is_empty() && n == s).ok_or_else(|| anyhow::anyhow!("field {k:?}: unknown value {s:?} (expected one of {names:?})"))? as u8,
                    v => u8::try_from(as_int(v, k)?)?,
                };
                scope.set(k, v as u32);
                w.u8(v);
            }
            F::Split(parts) => {
                let mut b = 0u8;
                for p in parts {
                    let (k, mask) = match *p {
                        P::Bool(k, m) | P::Num(k, m, _) | P::Enum(k, m, _, _) => (k, m),
                    };
                    let v: u8 = match (*p, rec.get(k)) {
                        (P::Bool(..), x) => x.and_then(Value::as_bool).unwrap_or(false) as u8,
                        (P::Num(_, _, d) | P::Enum(_, _, _, d), None) => d,
                        (P::Enum(_, _, names, _), Some(Value::String(s))) => {
                            names.iter().position(|n| !n.is_empty() && n == s).ok_or_else(|| anyhow::anyhow!("field {k:?}: unknown value {s:?}"))? as u8
                        }
                        (_, Some(x)) => u8::try_from(as_int(x, k)?)?,
                    };
                    let shift = mask.trailing_zeros();
                    ensure!(((v as u32) << shift) & !(mask as u32) == 0, "field {k:?}: {v} does not fit its bits");
                    b |= v << shift;
                    scope.set(k, v as u32);
                }
                w.u8(b);
            }
            F::Flags(k, names) => {
                let v = match rec.get(k) {
                    Some(v) => list_flags(v, names)?,
                    None => 0,
                };
                ensure!(v <= 0xff, "field {k:?}: flag bit out of range");
                scope.set(k, v);
                w.u8(v as u8);
            }
            F::Cstr(k) => {
                w.cstr(str_field(rec, k)?)?;
            }
            F::Str8(k) => {
                w.str_u8(str_field(rec, k)?)?;
            }
            F::Bytes(k, len) => {
                let b = hex_field(rec, k)?;
                ensure!(b.len() == len, "field {k:?}: expected {len} bytes, got {}", b.len());
                w.bytes(&b);
            }
            F::ArrayU8(k, len) => {
                let items = array_field(rec, k)?;
                ensure!(items.len() == len, "field {k:?}: expected {len} values, got {}", items.len());
                for it in items {
                    w.u8(u8::try_from(as_int(it, k)?)?);
                }
            }
            F::Color4(k) => {
                let s = str_field(rec, k)?;
                w.bytes(&parse_hex_color::<4>(s).ok_or_else(|| anyhow::anyhow!("field {k:?}: bad colour {s:?}"))?);
            }
            F::Color3(k) => {
                let s = str_field(rec, k)?;
                w.bytes(&parse_hex_color::<3>(s).ok_or_else(|| anyhow::anyhow!("field {k:?}: bad colour {s:?}"))?);
            }
            F::Blob32(k) => {
                let b = hex_field(rec, k)?;
                w.u32(b.len() as u32).bytes(&b);
            }
            F::If(k, mask, sub) => {
                if scope.get(k)? & mask != 0 {
                    encode_from(sub, rec, w, ctx, scope)?;
                }
            }
            F::IfNot(k, mask, sub) => {
                if scope.get(k)? & mask == 0 {
                    encode_from(sub, rec, w, ctx, scope)?;
                }
            }
            F::IfMaskEq(k, mask, value, sub) => {
                if scope.get(k)? & mask == value {
                    encode_from(sub, rec, w, ctx, scope)?;
                }
            }
            F::List(k, sub) => {
                let items = array_field(rec, k)?;
                w.u8(u8::try_from(items.len())?);
                for it in items {
                    let m = it.as_object().ok_or_else(|| anyhow::anyhow!("field {k:?}: expected objects"))?;
                    encode_from(sub, m, w, ctx, &mut scope.child())?;
                }
                scope.set(k, items.len() as u32);
            }
            F::Group(k, sub) => {
                let m = get(rec, k)?.as_object().ok_or_else(|| anyhow::anyhow!("field {k:?}: expected object"))?;
                encode_from(sub, m, w, ctx, &mut scope.child())?;
            }
            F::ListU8(k) => {
                let items = array_field(rec, k)?;
                w.u8(u8::try_from(items.len())?);
                for it in items {
                    w.u8(u8::try_from(as_int(it, k)?)?);
                }
            }
            F::ListRes16(k) => {
                let items = array_field(rec, k)?;
                w.u8(u8::try_from(items.len())?);
                for it in items {
                    w.u16(u16::try_from(json_res(it, ctx)?.unwrap_or(0xffff))?);
                }
            }
            F::ListRes32(k) => {
                let items = array_field(rec, k)?;
                w.u8(u8::try_from(items.len())?);
                for it in items {
                    w.u32(json_res(it, ctx)?.unwrap_or(u32::MAX));
                }
            }
            F::ListEntity(k) => {
                let items = array_field(rec, k)?;
                w.u8(u8::try_from(items.len())?);
                for it in items {
                    w.u32(json_to_entity(it)?);
                }
            }
            F::ParallelRes16(k, of) => {
                let items = array_field(rec, k)?;
                ensure!(items.len() as u32 == scope.get(of)?, "field {k:?} must have one entry per element of {of:?}");
                for it in items {
                    w.u16(u16::try_from(json_res(it, ctx)?.unwrap_or(0xffff))?);
                }
            }
            F::Adjectives(k) => {
                let items = array_field(rec, k)?;
                fmt_object::record::write_adjectives(items, w, ctx).map_err(|e| anyhow::anyhow!("field {k:?}: {e}"))?;
                scope.set(k, items.len() as u32);
            }
        }
    }
    Ok(())
}

pub(crate) fn array_field<'a>(rec: &'a Record, k: &str) -> Result<&'a Vec<Value>> {
    get(rec, k)?.as_array().ok_or_else(|| anyhow::anyhow!("field {k:?}: expected array"))
}

