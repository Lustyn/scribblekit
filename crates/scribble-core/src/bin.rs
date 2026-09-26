//! Little-endian cursor reader and writer. All game data is little-endian (x86 PC build).

use anyhow::{bail, ensure, Result};

#[derive(Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

macro_rules! read_prim {
    ($($name:ident: $ty:ty),*) => {$(
        pub fn $name(&mut self) -> Result<$ty> {
            Ok(<$ty>::from_le_bytes(self.array()?))
        }
    )*};
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    pub fn at_end(&self) -> bool {
        self.pos == self.data.len()
    }
    pub fn seek(&mut self, pos: usize) -> Result<()> {
        ensure!(pos <= self.data.len(), "seek to {pos:#x} past end ({:#x})", self.data.len());
        self.pos = pos;
        Ok(())
    }
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.remaining() {
            bail!("read of {n} bytes at {:#x} overruns end ({:#x})", self.pos, self.data.len());
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.data[self.pos..];
        self.pos = self.data.len();
        s
    }
    pub fn peek_u8(&self) -> Result<u8> {
        self.data.get(self.pos).copied().ok_or_else(|| anyhow::anyhow!("peek past end"))
    }
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.bytes(N)?.try_into().unwrap())
    }
    read_prim!(u8: u8, i8: i8, u16: u16, i16: i16, u32: u32, i32: i32, u64: u64, i64: i64, f32: f32, f64: f64);

    pub fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => bail!("expected bool at {:#x}, got {v}", self.pos - 1),
        }
    }
    /// NUL-terminated string (NUL consumed, not included).
    pub fn cstr(&mut self) -> Result<String> {
        let rest = &self.data[self.pos..];
        let n = rest.iter().position(|&b| b == 0).ok_or_else(|| anyhow::anyhow!("unterminated string at {:#x}", self.pos))?;
        let s = latin1(&rest[..n]);
        self.pos += n + 1;
        Ok(s)
    }
    /// String with a u8 length prefix.
    pub fn str_u8(&mut self) -> Result<String> {
        let n = self.u8()? as usize;
        Ok(latin1(self.bytes(n)?))
    }
    /// String with a u16 length prefix.
    pub fn str_u16(&mut self) -> Result<String> {
        let n = self.u16()? as usize;
        Ok(latin1(self.bytes(n)?))
    }
    /// String with a u32 length prefix.
    pub fn str_u32(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        Ok(latin1(self.bytes(n)?))
    }
    pub fn expect_end(&self) -> Result<()> {
        ensure!(self.at_end(), "{} trailing bytes at {:#x}", self.remaining(), self.pos);
        Ok(())
    }
}

/// Game text is single-byte Windows-1252/Latin-1; map bytes 1:1 to U+0000..U+00FF so it round-trips.
pub fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

pub fn to_latin1(s: &str) -> Result<Vec<u8>> {
    s.chars()
        .map(|c| u8::try_from(c as u32).map_err(|_| anyhow::anyhow!("character {c:?} not representable in Latin-1")))
        .collect()
}

#[derive(Default, Clone)]
pub struct Writer {
    pub buf: Vec<u8>,
}

macro_rules! write_prim {
    ($($name:ident: $ty:ty),*) => {$(
        pub fn $name(&mut self, v: $ty) -> &mut Self {
            self.buf.extend_from_slice(&v.to_le_bytes());
            self
        }
    )*};
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn pos(&self) -> usize {
        self.buf.len()
    }
    pub fn into_inner(self) -> Vec<u8> {
        self.buf
    }
    write_prim!(u8: u8, i8: i8, u16: u16, i16: i16, u32: u32, i32: i32, u64: u64, i64: i64, f32: f32, f64: f64);

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(v as u8)
    }
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(b);
        self
    }
    pub fn zeros(&mut self, n: usize) -> &mut Self {
        self.buf.resize(self.buf.len() + n, 0);
        self
    }
    pub fn cstr(&mut self, s: &str) -> Result<&mut Self> {
        let b = to_latin1(s)?;
        ensure!(!b.contains(&0), "string contains NUL");
        Ok(self.bytes(&b).u8(0))
    }
    pub fn str_u8(&mut self, s: &str) -> Result<&mut Self> {
        let b = to_latin1(s)?;
        let n = u8::try_from(b.len()).map_err(|_| anyhow::anyhow!("string too long for u8 length: {s:?}"))?;
        Ok(self.u8(n).bytes(&b))
    }
    pub fn str_u16(&mut self, s: &str) -> Result<&mut Self> {
        let b = to_latin1(s)?;
        let n = u16::try_from(b.len()).map_err(|_| anyhow::anyhow!("string too long for u16 length"))?;
        Ok(self.u16(n).bytes(&b))
    }
    pub fn str_u32(&mut self, s: &str) -> Result<&mut Self> {
        let b = to_latin1(s)?;
        Ok(self.u32(b.len() as u32).bytes(&b))
    }
    /// Overwrite a previously written u32 (e.g. a size or offset back-patch).
    pub fn patch_u32(&mut self, at: usize, v: u32) {
        self.buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn patch_u16(&mut self, at: usize, v: u16) {
        self.buf[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
}
