//! The [`Format`] trait every codec implements, and a type-erased [`Codec`] for registries.

use crate::{json, Context, Result};
use anyhow::{bail, Context as _};
use serde::{de::DeserializeOwned, Serialize};
use std::marker::PhantomData;

/// A game file format with a lossless binary <-> structured-data mapping.
///
/// Contract: for every file `b` the game ships, `encode(decode(b)) == b` byte-for-byte, and the
/// decoded value survives a trip through JSON text unchanged.
pub trait Format: Sized + Serialize + DeserializeOwned {
    /// Short identifier, usually the file extension (`"so"`, `"vec"`).
    const NAME: &'static str;
    /// One line describing what the format holds.
    const DESCRIPTION: &'static str;

    fn decode(data: &[u8], ctx: &Context) -> Result<Self>;
    fn encode(&self, ctx: &Context) -> Result<Vec<u8>>;
}

/// Object-safe view of a [`Format`].
///
/// Prefer the `*_text` methods for anything user-facing: they serialize the typed value
/// directly, which keeps `f32`s short. The `*_json` methods go through `serde_json::Value`
/// (whose numbers are f64) and are meant for programmatic inspection.
pub trait Codec: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// Binary -> readable JSON text.
    fn decode_text(&self, data: &[u8], ctx: &Context) -> Result<String>;
    /// JSON text -> binary.
    fn encode_text(&self, text: &str, ctx: &Context) -> Result<Vec<u8>>;
    fn decode_json(&self, data: &[u8], ctx: &Context) -> Result<serde_json::Value>;
    fn encode_json(&self, value: serde_json::Value, ctx: &Context) -> Result<Vec<u8>>;

    /// decode -> JSON text -> parse -> encode, and require identical bytes.
    fn roundtrip(&self, data: &[u8], ctx: &Context) -> Result<()> {
        let text = self.decode_text(data, ctx).context("decode")?;
        let out = self.encode_text(&text, ctx).context("encode")?;
        if out != data {
            let at = out.iter().zip(data).position(|(a, b)| a != b).unwrap_or(out.len().min(data.len()));
            bail!("re-encoded bytes differ at offset {at:#x} (original {} bytes, re-encoded {})", data.len(), out.len());
        }
        Ok(())
    }
}

pub struct FormatCodec<T>(PhantomData<fn() -> T>);

impl<T> Default for FormatCodec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> FormatCodec<T> {
    pub const fn new() -> Self {
        FormatCodec(PhantomData)
    }
}

impl<T: Format> Codec for FormatCodec<T> {
    fn name(&self) -> &'static str {
        T::NAME
    }
    fn description(&self) -> &'static str {
        T::DESCRIPTION
    }
    fn decode_text(&self, data: &[u8], ctx: &Context) -> Result<String> {
        json::to_string_of(&T::decode(data, ctx)?)
    }
    fn encode_text(&self, text: &str, ctx: &Context) -> Result<Vec<u8>> {
        let v: T = serde_json::from_str(text)?;
        v.encode(ctx)
    }
    fn decode_json(&self, data: &[u8], ctx: &Context) -> Result<serde_json::Value> {
        Ok(serde_json::to_value(T::decode(data, ctx)?)?)
    }
    fn encode_json(&self, value: serde_json::Value, ctx: &Context) -> Result<Vec<u8>> {
        let v: T = serde_json::from_value(value)?;
        v.encode(ctx)
    }
}

/// Test helper: assert a typed round trip (binary -> T -> JSON -> T -> binary).
pub fn assert_roundtrip<T: Format>(data: &[u8], ctx: &Context) -> Result<T> {
    let v = T::decode(data, ctx)?;
    let text = json::to_string_of(&v)?;
    let back: T = serde_json::from_str(&text)?;
    let out = back.encode(ctx)?;
    if out != data {
        let at = out.iter().zip(data).position(|(a, b)| a != b).unwrap_or(out.len().min(data.len()));
        bail!("{}: re-encoded bytes differ at offset {at:#x} (original {} bytes, re-encoded {})", T::NAME, data.len(), out.len());
    }
    Ok(v)
}
