//! Shared building blocks for Scribblenauts Unlimited asset codecs.
//!
//! Every asset format implements [`Format`]: a lossless `decode` from the game's
//! bytes into a serde-friendly struct and an `encode` back to identical bytes.
//! The struct is what users read and edit, serialized with [`json::to_string`].

pub mod bin;
pub mod context;
pub mod format;
pub mod json;
pub mod respath;
pub mod testing;
pub mod types;

pub use bin::{Reader, Writer};
pub use context::{ns, Context, IdNames, Platform};
pub use format::{Codec, Format, FormatCodec};
pub use respath::ResPath;
pub use types::{Fx12, Fx16, Hex, NamedId, NamedPath, ResRef};

pub type Result<T, E = anyhow::Error> = std::result::Result<T, E>;
pub use anyhow::{anyhow, bail, ensure, Context as ResultExt};
