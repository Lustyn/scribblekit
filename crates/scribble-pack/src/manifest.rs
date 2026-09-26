//! Unpacking a game directory to loose files + `manifest.json`, and packing it back.

use crate::index::{IndexBin, IndexEntry, FLAG_STORED, FLAG_ZLIB, NO_PACK};
use crate::pmindex::{PmFile, PmIndex};
use crate::symbols;
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use scribble_core::{bail, ensure, Platform, ResultExt as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "manifest.json";
/// Size of the header at the start of every `.p` file (u32 file size, u32 load-into-memory
/// flag: nonzero makes `FUN_004944a0` read the whole pack into RAM; zero elsewhere).
pub const PACK_HEADER_SIZE: usize = 0x40;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The build the resources come from (omitted for PC).
    #[serde(default, skip_serializing_if = "Platform::is_pc")]
    pub platform: Platform,
    /// Pack filenames in pack-id order.
    pub packs: Vec<String>,
    /// Every resource, in index order.
    pub files: Vec<ManifestFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFile {
    pub index: u32,
    /// Logical path with backslashes, as the game names it (`data\_game\...`).
    pub path: String,
    /// Pack holding the data; `None` for resources listed in pmindex without any data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<String>,
    /// Stored zlib-compressed in the pack.
    pub compressed: bool,
    /// Payload is a chunk list (pmindex `chunky` attribute).
    pub chunky: bool,
    pub guid: String,
    /// GUID used in `pmindex_for_code.xml`.
    pub code_guid: String,
    /// Symbol in `1s`, only when it differs from [`symbols::derive`] of the path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

impl ManifestFile {
    pub fn disk_path(&self, root: &Path) -> PathBuf {
        disk_path(root, &self.path)
    }
}

/// Map a logical `a\b\c.ext` path under `root`.
pub fn disk_path(root: &Path, logical: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    p.extend(logical.split('\\'));
    p
}

impl Manifest {
    pub fn load(dir: &Path) -> Result<Self> {
        let text = fs::read_to_string(dir.join(MANIFEST)).with_context(|| format!("reading {}", dir.join(MANIFEST).display()))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        // One file per line keeps a 31k-entry manifest diffable.
        let mut s = String::from("{\n");
        if !self.platform.is_pc() {
            s.push_str(&format!("  \"platform\": {},\n", serde_json::to_string(&self.platform)?));
        }
        s.push_str("  \"packs\": ");
        s.push_str(&serde_json::to_string(&self.packs)?);
        s.push_str(",\n  \"files\": [\n");
        for (i, f) in self.files.iter().enumerate() {
            s.push_str("    ");
            s.push_str(&serde_json::to_string(f)?);
            s.push_str(if i + 1 < self.files.len() { ",\n" } else { "\n" });
        }
        s.push_str("  ]\n}\n");
        fs::write(dir.join(MANIFEST), s)?;
        Ok(())
    }
}

/// Read one resource's decompressed bytes straight from a game directory.
pub struct GameReader {
    pub dir: PathBuf,
    pub index: IndexBin,
    pub pm: PmIndex,
    files: HashMap<u8, fs::File>,
    by_name: HashMap<String, u32>,
}

impl GameReader {
    pub fn open(dir: &Path) -> Result<Self> {
        let index = IndexBin::read(&fs::read(dir.join("index.bin"))?)?;
        let pm = PmIndex::read(&fs::read(dir.join("pmindex.xml"))?)?;
        let by_name = pm.files.iter().map(|f| (f.name.clone(), f.index)).collect();
        Ok(GameReader { dir: dir.to_path_buf(), index, pm, files: HashMap::new(), by_name })
    }

    /// A reader over a game directory's packs described by an already-parsed `index.bin`, for
    /// builds without `pmindex.xml` (the Wii U build); [`find`](Self::find) finds nothing.
    pub fn with_index(dir: &Path, index: IndexBin) -> Self {
        GameReader { dir: dir.to_path_buf(), index, pm: PmIndex { files: Vec::new() }, files: HashMap::new(), by_name: HashMap::new() }
    }

    pub fn find(&self, logical: &str) -> Option<u32> {
        self.by_name.get(logical).copied()
    }

    pub fn read(&mut self, index: u32) -> Result<Vec<u8>> {
        let e = *self.index.entries.get(index as usize).with_context(|| format!("no index entry {index}"))?;
        ensure!(e.pack != NO_PACK, "resource {index} has no data");
        if e.raw_size == 0 {
            return Ok(Vec::new()); // placeholder entries (data\_game\dummy\*)
        }
        let f = match self.files.entry(e.pack) {
            std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(fs::File::open(self.dir.join(&self.index.packs[e.pack as usize]))?)
            }
        };
        f.seek(SeekFrom::Start(e.offset as u64))?;
        let mut stored = vec![0; e.stored_size as usize];
        f.read_exact(&mut stored)?;
        Ok(match e.flags {
            FLAG_ZLIB => {
                let mut out = Vec::with_capacity(e.raw_size as usize);
                ZlibDecoder::new(&stored[..]).read_to_end(&mut out)?;
                ensure!(out.len() == e.raw_size as usize, "resource {index}: inflated {} bytes, expected {}", out.len(), e.raw_size);
                out
            }
            FLAG_STORED => {
                stored.truncate(e.raw_size as usize);
                stored
            }
            f => bail!("resource {index}: unknown flags {f}"),
        })
    }
}

/// Extract every resource from `game` into `out`, writing `out/manifest.json`.
pub fn unpack(game: &Path, out: &Path, mut progress: impl FnMut(usize, usize)) -> Result<Manifest> {
    let mut g = GameReader::open(game)?;
    let code = PmIndex::read(&fs::read(game.join("pmindex_for_code.xml"))?)?;
    let code_guids: HashMap<u32, String> = code.files.into_iter().map(|f| (f.index, f.guid)).collect();
    let syms: HashMap<u32, String> = symbols::read(&fs::read(game.join("1s"))?)?.into_iter().map(|(s, i)| (i, s)).collect();

    let pm_files = g.pm.files.clone();
    let mut files = Vec::with_capacity(pm_files.len());
    for (n, f) in pm_files.iter().enumerate() {
        progress(n, pm_files.len());
        let e = g.index.entries[f.index as usize];
        if e.pack != NO_PACK {
            let data = g.read(f.index)?;
            let path = disk_path(out, &f.name);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(&path, &data)?;
        }
        let symbol = syms.get(&f.index).filter(|s| **s != symbols::derive(&f.name)).cloned();
        files.push(ManifestFile {
            index: f.index,
            path: f.name.clone(),
            pack: (e.pack != NO_PACK).then(|| g.index.packs[e.pack as usize].clone()),
            compressed: e.flags == FLAG_ZLIB,
            chunky: f.chunky,
            guid: f.guid.clone(),
            code_guid: code_guids.get(&f.index).cloned().unwrap_or_else(|| f.guid.clone()),
            symbol,
        });
    }
    let m = Manifest { platform: Platform::Pc, packs: g.index.packs.clone(), files };
    m.save(out)?;
    Ok(m)
}

/// Rebuild `index.bin`, `pmindex.xml`, `pmindex_for_code.xml`, `1s` and all `.p` files in
/// `out` from an unpacked directory.
pub fn pack(unpacked: &Path, out: &Path, mut progress: impl FnMut(usize, usize)) -> Result<()> {
    let m = Manifest::load(unpacked)?;
    fs::create_dir_all(out)?;
    let pack_id: HashMap<&str, u8> = m.packs.iter().enumerate().map(|(i, p)| (p.as_str(), i as u8)).collect();
    let mut writers = m
        .packs
        .iter()
        .map(|p| {
            let mut f = std::io::BufWriter::new(fs::File::create(out.join(p))?);
            f.write_all(&[0; PACK_HEADER_SIZE])?;
            Ok((f, PACK_HEADER_SIZE as u64))
        })
        .collect::<Result<Vec<_>>>()?;

    let count = m.files.iter().map(|f| f.index + 1).max().unwrap_or(1);
    let null = IndexEntry { pack: NO_PACK, flags: 0, offset: 0, stored_size: 0, raw_size: 0 };
    let mut entries = vec![null; count as usize];
    for (n, f) in m.files.iter().enumerate() {
        progress(n, m.files.len());
        let Some(pack) = &f.pack else { continue };
        let data = fs::read(f.disk_path(unpacked)).with_context(|| format!("reading {}", f.path))?;
        let pid = *pack_id.get(pack.as_str()).with_context(|| format!("{}: unknown pack {pack}", f.path))?;
        let stored = if f.compressed {
            let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
            z.write_all(&data)?;
            z.finish()?
        } else {
            let mut d = data.clone();
            d.push(0);
            d
        };
        let (w, pos) = &mut writers[pid as usize];
        w.write_all(&stored)?;
        entries[f.index as usize] = IndexEntry {
            pack: pid,
            flags: if f.compressed { FLAG_ZLIB } else { FLAG_STORED },
            offset: *pos as u32,
            stored_size: stored.len() as u32,
            raw_size: data.len() as u32,
        };
        *pos += stored.len() as u64;
    }
    for (p, (w, size)) in m.packs.iter().zip(writers) {
        let mut f = w.into_inner().map_err(|e| e.into_error())?;
        ensure!(size <= u32::MAX as u64, "{p} exceeds 4 GiB");
        f.seek(SeekFrom::Start(0))?;
        f.write_all(&(size as u32).to_le_bytes())?;
    }

    fs::write(out.join("index.bin"), IndexBin { entries, packs: m.packs.clone() }.write()?)?;
    let pm = |code: bool| PmIndex {
        files: m
            .files
            .iter()
            .map(|f| PmFile { name: f.path.clone(), index: f.index, guid: if code { f.code_guid.clone() } else { f.guid.clone() }, chunky: f.chunky })
            .collect(),
    };
    fs::write(out.join("pmindex.xml"), pm(false).write())?;
    fs::write(out.join("pmindex_for_code.xml"), pm(true).write())?;
    let syms: Vec<(String, u32)> =
        m.files.iter().map(|f| (f.symbol.clone().unwrap_or_else(|| symbols::derive(&f.path)), f.index)).collect();
    fs::write(out.join("1s"), symbols::write(&syms)?)?;
    Ok(())
}
