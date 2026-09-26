//! Reading the Wii U build's packs.
//!
//! The Wii U build (title 00050000-1010B200) uses the PC container with two differences:
//! `index.bin` is big-endian ([`IndexBin::read_be`]) and there is no `pmindex.xml`, so logical
//! paths are rebuilt from the `1s` symbols ([`symbols::derive`] is lossy: `\`, `.` and ` ` all
//! became `_`). A symbol the PC build also has takes the PC path; the others (the Wii U-only
//! resources) are guessed by [`PathGuesser`] from the directories the PC build uses.

use scribble_core::{bail, ensure, Context, Platform, Result, ResultExt as _};
use scribble_pack::index::NO_PACK;
use scribble_pack::manifest::{disk_path, GameReader, Manifest, ManifestFile};
use scribble_pack::{symbols, IndexBin};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// One resource of the Wii U build.
#[derive(Clone, Debug)]
pub struct WiiuResource {
    pub index: u32,
    /// Symbol from `1s` (`DATA__GAME_SCRIBBLEOBJECTS_..._SO`).
    pub symbol: String,
    /// Logical path: the PC build's when it has the same symbol, else guessed.
    pub path: String,
    /// Whether the PC build has a resource with this symbol.
    pub on_pc: bool,
}

/// The Wii U build's `content` directory.
pub struct WiiuGame {
    pub reader: GameReader,
    /// Every resource with a symbol, in index order.
    pub resources: Vec<WiiuResource>,
}

/// Find the `content` directory of a Wii U dump: `dir` itself, or `dir/content` (a dump with
/// `code/`, `content/` and `meta/`).
pub fn content_dir(dir: &Path) -> Result<PathBuf> {
    for d in [dir.to_path_buf(), dir.join("content")] {
        if d.join("index.bin").is_file() && d.join("1s").is_file() {
            return Ok(d);
        }
    }
    bail!("{} is not a Wii U Scribblenauts Unlimited dump (no content/index.bin and content/1s)", dir.display())
}

impl WiiuGame {
    /// Open a Wii U dump. `pc_paths` maps the PC build's symbols to its logical paths.
    pub fn open(dir: &Path, pc_paths: &HashMap<String, String>) -> Result<Self> {
        let dir = content_dir(dir)?;
        let index_bytes = fs::read(dir.join("index.bin"))?;
        ensure!(
            index_bytes.len() >= 4 && u32::from_be_bytes(index_bytes[..4].try_into().unwrap()) < 0x0100_0000,
            "{} is little-endian: this looks like a PC install, not a Wii U dump",
            dir.join("index.bin").display()
        );
        let index = IndexBin::read_be(&index_bytes).context("reading the Wii U index.bin")?;
        let syms = symbols::read(&fs::read(dir.join("1s"))?).context("reading the Wii U 1s")?;
        let guesser = PathGuesser::new(pc_paths.values().map(String::as_str));
        let mut resources = Vec::with_capacity(syms.len());
        for (symbol, idx) in syms {
            ensure!((idx as usize) < index.entries.len(), "symbol {symbol} has index {idx} beyond index.bin");
            let (path, on_pc) = match pc_paths.get(&symbol) {
                Some(p) => (p.clone(), true),
                None => (guesser.guess(&symbol)?, false),
            };
            resources.push(WiiuResource { index: idx, symbol, path, on_pc });
        }
        resources.sort_by_key(|r| r.index);
        Ok(WiiuGame { reader: GameReader::with_index(&dir, index), resources })
    }

    /// Resource names for codecs, marked as the Wii U build.
    pub fn context(&self) -> Context {
        let mut ctx = Context::from_names(self.resources.iter().map(|r| (r.index, r.path.clone())));
        ctx.set_platform(Platform::WiiU);
        ctx
    }

    pub fn has_data(&self, index: u32) -> bool {
        self.reader.index.entries.get(index as usize).is_some_and(|e| e.pack != NO_PACK)
    }

    pub fn read(&mut self, index: u32) -> Result<Vec<u8>> {
        self.reader.read(index)
    }

    /// Extract every resource to `out` with a `manifest.json` (platform `wiiu`), like
    /// [`scribble_pack::unpack`]. The Wii U build has no GUIDs or chunky flags; they are taken
    /// from the PC resource of the same path where there is one (`pc`), else made up.
    pub fn unpack(&mut self, out: &Path, pc: Option<&Manifest>, mut progress: impl FnMut(usize, usize)) -> Result<Manifest> {
        let pc_files: HashMap<&str, &ManifestFile> = pc.map(|m| m.files.iter().map(|f| (f.path.as_str(), f)).collect()).unwrap_or_default();
        let chunky = chunky_by_extension(pc.map(|m| m.files.as_slice()).unwrap_or(&[]));
        let resources = self.resources.clone();
        let mut files = Vec::with_capacity(resources.len());
        for (n, r) in resources.iter().enumerate() {
            progress(n, resources.len());
            let e = self.reader.index.entries[r.index as usize];
            if e.pack != NO_PACK {
                let data = self.read(r.index).with_context(|| format!("reading {}", r.path))?;
                let path = disk_path(out, &r.path);
                fs::create_dir_all(path.parent().unwrap())?;
                fs::write(&path, &data)?;
            }
            let pc = pc_files.get(r.path.as_str());
            let guid = pc.map(|f| f.guid.clone()).unwrap_or_else(|| made_up_guid(&r.path));
            files.push(ManifestFile {
                index: r.index,
                path: r.path.clone(),
                pack: (e.pack != NO_PACK).then(|| self.reader.index.packs[e.pack as usize].clone()),
                compressed: e.flags == scribble_pack::index::FLAG_ZLIB,
                chunky: pc.map(|f| f.chunky).unwrap_or_else(|| chunky(&r.path)),
                code_guid: pc.map(|f| f.code_guid.clone()).unwrap_or_else(|| guid.clone()),
                guid,
                symbol: (r.symbol != symbols::derive(&r.path)).then(|| r.symbol.clone()),
            });
        }
        let m = Manifest { platform: Platform::WiiU, packs: self.reader.index.packs.clone(), files };
        m.save(out)?;
        Ok(m)
    }
}

/// The PC build's symbol -> path map from an unpacked tree's manifest.
pub fn pc_paths_from_manifest(m: &Manifest) -> HashMap<String, String> {
    m.files.iter().map(|f| (f.symbol.clone().unwrap_or_else(|| symbols::derive(&f.path)), f.path.clone())).collect()
}

/// The chunky flag most PC resources with the same extension have (pmindex records it for
/// every resource; resources of one extension almost always agree).
pub fn chunky_by_extension(files: &[ManifestFile]) -> impl Fn(&str) -> bool + use<> {
    let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
    for f in files {
        let c = counts.entry(extension(&f.path).to_string()).or_default();
        if f.chunky { c.0 += 1 } else { c.1 += 1 }
    }
    move |path: &str| counts.get(extension(path)).is_some_and(|(yes, no)| yes > no)
}

fn extension(path: &str) -> &str {
    let file = path.rsplit('\\').next().unwrap_or(path);
    file.rsplit_once('.').map(|(_, e)| e).unwrap_or("")
}

/// A stable GUID-shaped id for a resource that has none (FNV-1a of the path; the engine never
/// reads pmindex GUIDs).
pub fn made_up_guid(path: &str) -> String {
    let fnv = |seed: u64| path.bytes().fold(seed, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let (a, b) = (fnv(0xcbf2_9ce4_8422_2325), fnv(0x6c62_272e_07bb_0142));
    format!("{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}", a >> 32, (a >> 16) & 0xffff, a & 0xfff, (b >> 48) & 0xfff, b & 0xffff_ffff_ffff)
}

/// Rebuilds logical paths from symbols for resources the PC build does not have.
///
/// The longest known directory whose symbol form prefixes the symbol is taken as the directory;
/// the rest is the file name with its last `_` as the extension dot. Two conventions of the
/// shipped Wii U-only files make them nicer: a leading `_x_` names a subdirectory `_x`
/// (`[platform]\datavector\_nin\mario_texture.vec`), and a repeated first word names a
/// subdirectory (`data\meshanim\_nin\boo\boo_idle.anim`). The result always derives back to
/// the symbol.
pub struct PathGuesser {
    dirs: HashMap<String, String>,
}

impl PathGuesser {
    pub fn new<'a>(known_paths: impl IntoIterator<Item = &'a str>) -> Self {
        let mut dirs = HashMap::new();
        for p in known_paths {
            let parts: Vec<&str> = p.split('\\').collect();
            for k in 1..parts.len() {
                let d = parts[..k].join("\\");
                dirs.insert(format!("{}_", symbols::derive(&d)), d);
            }
        }
        PathGuesser { dirs }
    }

    pub fn guess(&self, symbol: &str) -> Result<String> {
        let (prefix, dir) = self
            .dirs
            .iter()
            .filter(|(p, _)| symbol.starts_with(p.as_str()))
            .max_by_key(|(p, _)| p.len())
            .with_context(|| format!("no known directory for symbol {symbol}"))?;
        let rest = symbol[prefix.len()..].to_lowercase();
        let mut toks: Vec<&str> = rest.split('_').collect();
        let mut segs: Vec<String> = vec![dir.clone()];
        if toks.len() > 3 && toks[0].is_empty() {
            segs.push(format!("_{}", toks[1]));
            toks.drain(..2);
        }
        if toks.len() > 2 && toks[0] == toks[1] {
            segs.push(toks[0].to_string());
            toks.remove(0);
        }
        let (ext, stem) = toks.split_last().with_context(|| format!("empty file name in symbol {symbol}"))?;
        let ext = if *ext == "texture" { "[texture]".to_string() } else { ext.to_string() };
        segs.push(if stem.is_empty() { ext } else { format!("{}.{ext}", stem.join("_")) });
        let path = segs.join("\\");
        ensure!(symbols::derive(&path) == symbol, "guessed path {path:?} does not derive to {symbol}");
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_wiiu_only_paths() {
        let g = PathGuesser::new([
            "data\\_game\\scribbleobjects\\cow.so",
            "data\\_game\\scribbleadjectives\\large.sa",
            "[platform]\\datavector\\clothes\\legs\\dresspants.vec",
            "data\\meshanim\\insects\\air\\firefly\\firefly_scaredfly.anim",
            "[platform]\\audio\\sfx\\animal_ape1.wav",
        ]);
        let cases = [
            ("DATA__GAME_SCRIBBLEOBJECTS_EASTEREGG_NINTENDO_CHARACTER_MARIO_SO", "data\\_game\\scribbleobjects\\easteregg_nintendo_character_mario.so"),
            ("DATA__GAME_SCRIBBLEADJECTIVES__FIREFLOWER_SA", "data\\_game\\scribbleadjectives\\_fireflower.sa"),
            ("PLATFORM_DATAVECTOR__NIN_MARIO_TEXTURE_VEC", "[platform]\\datavector\\_nin\\mario_texture.vec"),
            ("DATA_MESHANIM__NIN_BOO_BOO_IDLE_ANIM", "data\\meshanim\\_nin\\boo\\boo_idle.anim"),
            ("PLATFORM_AUDIO_SFX_NINTENDO_MARIO_JUMP1_WAV", "[platform]\\audio\\sfx\\nintendo_mario_jump1.wav"),
        ];
        for (sym, path) in cases {
            assert_eq!(g.guess(sym).unwrap(), path);
        }
    }

    #[test]
    fn made_up_guids_are_stable() {
        assert_eq!(made_up_guid("a\\b.so"), made_up_guid("a\\b.so"));
        assert_ne!(made_up_guid("a\\b.so"), made_up_guid("a\\c.so"));
        assert_eq!(made_up_guid("x").len(), 36);
    }
}
