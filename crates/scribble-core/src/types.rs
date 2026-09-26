//! Small value types that make binary fields readable in JSON while staying lossless.

use crate::context::Context;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Signed 16.16 fixed-point number (raw `i32 / 65536`). Serialized as an exact decimal
/// (every 16.16 value is exactly representable as an f64), so `1.5` in JSON means raw `0x18000`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Fx16(pub i32);

impl Fx16 {
    pub const ONE: Fx16 = Fx16(0x10000);
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / 65536.0
    }
    pub fn from_f64(v: f64) -> Option<Self> {
        let raw = v * 65536.0;
        (raw.fract() == 0.0 && raw >= i32::MIN as f64 && raw <= i32::MAX as f64).then_some(Fx16(raw as i32))
    }
    /// Nearest representable value (for editors that produce arbitrary floats).
    pub fn from_f64_lossy(v: f64) -> Self {
        Fx16((v * 65536.0).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
    }
}

impl fmt::Debug for Fx16 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_f64())
    }
}

impl Serialize for Fx16 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(self.to_f64())
    }
}

impl<'de> Deserialize<'de> for Fx16 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = f64::deserialize(d)?;
        Fx16::from_f64(v).ok_or_else(|| serde::de::Error::custom(format!("{v} is not an exact 16.16 fixed-point value")))
    }
}

/// Signed 20.12 fixed-point number (raw `i32 / 4096`), the engine's "world" scale (`0x1000` =
/// 1.0). Serialized as an exact decimal like [`Fx16`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Fx12(pub i32);

impl Fx12 {
    pub const ONE: Fx12 = Fx12(0x1000);
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / 4096.0
    }
    pub fn from_f64(v: f64) -> Option<Self> {
        let raw = v * 4096.0;
        (raw.fract() == 0.0 && raw >= i32::MIN as f64 && raw <= i32::MAX as f64).then_some(Fx12(raw as i32))
    }
    /// Nearest representable value.
    pub fn from_f64_lossy(v: f64) -> Self {
        Fx12((v * 4096.0).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
    }
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

impl fmt::Debug for Fx12 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_f64())
    }
}

impl Serialize for Fx12 {
    /// Whole numbers print without a fraction (`3`, not `3.0`).
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v = self.to_f64();
        if v.fract() == 0.0 { s.serialize_i64(v as i64) } else { s.serialize_f64(v) }
    }
}

impl<'de> Deserialize<'de> for Fx12 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = f64::deserialize(d)?;
        Fx12::from_f64(v).ok_or_else(|| serde::de::Error::custom(format!("{v} is not an exact 20.12 fixed-point value")))
    }
}

/// Opaque bytes, serialized as space-separated hex (`"0a ff 00"`). Use only for regions whose
/// meaning is genuinely unknown or irrelevant (padding); prefer typed fields everywhere else.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Hex(pub Vec<u8>);

impl fmt::Debug for Hex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Hex({})", self)
    }
}

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl Serialize for Hex {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Hex {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let digits: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        if !digits.len().is_multiple_of(2) {
            return Err(serde::de::Error::custom("odd number of hex digits"));
        }
        (0..digits.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).map_err(serde::de::Error::custom))
            .collect::<Result<_, _>>()
            .map(Hex)
    }
}

/// A reference to another packed resource by its pmindex index. Decoded to the resource's
/// logical path when the [`Context`] knows it, so JSON reads `"data\\_game\\...\\cow.so"`
/// instead of `7682`. Unknown indices stay numeric, so the mapping is always lossless.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResRef {
    Path(String),
    Index(u32),
}

impl ResRef {
    pub fn from_index(index: u32, ctx: &Context) -> Self {
        match ctx.name(index) {
            Some(n) => ResRef::Path(n.to_string()),
            None => ResRef::Index(index),
        }
    }
    pub fn to_index(&self, ctx: &Context) -> crate::Result<u32> {
        match self {
            ResRef::Index(i) => Ok(*i),
            ResRef::Path(p) => ctx.index(p).ok_or_else(|| anyhow::anyhow!("unknown resource path {p:?}")),
        }
    }
}

/// An id from a named namespace of the [`Context`] (see [`Context::namespace`]), e.g. a merit or
/// a dictionary word. Printed as its name when the namespace has one (`name#id` when the name is
/// ambiguous) and as the plain number otherwise, so the mapping is always lossless.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NamedId {
    Name(String),
    Id(u32),
}

impl NamedId {
    /// Name `id` in namespace `ns`; `parent` narrows ambiguous names in hierarchical namespaces.
    pub fn from_id(id: u32, ns: &str, parent: Option<u32>, ctx: &Context) -> Self {
        match ctx.namespace(ns) {
            Some(names) if names.name(id).is_some() => NamedId::Name(names.display(id, parent)),
            _ => NamedId::Id(id),
        }
    }
    pub fn to_id(&self, ns: &str, parent: Option<u32>, ctx: &Context) -> crate::Result<u32> {
        match self {
            NamedId::Id(i) => Ok(*i),
            NamedId::Name(s) => crate::context::parse_component(ctx.namespace(ns), s, parent).map_err(|e| anyhow::anyhow!("{ns}: {e}")),
        }
    }
}

/// A path of ids through a hierarchy of namespaces (one per level), e.g. an object's taxonomy
/// `category/subcategory/group/object`. Printed as `"mammal/large/hooved/cow"`: each component
/// is the level's name for the id (resolved within its parent where names repeat), `name#id`
/// when a name is ambiguous, the decimal id when it has no name, and `*` for the wildcard value.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NamedPath(pub String);

impl NamedPath {
    /// Print `ids` (one per level of `levels`; `wildcard` prints as `*`).
    pub fn from_ids(ids: &[u32], levels: &[&str], wildcard: u32, ctx: &Context) -> Self {
        let mut parts = Vec::with_capacity(ids.len());
        let mut parent = None;
        for (i, &id) in ids.iter().enumerate() {
            parts.push(if id == wildcard {
                "*".to_string()
            } else {
                match levels.get(i).and_then(|l| ctx.namespace(l)) {
                    Some(ns) => ns.display(id, parent),
                    None => id.to_string(),
                }
            });
            parent = (id != wildcard).then_some(id);
        }
        NamedPath(parts.join("/"))
    }
    /// Inverse of [`NamedPath::from_ids`]; requires exactly `levels.len()` components.
    pub fn to_ids(&self, levels: &[&str], wildcard: u32, ctx: &Context) -> crate::Result<Vec<u32>> {
        let parts: Vec<&str> = self.0.split('/').collect();
        anyhow::ensure!(parts.len() == levels.len(), "{:?}: expected {} components separated by '/'", self.0, levels.len());
        let mut out = Vec::with_capacity(parts.len());
        let mut parent = None;
        for (part, level) in parts.iter().zip(levels) {
            let id = if *part == "*" {
                wildcard
            } else {
                crate::context::parse_component(ctx.namespace(level), part, parent).map_err(|e| anyhow::anyhow!("{:?}: {level}: {e}", self.0))?
            };
            out.push(id);
            parent = (id != wildcard).then_some(id);
        }
        Ok(out)
    }
    /// Whether every component is the wildcard.
    pub fn is_any(&self) -> bool {
        self.0.split('/').all(|p| p == "*")
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
