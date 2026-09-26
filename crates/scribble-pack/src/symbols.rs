//! `1s`: resource symbol table, used by game code to look resources up by constant name.
//!
//! ```text
//! u32 count
//! count x { u8 len; char symbol[len] /* UTF-8 */; u32 index }
//! ```
//!
//! Symbols are normally derived from the logical path by [`derive`]; the manifest stores a
//! symbol only where the game's differs.

use scribble_core::{Reader, Result, Writer};

pub fn read(data: &[u8]) -> Result<Vec<(String, u32)>> {
    let mut r = Reader::new(data);
    let n = r.u32()?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let len = r.u8()? as usize;
        let sym = String::from_utf8(r.bytes(len)?.to_vec())?;
        out.push((sym, r.u32()?));
    }
    r.expect_end()?;
    Ok(out)
}

pub fn write(symbols: &[(String, u32)]) -> Result<Vec<u8>> {
    let mut w = Writer::new();
    w.u32(symbols.len() as u32);
    for (sym, idx) in symbols {
        let b = sym.as_bytes();
        w.u8(u8::try_from(b.len())?).bytes(b).u32(*idx);
    }
    Ok(w.into_inner())
}

/// `data\_game\foo bar.so` -> `DATA__GAME_FOO_BAR_SO`; `[platform]\x.[texture]` -> `PLATFORM_X_TEXTURE`.
pub fn derive(path: &str) -> String {
    path.to_uppercase()
        .chars()
        .filter(|c| !matches!(c, '[' | ']'))
        .map(|c| if matches!(c, '\\' | '.' | ' ') { '_' } else { c })
        .collect()
}
