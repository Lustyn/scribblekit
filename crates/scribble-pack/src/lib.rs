//! The pack container.
//!
//! A game install holds resources in a handful of `.p` pack files, described by:
//!
//! * `index.bin` — one [`IndexEntry`] per resource index (entry 0 is a null placeholder),
//!   followed by the list of pack filenames. See [`IndexBin`].
//! * `pmindex.xml` / `pmindex_for_code.xml` — resource index -> logical path, GUID and the
//!   "chunky" flag (whether the payload is a chunk list). The two files differ only in GUIDs.
//! * `1s` — resource symbol (`DATA__GAME_..._SO`) -> index, sorted by index. See [`symbols`].
//!
//! Each `.p` file starts with a 64-byte header whose first u32 is the total file size; the rest
//! of the header is zero. Payloads follow back to back, either stored (with one trailing NUL
//! not counted in the entry's raw size) or zlib-compressed.
//!
//! [`unpack`] writes every resource to a directory tree plus a human-readable `manifest.json`;
//! [`pack`] rebuilds a game directory from that. Repacking is content-lossless: every resource
//! decodes to identical bytes (zlib streams are recompressed, so compressed bytes may differ).

pub mod index;
pub mod manifest;
pub mod pmindex;
pub mod symbols;

pub use index::{IndexBin, IndexEntry};
pub use manifest::{pack, unpack, Manifest, ManifestFile};

use scribble_core::Context;
use std::path::Path;

/// Build a resource-name [`Context`] from a game directory (`pmindex.xml`) or an unpacked
/// directory (`manifest.json`).
pub fn load_context(dir: &Path) -> scribble_core::Result<Context> {
    if dir.join(manifest::MANIFEST).exists() {
        let m = Manifest::load(dir)?;
        let mut ctx = Context::from_names(m.files.into_iter().map(|f| (f.index, f.path)));
        ctx.set_platform(m.platform);
        return Ok(ctx);
    }
    let x = pmindex::PmIndex::read(&std::fs::read(dir.join("pmindex.xml"))?)?;
    Ok(Context::from_names(x.files.into_iter().map(|f| (f.index, f.name))))
}
