//! Helpers for small integer enumerations and bit sets whose values the engine gives meaning to.

/// An enumeration stored as an integer: known values print as snake_case names, anything else
/// stays a plain number (lossless).
macro_rules! int_enum {
    ($(#[$m:meta])* pub enum $name:ident : $t:ty { $($(#[$vm:meta])* $var:ident = $val:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($(#[$vm])* $var,)*
            /// A value with no known meaning.
            #[serde(untagged)]
            Other($t),
        }
        impl $name {
            pub fn from_raw(v: $t) -> Self {
                match v {
                    $($val => Self::$var,)*
                    v => Self::Other(v),
                }
            }
            pub fn to_raw(self) -> $t {
                match self {
                    $(Self::$var => $val,)*
                    Self::Other(v) => v,
                }
            }
        }
    };
}

/// A u8 bit set serialized as a list of flag names; bits without a name print as `"bit_N"`.
macro_rules! bit_flags {
    ($(#[$m:meta])* pub struct $name:ident { $($bit:literal => $flag:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
        pub struct $name(pub u8);
        impl $name {
            const NAMES: &'static [(u8, &'static str)] = &[$(($bit, $flag)),*];
            pub fn has(self, bit: u8) -> bool {
                self.0 & (1 << bit) != 0
            }
            pub fn is_empty(&self) -> bool {
                self.0 == 0
            }
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let names: Vec<String> = (0..8u8)
                    .filter(|b| self.has(*b))
                    .map(|b| match Self::NAMES.iter().find(|(n, _)| *n == b) {
                        Some((_, f)) => f.to_string(),
                        None => format!("bit_{b}"),
                    })
                    .collect();
                names.serialize(s)
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let names = Vec::<String>::deserialize(d)?;
                let mut v = 0u8;
                for n in names {
                    let bit = match Self::NAMES.iter().find(|(_, f)| *f == n) {
                        Some((b, _)) => *b,
                        None => n
                            .strip_prefix("bit_")
                            .and_then(|b| b.parse::<u8>().ok())
                            .filter(|b| *b < 8)
                            .ok_or_else(|| serde::de::Error::custom(format!("unknown flag {n:?}")))?,
                    };
                    v |= 1 << bit;
                }
                Ok($name(v))
            }
        }
    };
}

pub(crate) use {bit_flags, int_enum};
