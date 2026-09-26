//! Readable f32 in JSON.
//!
//! Codecs go through `serde_json::Value`, which stores numbers as f64, so a plain `f32` field
//! prints as e.g. `0.10000000149011612`. These `serde(with = ...)` helpers emit the f64 whose
//! decimal form is the shortest one that round-trips the f32 (`0.1`), and map it back to exactly
//! the original f32 when parsing.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The f64 with the shortest decimal representation that identifies `x`.
pub(crate) fn to_json(x: f32) -> f64 {
    if x.is_finite() { format!("{x}").parse().unwrap() } else { x as f64 }
}

/// Inverse of [`to_json`] (also accepts any other number, rounded to nearest).
pub(crate) fn from_json(v: f64) -> f32 {
    let c = v as f32;
    if c.is_finite() {
        for cand in [c, c.next_up(), c.next_down()] {
            if to_json(cand).to_bits() == v.to_bits() {
                return cand;
            }
        }
    }
    c
}

pub(crate) mod scalar {
    use super::*;
    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        to_json(*v).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        f64::deserialize(d).map(from_json)
    }
}

pub(crate) mod pair {
    use super::*;
    pub fn serialize<S: Serializer>(v: &[f32; 2], s: S) -> Result<S::Ok, S::Error> {
        [to_json(v[0]), to_json(v[1])].serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[f32; 2], D::Error> {
        <[f64; 2]>::deserialize(d).map(|v| [from_json(v[0]), from_json(v[1])])
    }
}

pub(crate) mod opt {
    use super::*;
    pub fn serialize<S: Serializer>(v: &Option<f32>, s: S) -> Result<S::Ok, S::Error> {
        v.map(to_json).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f32>, D::Error> {
        Option::<f64>::deserialize(d).map(|v| v.map(from_json))
    }
}

pub(crate) mod rows {
    use super::*;
    pub fn serialize<S: Serializer>(v: &[Vec<f32>], s: S) -> Result<S::Ok, S::Error> {
        v.iter().map(|r| r.iter().map(|&x| to_json(x)).collect::<Vec<_>>()).collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vec<f32>>, D::Error> {
        Vec::<Vec<f64>>::deserialize(d).map(|v| v.into_iter().map(|r| r.into_iter().map(from_json).collect()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact() {
        for x in [0.1f32, -0.0, 0.0, 1e-45, f32::MAX, f32::MIN_POSITIVE, 3.1415927, -107374176.0, 1.0 / 3.0] {
            let v = to_json(x);
            let text = serde_json::to_string(&v).unwrap();
            let back: f64 = serde_json::from_str(&text).unwrap();
            assert_eq!(from_json(back).to_bits(), x.to_bits(), "{x} -> {text}");
        }
        assert_eq!(serde_json::to_string(&to_json(0.1)).unwrap(), "0.1");
    }
}
