//! Porting the Wii U-exclusive content (the Nintendo easter eggs) into a PC install.
//!
//! The PC release shipped with that content stripped: the objects, adjectives, art, animations
//! and sounds are gone, `_lm7`/`_lm8` (super star / super mushroom) are empty stubs, the
//! grappling hook's end has no art (`0xFFFFFFFF`), the sound table has 71 slots with no sound,
//! and a few adjectives still name the Nintendo taxonomy ids. Everything the Wii U build adds
//! is data the PC engine already understands, so porting is purely a data job:
//!
//! 1. **New resources** (the ~372 resources whose `1s` symbol the PC build lacks: `.so`,
//!    `.sa`, `.vec`, `.anim`, `.wav`) are appended after the PC build's last resource index.
//! 2. **Replaced resources** ([`REPLACED`]): shared resources whose Wii U version only adds
//!    Nintendo-specific parts; the Wii U version replaces the PC one.
//! 3. **Merged resources**: the 9 dictionaries the engine reads gain the Nintendo words (and
//!    extra meanings on shared words such as `PEACH`), `scribbleobject.odt` gains the entries
//!    of the new objects, and `audiometadata.aaf` fills its empty slots.
//!
//! Resources move between builds by decoding with the Wii U names and encoding with the PC
//! names (resource references decode to logical paths, so they are renumbered on the way).
//! [`transcode`] then checks, on raw decodes, that nothing but resource references changed.
//!
//! The install never touches the shipped packs: everything goes into [`PORT_PACK`], and
//! `index.bin`, `pmindex.xml`, `pmindex_for_code.xml` and `1s` are rewritten after saving the
//! originals in [`BACKUP_DIR`]. The engine opens every pack listed in `index.bin`
//! (`FUN_004944a0`) and never reads `pmindex*.xml` or `1s`, which are kept in step for tools.

use crate::emulation;
use crate::wiiu::{chunky_by_extension, made_up_guid, WiiuGame};
use flate2::{write::ZlibEncoder, Compression};
use fmt_dictionary::{Dictionary, Sense, WordKind, LANGUAGES, LEGACY_LANGUAGES};
use fmt_effects::AudioMetadata;
use fmt_object::ObjectDetailsTable;
use scribble_core::{bail, ensure, Context, Format, ResRef, Result, ResultExt as _};
use scribble_formats::{handler_for, Handler};
use scribble_pack::index::{IndexEntry, FLAG_ZLIB, NO_PACK};
use scribble_pack::manifest::{GameReader, ManifestFile, PACK_HEADER_SIZE};
use scribble_pack::pmindex::{PmFile, PmIndex};
use scribble_pack::{symbols, IndexBin};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The pack holding everything the port adds or replaces.
pub const PORT_PACK: &str = "wiiu_content.p";
/// Where the original index files are kept while the port is installed.
pub const BACKUP_DIR: &str = "wiiu_port_backup";
/// The index files the install rewrites.
pub const INDEX_FILES: [&str; 4] = ["index.bin", "pmindex.xml", "pmindex_for_code.xml", "1s"];

/// Shared resources replaced by their Wii U version. Each Wii U version equals the PC one plus
/// Nintendo-specific parts (checked against the shipped files: the PC effects are a prefix or
/// subsequence of the Wii U ones, and the other fields match except the tool-computed
/// `budget_cost`).
pub const REPLACED: [&str; 6] = [
    // Super star (applied by easteregg/nintendo/object/superstar): an empty stub on PC.
    "data\\_game\\scribbleadjectives\\_lm7.sa",
    // Super mushroom (applied by .../supermushroom): an empty stub on PC.
    "data\\_game\\scribbleadjectives\\_lm8.sa",
    // + the growth effect for objects with _lm8/_lm9.
    "data\\_game\\scribbleadjectives\\super.sa",
    // + the squashed goomba.
    "data\\_game\\scribbleadjectives\\_dead.sa",
    // + chickens attack Link.
    "data\\_game\\scribbleadjectives\\_voodoo.sa",
    // The hook at the end of the hookshot's rope: its art is missing on PC.
    "data\\_game\\scribbleobjects\\tool_rope_pieces__grapplinghookend.so",
];
const OBJECT_DETAILS: &str = "data\\_game\\metadata\\scribbleobject.odt";
const AUDIO_METADATA: &str = "[platform]\\audio\\sfx\\audiometadata.aaf";

/// Values the engine treats as particle-stream ids rather than resource indices where an
/// object is expected (`FUN_00474f60`'s cases): a transcoded file must never renumber them.
pub const STREAM_IDS: [u32; 21] = [
    0xb34, 0xb35, 0xb3d, 0xb3f, 0xb46, 0xb47, 0x13b8, 0x13ba, 0x13bc, 0x13bd, 0x13c0, 0x13c1, 0x14f4, 0x14f5, 0x14f6, 0x14f7, 0x14f8,
    0x14f9, 0x14fa, 0x14fb, 0x14fc,
];
/// Resource indices with bit 15 set mean user-made objects (`FUN_0068b6c0`).
const MAX_INDEX: u32 = 0x8000;

/// Install choices.
#[derive(Clone, Debug)]
pub struct Options {
    /// Add the data stand-ins for the Wii U-only item physics ([`crate::emulation`]).
    pub emulate_engine: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { emulate_engine: true }
    }
}

/// What an install did.
#[derive(Debug, Default)]
pub struct Summary {
    /// New resources by extension.
    pub added: BTreeMap<String, usize>,
    pub replaced: Vec<String>,
    /// Words added (or given Nintendo meanings) per dictionary language.
    pub words: BTreeMap<String, usize>,
    pub object_details: usize,
    pub sounds: usize,
    /// Objects given data stand-ins for Wii U-only engine code.
    pub emulated: Vec<String>,
    pub pack_bytes: u64,
}

/// The PC install's original index files and a reader over its packs.
struct PcGame {
    reader: GameReader,
    pm: PmIndex,
    pm_code: PmIndex,
    symbols: Vec<(String, u32)>,
    by_name: HashMap<String, u32>,
}

impl PcGame {
    /// Load the original index files: from the backup when the port is installed, else from
    /// the game directory.
    fn load(game: &Path) -> Result<Self> {
        ensure!(game.join("index.bin").is_file() && game.join("pmindex.xml").is_file(), "{} is not a Scribblenauts Unlimited PC install (no index.bin / pmindex.xml)", game.display());
        let src = if installed(game)? { game.join(BACKUP_DIR) } else { game.to_path_buf() };
        let read = |f: &str| fs::read(src.join(f)).with_context(|| format!("reading {}", src.join(f).display()));
        let index = IndexBin::read(&read("index.bin")?).context("reading the PC index.bin")?;
        ensure!(!index.packs.iter().any(|p| p == PORT_PACK), "{} already lists {PORT_PACK}", src.join("index.bin").display());
        // The port only appends to these, so entries past the original index.bin are leftovers
        // of an install whose index.bin was since restored (e.g. by Steam's file verification).
        let count = index.entries.len() as u32;
        let mut pm = PmIndex::read(&read("pmindex.xml")?)?;
        let mut pm_code = PmIndex::read(&read("pmindex_for_code.xml")?)?;
        let mut symbols = symbols::read(&read("1s")?)?;
        pm.files.retain(|f| f.index < count);
        pm_code.files.retain(|f| f.index < count);
        symbols.retain(|(_, i)| *i < count);
        let by_name = pm.files.iter().map(|f| (f.name.clone(), f.index)).collect();
        Ok(PcGame { reader: GameReader::with_index(game, index), pm, pm_code, symbols, by_name })
    }

    fn read_path(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
        match self.by_name.get(path) {
            Some(&i) if self.reader.index.entries[i as usize].pack != NO_PACK => self.reader.read(i).map(Some),
            _ => Ok(None),
        }
    }
}

/// Whether the port is currently installed in `game` (its `index.bin` lists [`PORT_PACK`]).
pub fn installed(game: &Path) -> Result<bool> {
    let index = IndexBin::read(&fs::read(game.join("index.bin"))?)?;
    let listed = index.packs.iter().any(|p| p == PORT_PACK);
    ensure!(!listed || game.join(BACKUP_DIR).join("index.bin").is_file(), "the port is installed but {} is missing; reinstall the game files", game.join(BACKUP_DIR).display());
    Ok(listed)
}

/// Install (or reinstall) the Wii U content from the dump at `wiiu` into the PC install at `game`.
pub fn install(game: &Path, wiiu: &Path, options: &Options, log: &mut dyn FnMut(&str)) -> Result<Summary> {
    let mut pc = PcGame::load(game)?;
    let pc_names: HashMap<u32, &str> = pc.pm.files.iter().map(|f| (f.index, f.name.as_str())).collect();
    let pc_paths: HashMap<String, String> = pc.symbols.iter().filter_map(|(s, i)| Some((s.clone(), pc_names.get(i)?.to_string()))).collect();
    let mut wu = WiiuGame::open(wiiu, &pc_paths)?;
    let wu_ctx = wu.context();
    let mut summary = Summary::default();

    // New resources, appended in Wii U index order.
    let first = pc.reader.index.entries.len() as u32;
    let new: Vec<_> = wu.resources.iter().filter(|r| !r.on_pc && wu.has_data(r.index)).cloned().collect();
    ensure!(!new.is_empty(), "the Wii U dump has no resources the PC build lacks: is it the Wii U build?");
    ensure!(first + (new.len() as u32) < MAX_INDEX, "{} new resources after index {first} would reach the user-object range", new.len());
    let mut pc_names: Vec<(u32, String)> = pc.pm.files.iter().map(|f| (f.index, f.name.clone())).collect();
    pc_names.extend(new.iter().enumerate().map(|(i, r)| (first + i as u32, r.path.clone())));
    let extra: &[&str] = if options.emulate_engine { &emulation::ADJECTIVES } else { &[] };
    pc_names.extend(extra.iter().enumerate().map(|(i, p)| (first + (new.len() + i) as u32, p.to_string())));
    let pc_ctx = Context::from_names(pc_names);
    let remap = |wiiu_index: u32| wu_ctx.name(wiiu_index).and_then(|p| pc_ctx.index(p));
    let new_paths: HashSet<&str> = new.iter().map(|r| r.path.as_str()).collect();
    log(&format!("Porting {} Wii U resources (PC indices {first}..{})", new.len(), first + new.len() as u32));

    let chunky = chunky_by_extension(
        &pc.pm.files.iter().map(|f| ManifestFile { index: f.index, path: f.name.clone(), pack: None, compressed: false, chunky: f.chunky, guid: String::new(), code_guid: String::new(), symbol: None }).collect::<Vec<_>>(),
    );
    let mut added = Vec::with_capacity(new.len());
    for (i, r) in new.iter().enumerate() {
        let data = wu.read(r.index).with_context(|| format!("reading {} from the Wii U packs", r.path))?;
        let out = transcode(&r.path, &data, &wu_ctx, &pc_ctx, &remap)?;
        *summary.added.entry(r.path.rsplit('.').next().unwrap_or("").to_string()).or_default() += 1;
        added.push(Added { index: first + i as u32, path: r.path.clone(), symbol: r.symbol.clone(), chunky: chunky(&r.path), data: out });
    }

    if options.emulate_engine {
        emulate(&mut pc, &mut added, &pc_ctx, first + new.len() as u32, &chunky)?;
        summary.emulated = vec![emulation::STAR.to_string(), emulation::FIREBALL.to_string()];
    }

    // Shared resources replaced by their Wii U version.
    let mut replaced: BTreeMap<u32, (String, Vec<u8>)> = BTreeMap::new();
    for path in REPLACED {
        let wi = wu_ctx.index(path).with_context(|| format!("the Wii U dump has no {path}"))?;
        let data = transcode(path, &wu.read(wi)?, &wu_ctx, &pc_ctx, &remap)?;
        if pc.read_path(path)?.as_deref() != Some(&data[..]) {
            replaced.insert(pc.by_name[path], (path.to_string(), data));
            summary.replaced.push(path.to_string());
        }
    }

    // Dictionaries.
    for lang in LANGUAGES.iter().filter(|l| !LEGACY_LANGUAGES.contains(l)) {
        let mut pd = Dictionary::load(lang, &pc_ctx, |p: &str| pc.read_path(p)).with_context(|| format!("loading the PC {lang} dictionary"))?;
        let wd = Dictionary::load(lang, &wu_ctx, |p: &str| wu_read_path(&mut wu, &wu_ctx, p)).with_context(|| format!("loading the Wii U {lang} dictionary"))?;
        let words = merge_dictionary(&mut pd, &wd, &new_paths).with_context(|| format!("merging the {lang} dictionary"))?;
        for (path, bytes) in pd.to_files(&pc_ctx)? {
            if pc.read_path(&path)?.as_deref() != Some(&bytes[..]) {
                let idx = *pc.by_name.get(&path).with_context(|| format!("the PC build has no {path}"))?;
                replaced.insert(idx, (path, bytes));
            }
        }
        summary.words.insert(lang.to_string(), words);
    }

    // Object details: the new objects' entries.
    {
        let mut pt = ObjectDetailsTable::decode(&pc.read_path(OBJECT_DETAILS)?.context("no PC object details")?, &pc_ctx)?;
        let wt = ObjectDetailsTable::decode(&wu_read_path(&mut wu, &wu_ctx, OBJECT_DETAILS)?.context("no Wii U object details")?, &wu_ctx)?;
        for e in wt.entries {
            if matches!(&e.object, ResRef::Path(p) if new_paths.contains(p.as_str())) {
                pt.entries.push(e);
                summary.object_details += 1;
            }
        }
        pt.entries.sort_by_key(|e| e.object.to_index(&pc_ctx).unwrap_or(u32::MAX));
        replaced.insert(pc.by_name[OBJECT_DETAILS], (OBJECT_DETAILS.to_string(), pt.encode(&pc_ctx)?));
    }

    // Sound settings: fill the slots the PC build emptied.
    {
        let mut pa = AudioMetadata::decode(&pc.read_path(AUDIO_METADATA)?.context("no PC audio metadata")?, &pc_ctx)?;
        let wa = AudioMetadata::decode(&wu_read_path(&mut wu, &wu_ctx, AUDIO_METADATA)?.context("no Wii U audio metadata")?, &wu_ctx)?;
        ensure!(pa.items.len() == wa.items.len(), "audio metadata has {} items on PC but {} on Wii U", pa.items.len(), wa.items.len());
        for (p, w) in pa.items.iter_mut().zip(wa.items) {
            if p.sound.is_none() && matches!(&w.sound, Some(ResRef::Path(s)) if new_paths.contains(s.as_str())) {
                let mut filled = w.clone();
                filled.sound = None;
                ensure!(*p == filled, "audio slot for {:?} has different settings on PC", w.sound);
                *p = w;
                summary.sounds += 1;
            }
        }
        replaced.insert(pc.by_name[AUDIO_METADATA], (AUDIO_METADATA.to_string(), pa.encode(&pc_ctx)?));
    }

    log(&format!("Writing {PORT_PACK}"));
    summary.pack_bytes = write_install(game, &pc, &added, &replaced)?;
    Ok(summary)
}

/// Remove the port: restore the original index files and delete [`PORT_PACK`].
pub fn uninstall(game: &Path) -> Result<bool> {
    if !installed(game)? {
        return Ok(false);
    }
    let backup = game.join(BACKUP_DIR);
    for f in INDEX_FILES {
        replace_file(&game.join(f), &fs::read(backup.join(f))?)?;
    }
    let pack = game.join(PORT_PACK);
    if pack.exists() {
        fs::remove_file(&pack)?;
    }
    fs::remove_dir_all(&backup)?;
    Ok(true)
}

/// Add [`emulation`]'s adjectives after the ported resources (from `index`) and patch the
/// ported star and fireball.
fn emulate(pc: &mut PcGame, added: &mut Vec<Added>, ctx: &Context, index: u32, chunky: &dyn Fn(&str) -> bool) -> Result<()> {
    let mut max_id = 0;
    let adjectives: Vec<u32> = pc.pm.files.iter().filter(|f| f.name.ends_with(".sa")).map(|f| f.index).collect();
    for i in adjectives {
        if pc.reader.index.entries[i as usize].pack != NO_PACK {
            max_id = max_id.max(emulation::adjective_id(&pc.reader.read(i)?).unwrap_or(0));
        }
    }
    for a in added.iter().filter(|a| a.path.ends_with(".sa")) {
        max_id = max_id.max(emulation::adjective_id(&a.data).unwrap_or(0));
    }
    let find = |added: &[Added], path: &str| added.iter().position(|a| a.path == path).with_context(|| format!("{path} was not ported"));
    let template = &added[find(added, emulation::TEMPLATE)?].data;
    let new = emulation::adjectives(template, max_id + 1, ctx)?;
    for (i, (path, data)) in emulation::ADJECTIVES.iter().zip(new).enumerate() {
        ensure!(added.last().map(|a| a.index + 1) == Some(index + i as u32), "emulation adjectives out of order");
        added.push(Added { index: index + i as u32, path: path.to_string(), symbol: symbols::derive(path), chunky: chunky(path), data });
    }
    for path in [emulation::STAR, emulation::FIREBALL] {
        let i = find(added, path)?;
        added[i].data = emulation::patch_object(path, &added[i].data, ctx)?;
    }
    Ok(())
}

fn wu_read_path(wu: &mut WiiuGame, ctx: &Context, path: &str) -> Result<Option<Vec<u8>>> {
    match ctx.index(path) {
        Some(i) if wu.has_data(i) => wu.read(i).map(Some),
        _ => Ok(None),
    }
}

/// A new resource: its PC index and bytes.
struct Added {
    index: u32,
    path: String,
    symbol: String,
    chunky: bool,
    data: Vec<u8>,
}

/// Re-encode one resource from the Wii U build for the PC build: decode with the Wii U names,
/// encode with the PC names. Standard formats (Bink audio, textures) are copied.
///
/// Checks the result on raw decodes (no names): the two must be identical except for resource
/// references, each renumbered to `remap` of its Wii U index, and no [`STREAM_IDS`] value may
/// change (they are engine constants, not indices, in some fields).
pub fn transcode(path: &str, data: &[u8], from: &Context, to: &Context, remap: &dyn Fn(u32) -> Option<u32>) -> Result<Vec<u8>> {
    let codec = match handler_for(path, data) {
        Some(Handler::Standard { .. }) => return Ok(data.to_vec()),
        Some(Handler::Codec(c)) => c,
        None => bail!("no codec for {path}"),
    };
    let text = codec.decode_text(data, from).with_context(|| format!("decoding the Wii U {path}"))?;
    let out = codec.encode_text(&text, to).with_context(|| format!("encoding {path} for PC"))?;
    let raw = |platform| {
        let mut c = Context::empty();
        c.set_platform(platform);
        c
    };
    let a = codec.decode_json(data, &raw(from.platform()))?;
    let b = codec.decode_json(&out, &raw(to.platform()))?;
    check_renumbered(&a, &b, remap, "").with_context(|| format!("checking the transcoded {path}"))?;
    Ok(out)
}

fn check_renumbered(a: &Value, b: &Value, remap: &dyn Fn(u32) -> Option<u32>, at: &str) -> Result<()> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            ensure!(x.len() == y.len() && x.keys().all(|k| y.contains_key(k)), "fields differ at {at:?}");
            for (k, v) in x {
                check_renumbered(v, &y[k], remap, &format!("{at}.{k}"))?;
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            ensure!(x.len() == y.len(), "list lengths differ at {at:?}");
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                check_renumbered(v, w, remap, &format!("{at}[{i}]"))?;
            }
        }
        (Value::Number(x), Value::Number(y)) if x != y => {
            let (v, w) = (x.as_u64().context("non-integer changed")? as u32, y.as_u64().context("non-integer changed")? as u32);
            ensure!(!STREAM_IDS.contains(&v), "{at}: stream id {v:#x} would be renumbered to {w:#x}");
            ensure!(remap(v) == Some(w), "{at}: {v} became {w}, which is not the PC index of Wii U resource {v}");
        }
        _ => ensure!(a == b, "values differ at {at:?}: {a} vs {b}"),
    }
    Ok(())
}

/// Add the words and targets of `wiiu` that name new resources to `pc`, plus the words the Wii U
/// build added for existing objects (`FOOD PEACH`, `MONEY COIN`: unambiguous spellings of the
/// meanings that now share a word with a Nintendo one). Targets of [`REPLACED`] resources take
/// their Wii U properties (the budget cost follows the new adjective). Returns the number of
/// words added or extended.
fn merge_dictionary(pc: &mut Dictionary, wiiu: &Dictionary, new_paths: &HashSet<&str>) -> Result<usize> {
    let is_new = |t: &fmt_dictionary::Target| matches!(&t.properties.resource, Some(ResRef::Path(p)) if new_paths.contains(p.as_str()));
    let is_replaced = |t: &fmt_dictionary::Target| matches!(&t.properties.resource, Some(ResRef::Path(p)) if REPLACED.contains(&p.as_str()));
    let mut ported: HashMap<WordKind, BTreeSet<u16>> = HashMap::new();
    for kind in [WordKind::Object, WordKind::Adjective] {
        let (Some(wt), Some(pt)) = (wiiu.targets(kind), pc.targets(kind)) else { continue };
        let adds: Vec<_> = wt.iter().filter(|(_, t)| is_new(t)).map(|(id, t)| (*id, t.clone())).collect();
        let updates: Vec<_> = wt
            .iter()
            .filter(|(id, t)| is_replaced(t) && pt.get(id).is_some_and(|p| p.properties.resource == t.properties.resource))
            .map(|(id, t)| (*id, t.properties.clone()))
            .collect();
        for (id, properties) in updates {
            pc.targets_mut(kind).unwrap().get_mut(&id).unwrap().properties = properties;
        }
        let existing: BTreeMap<u16, _> = pc.targets(kind).unwrap().clone();
        for (id, t) in adds {
            match existing.get(&id) {
                // The PC build keeps a stale target for some stripped ids: point it at the port.
                Some(p) if p.properties.resource.is_some() && p.properties != t.properties => {
                    bail!("{kind:?} id {id} names {:?} on PC but {:?} on Wii U", p.properties.resource, t.properties.resource)
                }
                Some(_) => {
                    pc.targets_mut(kind).unwrap().get_mut(&id).unwrap().properties = t.properties.clone();
                }
                None => pc.add_target(kind, id, t.properties.clone())?,
            }
            ported.entry(kind).or_default().insert(id);
        }
    }
    let mut count = 0;
    // Existing meanings of words that gain a Nintendo meaning (`PEACH` the fruit).
    let mut shared: HashSet<(WordKind, u16)> = HashSet::new();
    for e in &wiiu.words {
        let Some(ids) = ported.get(&e.kind) else { continue };
        let senses: Vec<Sense> = e.meanings.iter().filter(|s| ids.contains(&s.id)).cloned().collect();
        if senses.is_empty() {
            continue;
        }
        match pc.find(&e.word, e.kind).cloned() {
            None => {
                pc.add_word(&e.word, e.kind, senses)?;
                count += 1;
            }
            // The Wii U build reassigned the word rather than adding a meaning (english_uk maps
            // hidden adjectives to stray words such as `PIPELINE`): leave it.
            Some(existing) if !existing.meanings.iter().all(|m| e.meanings.iter().any(|s| s.id == m.id)) => {}
            Some(existing) => {
                count += 1;
                // Keep the PC meanings (labelled as on Wii U, where the word now has a choice)
                // and append the Nintendo ones.
                shared.extend(existing.meanings.iter().map(|s| (e.kind, s.id)));
                let label = |id: u16| e.meanings.iter().find(|s| s.id == id).and_then(|s| s.label.clone());
                let mut meanings: Vec<Sense> =
                    existing.meanings.iter().map(|s| Sense { id: s.id, label: s.label.clone().or_else(|| label(s.id)) }).collect();
                for s in senses {
                    if !meanings.iter().any(|m| m.id == s.id) {
                        meanings.push(s);
                    }
                }
                pc.set_meanings(&e.word, e.kind, meanings)?;
            }
        }
    }
    // Words naming a replaced adjective follow the Wii U build, which moved the ones that would
    // now do something unintended (english_uk `BAND` named `_lm7`, the super star, on PC).
    let replaced_ids: HashSet<(WordKind, u16)> = [WordKind::Object, WordKind::Adjective]
        .into_iter()
        .flat_map(|k| pc.targets(k).into_iter().flatten().filter(|(_, t)| is_replaced(t)).map(move |(id, _)| (k, *id)))
        .collect();
    for e in &wiiu.words {
        let Some(p) = pc.find(&e.word, e.kind) else { continue };
        let known = |id: u16| pc.targets(e.kind).is_some_and(|t| t.contains_key(&id));
        if p.meanings != e.meanings && p.meanings.iter().any(|s| replaced_ids.contains(&(e.kind, s.id))) && e.meanings.iter().all(|s| known(s.id)) {
            pc.set_meanings(&e.word, e.kind, e.meanings.clone())?;
            count += 1;
        }
    }
    // The Wii U build's unambiguous spellings of those meanings (`FOOD PEACH`). Other words
    // only one build has are localisation revisions unrelated to the port.
    for e in &wiiu.words {
        if pc.find(&e.word, e.kind).is_none() && e.meanings.iter().all(|s| shared.contains(&(e.kind, s.id))) {
            pc.add_word(&e.word, e.kind, e.meanings.clone())?;
            count += 1;
        }
    }
    // Display names of the new targets, as on Wii U.
    for (kind, ids) in &ported {
        for id in ids {
            let name = wiiu.targets(*kind).and_then(|t| t.get(id)).and_then(|t| t.name.clone());
            pc.targets_mut(*kind).unwrap().get_mut(id).unwrap().name = name;
        }
    }
    Ok(count)
}

/// Write [`PORT_PACK`] and the updated index files, saving the originals first. Returns the
/// pack size.
fn write_install(game: &Path, pc: &PcGame, added: &[Added], replaced: &BTreeMap<u32, (String, Vec<u8>)>) -> Result<u64> {
    let backup = game.join(BACKUP_DIR);
    if !installed(game)? {
        // Fresh install (or the game files were reset since): save the originals as loaded.
        fs::create_dir_all(&backup)?;
        replace_file(&backup.join("index.bin"), &pc.reader.index.write()?)?;
        replace_file(&backup.join("pmindex.xml"), &pc.pm.write())?;
        replace_file(&backup.join("pmindex_for_code.xml"), &pc.pm_code.write())?;
        replace_file(&backup.join("1s"), &symbols::write(&pc.symbols)?)?;
    }

    let mut index = pc.reader.index.clone();
    let pack_id = u8::try_from(index.packs.len())?;
    index.packs.push(PORT_PACK.to_string());
    let mut pack = vec![0u8; PACK_HEADER_SIZE];
    let mut store = |data: &[u8]| -> Result<IndexEntry> {
        let mut z = ZlibEncoder::new(Vec::new(), Compression::best());
        z.write_all(data)?;
        let z = z.finish()?;
        let e = IndexEntry { pack: pack_id, flags: FLAG_ZLIB, offset: pack.len() as u32, stored_size: z.len() as u32, raw_size: data.len() as u32 };
        pack.extend_from_slice(&z);
        Ok(e)
    };
    let (mut pm, mut pm_code, mut syms) = (pc.pm.clone(), pc.pm_code.clone(), pc.symbols.clone());
    for a in added {
        ensure!(a.index as usize == index.entries.len(), "new resource {} is out of order", a.path);
        index.entries.push(store(&a.data)?);
        let guid = made_up_guid(&a.path);
        pm.files.push(PmFile { name: a.path.clone(), index: a.index, guid: guid.clone(), chunky: a.chunky });
        pm_code.files.push(PmFile { name: a.path.clone(), index: a.index, guid, chunky: a.chunky });
        syms.push((a.symbol.clone(), a.index));
    }
    for (&i, (_, data)) in replaced {
        index.entries[i as usize] = store(data)?;
    }
    ensure!(pack.len() <= u32::MAX as usize, "{PORT_PACK} exceeds 4 GiB");
    let size = pack.len() as u32;
    pack[..4].copy_from_slice(&size.to_le_bytes());

    replace_file(&game.join(PORT_PACK), &pack)?;
    replace_file(&game.join("pmindex.xml"), &pm.write())?;
    replace_file(&game.join("pmindex_for_code.xml"), &pm_code.write())?;
    replace_file(&game.join("1s"), &symbols::write(&syms)?)?;
    // Last, so an interrupted install leaves the game on its original index.
    replace_file(&game.join("index.bin"), &index.write()?)?;
    Ok(pack.len() as u64)
}

/// Write via a temporary file and rename, so a failure never leaves a half-written file.
fn replace_file(path: &Path, data: &[u8]) -> Result<()> {
    let tmp: PathBuf = path.with_extension("wiiu_port_tmp");
    fs::write(&tmp, data).with_context(|| format!("writing {} (is the game running?)", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {} (is the game running?)", path.display()))?;
    Ok(())
}
