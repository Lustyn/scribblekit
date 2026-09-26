//! Small helpers shared by the codecs in this crate: strict readers, id/resource conversions and
//! the macros that make bit flags and small enumerations print as names.

use scribble_core::{bail, ns, Context, NamedId, Reader, ResRef, Result, Writer};

/// `0xFFFF` is the engine's "unset / any" value for 16-bit ids.
pub(crate) fn opt16(v: u16) -> Option<u16> {
    (v != u16::MAX).then_some(v)
}

pub(crate) fn from_opt16(v: Option<u16>) -> u16 {
    v.unwrap_or(u16::MAX)
}

/// A u32 pmindex resource index, `0xFFFFFFFF` = none.
pub(crate) fn res32(v: u32, ctx: &Context) -> Option<ResRef> {
    (v != u32::MAX).then(|| ResRef::from_index(v, ctx))
}

pub(crate) fn from_res32(v: &Option<ResRef>, ctx: &Context) -> Result<u32> {
    match v {
        Some(r) => r.to_index(ctx),
        None => Ok(u32::MAX),
    }
}

/// A u16 pmindex resource index, `0xFFFF` = none.
pub(crate) fn res16(v: u16, ctx: &Context) -> Option<ResRef> {
    (v != u16::MAX).then(|| ResRef::from_index(v as u32, ctx))
}

pub(crate) fn from_res16(v: &Option<ResRef>, ctx: &Context) -> Result<u16> {
    match v {
        Some(r) => {
            let i = r.to_index(ctx)?;
            u16::try_from(i).map_err(|_| scribble_core::anyhow!("resource index {i} does not fit in 16 bits"))
        }
        None => Ok(u16::MAX),
    }
}

/// A u16 tag id, printed as the tag's name (see `scribble_core::ns::TAG`).
pub fn tag(v: u16, ctx: &Context) -> NamedId {
    NamedId::from_id(v as u32, ns::TAG, None, ctx)
}

/// Inverse of [`tag`].
pub fn tag_id(t: &NamedId, ctx: &Context) -> Result<u16> {
    let id = t.to_id(ns::TAG, None, ctx)?;
    u16::try_from(id).map_err(|_| scribble_core::anyhow!("tag {id} does not fit in 16 bits"))
}

/// `u8 n; n x u16 tag`.
pub(crate) fn read_tags(r: &mut Reader, ctx: &Context) -> Result<Vec<NamedId>> {
    let n = r.u8()?;
    (0..n).map(|_| Ok(tag(r.u16()?, ctx))).collect()
}

pub(crate) fn write_tags(tags: &[NamedId], w: &mut Writer, ctx: &Context) -> Result<()> {
    count8(w, tags.len(), "tags")?;
    for t in tags {
        w.u16(tag_id(t, ctx)?);
    }
    Ok(())
}

/// A constant section marker byte.
pub(crate) fn marker(r: &mut Reader, expected: u8) -> Result<()> {
    let at = r.pos();
    let got = r.u8()?;
    if got != expected {
        bail!("expected section marker {expected:#04x} at {at:#x}, found {got:#04x}");
    }
    Ok(())
}

/// Write a count prefix, failing if it does not fit.
pub(crate) fn count8(w: &mut Writer, n: usize, what: &str) -> Result<()> {
    let n = u8::try_from(n).map_err(|_| scribble_core::anyhow!("too many {what} ({n}, max 255)"))?;
    w.u8(n);
    Ok(())
}

pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}

pub(crate) fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

/// Serialize the set bits of `bits` as names (`names[i]` for bit `i`, `"bit_i"` when unnamed).
pub(crate) fn flag_names(bits: u32, width: u32, names: &[&str]) -> Vec<String> {
    (0..width)
        .filter(|i| bits >> i & 1 != 0)
        .map(|i| match names.get(i as usize) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => format!("bit_{i}"),
        })
        .collect()
}

/// Inverse of [`flag_names`].
pub(crate) fn flag_bits(list: &[String], width: u32, names: &[&str]) -> Result<u32> {
    let mut v = 0u32;
    for s in list {
        let i = if let Some(i) = names.iter().position(|n| !n.is_empty() && n == s) {
            i as u32
        } else if let Some(i) = s.strip_prefix("bit_").and_then(|x| x.parse::<u32>().ok()) {
            i
        } else {
            bail!("unknown flag {s:?} (known: {names:?})");
        };
        if i >= width {
            bail!("flag {s:?} out of range");
        }
        v |= 1 << i;
    }
    Ok(v)
}

/// Declare a `u8` bit-flag newtype that serializes as a list of flag names.
///
/// ```ignore
/// flags8!(/// doc
///     Foo { 0 => "first", 3 => "fourth" });
/// ```
macro_rules! flags8 {
    ($(#[$meta:meta])* $name:ident { $($(#[$fmeta:meta])* $bit:literal => $fname:literal),* $(,)? }) => {
        $(#[$meta])*
        ///
        /// Flags (bit: name):
        $(#[doc = concat!("* bit ", stringify!($bit), ": `", $fname, "`")] $(#[$fmeta])*)*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
        pub struct $name(pub u8);

        impl $name {
            /// Flag names by bit index (`""` = unnamed, printed as `bit_N`).
            pub const NAMES: [&'static str; 8] = {
                let mut n = [""; 8];
                $(n[$bit] = $fname;)*
                n
            };
            /// Whether the named flag is set.
            pub fn has(&self, name: &str) -> bool {
                Self::NAMES.iter().position(|n| *n == name).map_or(false, |i| self.0 >> i & 1 != 0)
            }
            /// Set or clear the named flag.
            pub fn set(&mut self, name: &str, on: bool) {
                if let Some(i) = Self::NAMES.iter().position(|n| *n == name) {
                    if on { self.0 |= 1 << i } else { self.0 &= !(1 << i) }
                }
            }
            pub fn is_empty(&self) -> bool {
                self.0 == 0
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                crate::util::flag_names(self.0 as u32, 8, &Self::NAMES).serialize(s)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = Vec::<String>::deserialize(d)?;
                crate::util::flag_bits(&v, 8, &Self::NAMES).map(|b| $name(b as u8)).map_err(serde::de::Error::custom)
            }
        }
    };
}
pub(crate) use flags8;

/// Declare a `u8` enumeration that prints known values as names and unknown ones as numbers.
macro_rules! enum8 {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $val:literal => $variant:ident $sname:literal),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)*
            /// A value with no known name.
            Other(u8),
        }

        impl $name {
            pub fn from_u8(v: u8) -> Self {
                match v {
                    $($val => $name::$variant,)*
                    v => $name::Other(v),
                }
            }
            pub fn to_u8(self) -> u8 {
                match self {
                    $($name::$variant => $val,)*
                    $name::Other(v) => v,
                }
            }
            pub fn name(self) -> Option<&'static str> {
                match self {
                    $($name::$variant => Some($sname),)*
                    $name::Other(_) => None,
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                match self.name() {
                    Some(n) => s.serialize_str(n),
                    None => s.serialize_u8(self.to_u8()),
                }
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                #[derive(serde::Deserialize)]
                #[serde(untagged)]
                enum Repr {
                    N(u8),
                    S(String),
                }
                match Repr::deserialize(d)? {
                    Repr::N(v) => Ok($name::from_u8(v)),
                    Repr::S(s) => match s.as_str() {
                        $($sname => Ok($name::$variant),)*
                        _ => Err(serde::de::Error::custom(format!("unknown {} {:?}", stringify!($name), s))),
                    },
                }
            }
        }
    };
}
pub(crate) use enum8;
