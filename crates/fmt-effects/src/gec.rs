//! `.gec`: GIGL emitter collection (`gigl\prtcl\EmitterCollection.cpp`) — a named selection of
//! emitters from one particle system (`.gps`), each placed at an offset. This is what objects,
//! adjectives and levels spawn as a single "effect".
//!
//! Reader `FUN_007a3370`, writer `FUN_007a2e00`.
//!
//! ```text
//! u32 version          // always 1; the reader drops it
//! AssetRef system      // kind 3, the .gps holding the emitters
//! u32 count
//! count x {
//!     u32 len; u8 emitter[len]   // Emitter::name in the system
//!     f32 x, y                   // placement relative to the effect origin
//! }
//! ```

use crate::asset::{AssetKind, AssetRef};
use scribble_core::{Context, Format, Reader, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmitterCollection {
    /// Format version: the writer `FUN_007a2e00` always writes 1 (stream slot `+0x48`); the
    /// reader `FUN_007a3370` reads it into a local it then overwrites (1 in all 312 files).
    pub version: u32,
    /// The particle system (`.gps`) the emitters come from (asset kind 3, `<system>`; read with
    /// stream slot `+0x60` by `FUN_007a3370`).
    pub system: AssetRef,
    pub emitters: Vec<PlacedEmitter>,
}

/// An emitter of the system, instantiated at an offset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedEmitter {
    /// Name of the emitter in the particle system (string, slot `+0x04`; the writer asserts
    /// `types.size() == positions.size()`, `EmitterCollection.cpp:0x1e`).
    pub emitter: String,
    /// Offset from the effect's origin (vec2, slot `+0x2c`; kept in the parallel `positions`
    /// list).
    #[serde(with = "crate::float::pair")]
    pub position: [f32; 2],
}

impl Format for EmitterCollection {
    const NAME: &'static str = "gec";
    const DESCRIPTION: &'static str = "Effect: emitters picked from a particle system, with positions";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let version = r.u32()?;
        let system = AssetRef::read(&mut r, AssetKind::System, ctx)?;
        let n = r.u32()?;
        let mut emitters = Vec::new();
        for _ in 0..n {
            emitters.push(PlacedEmitter { emitter: r.str_u32()?, position: [r.f32()?, r.f32()?] });
        }
        r.expect_end()?;
        Ok(EmitterCollection { version, system, emitters })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u32(self.version);
        self.system.write(&mut w, AssetKind::System, ctx)?;
        w.u32(self.emitters.len() as u32);
        for e in &self.emitters {
            w.str_u32(&e.emitter)?;
            w.f32(e.position[0]).f32(e.position[1]);
        }
        Ok(w.into_inner())
    }
}
