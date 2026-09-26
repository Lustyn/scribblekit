//! `index.bin`: the table of where each resource lives.
//!
//! ```text
//! u32 count                        // number of entries, including the null entry 0
//! count x {
//!     u8  pack                     // index into the pack list below; 0xFF = no resource
//!     u8  flags                    // 0 = stored, 6 = zlib
//!     u32 offset                   // byte offset of the payload within the pack
//!     u32 stored_size              // bytes occupied in the pack
//!     u32 raw_size                 // size of the resource after decompression
//! }
//! u32 pack_count
//! pack_count x { u8 len; char name[len] }
//! ```
//!
//! The Wii U build stores the same layout big-endian ([`IndexBin::read_be`]).

use scribble_core::{Reader, Result, Writer};
use serde::{Deserialize, Serialize};

pub const FLAG_STORED: u8 = 0;
pub const FLAG_ZLIB: u8 = 6;
pub const NO_PACK: u8 = 0xFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub pack: u8,
    pub flags: u8,
    pub offset: u32,
    pub stored_size: u32,
    pub raw_size: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexBin {
    pub entries: Vec<IndexEntry>,
    pub packs: Vec<String>,
}

impl IndexBin {
    pub fn read(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let n = r.u32()?;
        let entries = (0..n)
            .map(|_| {
                Ok(IndexEntry { pack: r.u8()?, flags: r.u8()?, offset: r.u32()?, stored_size: r.u32()?, raw_size: r.u32()? })
            })
            .collect::<Result<_>>()?;
        let np = r.u32()?;
        let packs = (0..np).map(|_| r.str_u8()).collect::<Result<_>>()?;
        r.expect_end()?;
        Ok(IndexBin { entries, packs })
    }

    /// Read the Wii U build's big-endian `index.bin`.
    pub fn read_be(data: &[u8]) -> Result<Self> {
        let mut pos = 0;
        let mut take = |n: usize| -> Result<&[u8]> {
            let b = data.get(pos..pos + n).ok_or_else(|| scribble_core::anyhow!("index.bin truncated at {pos:#x}"))?;
            pos += n;
            Ok(b)
        };
        let u32be = |b: &[u8]| u32::from_be_bytes(b.try_into().unwrap());
        let n = u32be(take(4)?);
        let mut entries = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let e = take(14)?;
            entries.push(IndexEntry { pack: e[0], flags: e[1], offset: u32be(&e[2..6]), stored_size: u32be(&e[6..10]), raw_size: u32be(&e[10..14]) });
        }
        let np = u32be(take(4)?);
        let mut packs = Vec::with_capacity(np as usize);
        for _ in 0..np {
            let len = take(1)?[0] as usize;
            packs.push(String::from_utf8(take(len)?.to_vec())?);
        }
        scribble_core::ensure!(pos == data.len(), "{} trailing bytes after index.bin", data.len() - pos);
        Ok(IndexBin { entries, packs })
    }

    pub fn write(&self) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u32(self.entries.len() as u32);
        for e in &self.entries {
            w.u8(e.pack).u8(e.flags).u32(e.offset).u32(e.stored_size).u32(e.raw_size);
        }
        w.u32(self.packs.len() as u32);
        for p in &self.packs {
            w.str_u8(p)?;
        }
        Ok(w.into_inner())
    }
}
