//! Small value types shared by the map formats.

use scribble_core::{Context, ResRef, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};


pub use scribble_core::Fx12;

/// 24-bit colour stored as three bytes `r g b`, serialized as `"#rrggbb"`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgb(pub [u8; 3]);

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("#{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2]))
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        parse_hex_color::<3>(&s).map(Rgb).ok_or_else(|| serde::de::Error::custom(format!("bad colour {s:?}, expected #rrggbb")))
    }
}

/// 32-bit colour stored as four bytes in file order, serialized as `"#aabbccdd"`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgba(pub [u8; 4]);

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let [a, b, c, d] = self.0;
        s.serialize_str(&format!("#{a:02x}{b:02x}{c:02x}{d:02x}"))
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        parse_hex_color::<4>(&s).map(Rgba).ok_or_else(|| serde::de::Error::custom(format!("bad colour {s:?}, expected #rrggbbaa")))
    }
}

pub(crate) fn parse_hex_color<const N: usize>(s: &str) -> Option<[u8; N]> {
    let h = s.strip_prefix('#')?;
    if h.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// A u32 resource index where `0xFFFFFFFF` means "none".
pub(crate) fn opt_res(index: u32, ctx: &Context) -> Option<ResRef> {
    (index != u32::MAX).then(|| ResRef::from_index(index, ctx))
}

pub(crate) fn opt_res_index(r: &Option<ResRef>, ctx: &Context) -> Result<u32> {
    match r {
        Some(r) => r.to_index(ctx),
        None => Ok(u32::MAX),
    }
}

pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}

pub(crate) fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
