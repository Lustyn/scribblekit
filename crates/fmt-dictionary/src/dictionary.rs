//! High-level, editable model of one language's dictionary, spanning all the files that
//! describe it, plus [`RelatedObjects`] and [`LegacyAdjectiveDictionary`].
//!
//! ```text
//! data\_game\scribbleobjects\d\
//!   object_dictionary_<lang>        words (objects + adjectives)         -> Dictionary::words
//!   object_word_id_table_<lang>     object word id -> record              (derived)
//!   object_jumptable_<lang>         object id -> display-name record       -> Target::name
//!   adjective_word_id_table_<lang>  adjective word id -> record           (derived)
//!   adjective_jumptable_<lang>      adjective id -> display-name record    -> Target::name
//!   tag_dictionary_<lang>           tag words                              -> TagDictionary::words
//!   tag_word_id_table_<lang>        tag word id -> record                 (derived)
//!   tag_jumptable_<lang>            tag id -> display-name record          -> Target::name
//!   object_single_word_<lang>       tokens of all words                   (derived)
//!   details_file_<lang>             labels of multi-meaning words          -> Sense::label
//!   <lang>.dtm                      level word keys -> word ids            -> Dictionary::translations
//! ```
//!
//! [`Dictionary::load`] reads them into [`Dictionary`]; [`Dictionary::to_files`] regenerates
//! every one of them consistently (byte-identical for unmodified shipped data). Word ids are
//! positions in the sorted word list, so adding or removing a word renumbers the words after it;
//! the model therefore stores references by word text and recomputes every id on write.
//!
//! **Lookup in the engine** (`FUN_00741d10`): leading and trailing spaces / `?` / `'` are
//! trimmed, and those characters are dropped after the first two characters of both the typed
//! text and each record. With the fuzzy flag (5th argument) accents are folded on both sides,
//! upper and lower case alike: `À`-`Æ`→`A`, `È`-`Ë`→`E`, `Ì`-`Ï`→`I`, `Ò`-`Ö` `Ø` `Œ`→`O`,
//! `Ù`-`Ü`→`U`, `Ç`→`C`, `Ñ`→`N`, `Š`→`S`, `Ž`→`Z`, `ß`→`B`, and the bytes `0xB5` `0xB6`
//! `0xB7` `0xB8` `0xB9` `0xBA` → `A` `E` `I` `O` `U` `O`. In the dictionaries those six bytes are
//! the game font's macron vowels `Ā Ē Ī Ō Ū` (`BµMUK¹HEN` = BĀMUKŪHEN, `GY¸ZA` = GYŌZA,
//! `N¶N¶` = NĒNĒ); the JSON keeps the raw Windows-1252 characters (`µ ¶ · ¸ ¹ º`) so the round
//! trip stays byte-exact. The first two characters index the prefix table of the dictionary
//! for the requested kind (1 object, 2 adjective, 4 tag, 8 single-word; the fuzzy search starts
//! at `prefix[i][0]`); records are then scanned while the first two letters still match,
//! comparing normalized text. A hit yields the word id, and for every meaning the resource, id,
//! cost, cost multiplier, `flags & 1` and `flags >> 1 & 3`. Words with several meanings show a
//! choice built from `details_file`.

use crate::tables::{Choice, DetailEntry, Details, OffsetTable, RelatedObjectsLists, Translation, TranslationMap, offsets_to_bytes};
use crate::wordfile::{self, Layout, Properties, RawMeaning, RawRecord, WordKind};
use crate::words::{KindSet, SingleWord, SingleWordIndex};
use scribble_core::{Context, Format, Result, ResultExt, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Directory of the live dictionary files.
pub const DICTIONARY_DIR: &str = "data\\_game\\scribbleobjects\\d\\";
/// Directory of the legacy adjective dictionaries (not referenced by the shipped engine).
pub const LEGACY_ADJECTIVE_DIR: &str = "data\\_game\\scribbleadjectives\\dictionary\\";

/// Languages the shipped engine can select, in the order of its language setting
/// (`*(DAT_008a82fc + 4)`, see `FUN_0073c430`).
pub const ENGINE_LANGUAGES: [&str; 7] = ["english", "dutch", "french", "german", "italian", "spanish_mexico", "portuguese_brazil"];
/// Every language with an `object_dictionary_<lang>`.
pub const LANGUAGES: [&str; 13] = [
    "danish",
    "dutch",
    "english",
    "english_uk",
    "finnish",
    "french",
    "german",
    "italian",
    "norwegian",
    "portuguese_brazil",
    "spanish",
    "spanish_mexico",
    "swedish",
];
/// Languages whose object dictionary uses the legacy layout with stale resource indices.
pub const LEGACY_LANGUAGES: [&str; 4] = ["danish", "finnish", "norwegian", "swedish"];

/// One word of a dictionary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Upper-case Windows-1252 text (see [`normalize_word`]).
    pub word: String,
    pub kind: WordKind,
    /// The ids this word names (object ids for objects, adjective ids for adjectives, tag ids for
    /// tags), each with its choice label. Properties of each id live in the target maps.
    pub meanings: Vec<Sense>,
}

/// One meaning of a word.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sense {
    pub id: u16,
    /// Label shown in the choice menu when the word has more than one meaning
    /// (`details_file_<lang>`). Ignored for single-meaning words; missing labels are written as
    /// empty strings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Sense {
    pub fn new(id: u16) -> Self {
        Sense { id, label: None }
    }
}

/// An object, adjective or tag id: what it spawns and its display name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// The word the game displays for this id: its jump-table entry, whose record's text
    /// `FUN_0073cd60` copies as the name. Must be one of the words naming the id; if missing or
    /// invalid, the first such word (in sorted order) is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub properties: Properties,
}

/// Tag words (`tag_dictionary_<lang>` and its tables).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TagDictionary {
    pub words: Vec<Entry>,
    pub tags: BTreeMap<u16, Target>,
    /// Number of entries in `tag_jumptable` (max tag id + 1 or more).
    pub tag_id_slots: u32,
}

/// A reference from a level word key to a word.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WordRef {
    /// The word with this text (of the table's kind).
    Word(String),
    /// A raw word id that did not resolve to a word.
    Id(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordLink {
    pub key: u16,
    pub word: WordRef,
}

/// `<lang>.dtm` with word ids resolved to words.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Translations {
    pub object_words: Vec<WordLink>,
    pub adjective_words: Vec<WordLink>,
    pub apply_adjectives_words: Vec<WordLink>,
}

/// Everything the engine knows about typeable words in one language.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dictionary {
    pub language: String,
    #[serde(default, skip_serializing_if = "Layout::is_current")]
    pub layout: Layout,
    /// Object and adjective words (`object_dictionary_<lang>`), kept sorted by bytes.
    pub words: Vec<Entry>,
    /// Object id -> target.
    pub objects: BTreeMap<u16, Target>,
    /// Adjective id -> target.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub adjectives: BTreeMap<u16, Target>,
    /// Entries in `object_jumptable` (grows automatically to max id + 1).
    pub object_id_slots: u32,
    /// Entries in `adjective_jumptable`; `None` if the language has no adjective tables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adjective_id_slots: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<TagDictionary>,
    /// Whether `object_single_word_<lang>` exists (it is regenerated from the words).
    #[serde(default)]
    pub single_word_index: bool,
    /// Entries of `details_file_<lang>` that do not correspond to a multi-meaning word (words
    /// with apostrophes that the dictionary spells without). `None` if there is no details file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_details: Option<Vec<DetailEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translations: Option<Translations>,
}

/// Upper-case a word the way the game expects dictionary words (Windows-1252 aware: `é`→`É`,
/// `š`→`Š`, `œ`→`Œ`, `ÿ`→`Ÿ`), and drop apostrophes and question marks, which the lookup
/// ignores. `ß` has no single-byte upper case and is kept.
pub fn normalize_word(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|&c| c != '\'' && c != '?')
        .map(|c| match c as u32 {
            0x61..=0x7A => ((c as u8) - 0x20) as char,
            0xE0..=0xFE if c as u32 != 0xF7 => char::from_u32(c as u32 - 0x20).unwrap(),
            0x9A => '\u{8A}',
            0x9C => '\u{8C}',
            0x9E => '\u{8E}',
            0xFF => '\u{9F}',
            _ => c,
        })
        .collect()
}

fn path(dir: &str, file: &str) -> String {
    format!("{dir}{file}")
}

fn kind_bit(k: WordKind) -> u8 {
    k.to_u8()
}

/// Loaded word file: entries in file order, their offsets, targets per kind.
struct Loaded {
    layout: Layout,
    entries: Vec<Entry>,
    offsets: Vec<u32>,
    targets: BTreeMap<WordKind, BTreeMap<u16, Target>>,
}

fn load_words(data: &[u8], legacy_kind: WordKind, res_ctx: &Context, what: &str) -> Result<Loaded> {
    let file = wordfile::read(data).with_context(|| what.to_string())?;
    let seq = wordfile::sequential_ids(&file.records);
    let mut entries = Vec::with_capacity(file.records.len());
    let mut targets: BTreeMap<WordKind, BTreeMap<u16, Target>> = BTreeMap::new();
    for (r, s) in file.records.iter().zip(seq) {
        let word = wordfile::text(&r.word);
        ensure!(r.word_id == s, "{what}: word {word:?} has non-sequential word id {}", r.word_id);
        let kind = r.kind.map(WordKind::from_u8).unwrap_or(legacy_kind);
        let map = targets.entry(kind).or_default();
        for m in &r.meanings {
            let p = m.properties(res_ctx);
            match map.get(&m.id) {
                Some(t) => ensure!(t.properties == p, "{what}: id {} has different properties in {word:?}", m.id),
                None => {
                    map.insert(m.id, Target { name: None, properties: p });
                }
            }
        }
        entries.push(Entry { word, kind, meanings: r.meanings.iter().map(|m| Sense::new(m.id)).collect() });
    }
    Ok(Loaded { layout: file.layout, entries, offsets: file.offsets, targets })
}

impl Loaded {
    fn index_of_offset(&self) -> HashMap<u32, usize> {
        self.offsets.iter().enumerate().map(|(i, &o)| (o, i)).collect()
    }

    /// Check a word id table against the derived one.
    fn check_word_ids(&self, table: &[u8], kind: Option<WordKind>, what: &str) -> Result<()> {
        let t = OffsetTable::decode(table, &Context::empty())?.offsets;
        let derived: Vec<u32> =
            self.entries.iter().zip(&self.offsets).filter(|(e, _)| kind.is_none_or(|k| e.kind == k)).map(|(_, &o)| o).collect();
        ensure!(t == derived, "{what}: word id table is not the derived one");
        Ok(())
    }

    /// Read display names from a jump table; returns the slot count.
    fn apply_names(&mut self, table: &[u8], kind: WordKind, what: &str) -> Result<u32> {
        let t = OffsetTable::decode(table, &Context::empty())?.offsets;
        let at = self.index_of_offset();
        let map = self.targets.entry(kind).or_default();
        for (id, &off) in t.iter().enumerate() {
            if off == 0 {
                continue;
            }
            let id = u16::try_from(id).map_err(|_| anyhow!("{what}: jump table longer than 65536 entries"))?;
            let e = at.get(&off).map(|&i| &self.entries[i]).ok_or_else(|| anyhow!("{what}: entry {id} points into a record"))?;
            ensure!(e.kind == kind && e.meanings.iter().any(|m| m.id == id), "{what}: entry {id} points at {:?}, which does not name it", e.word);
            map.get_mut(&id).ok_or_else(|| anyhow!("{what}: id {id} has a name but no word"))?.name = Some(e.word.clone());
        }
        Ok(t.len() as u32)
    }
}

/// Result of writing a word file.
struct Built {
    bytes: Vec<u8>,
    /// Entries in written (sorted) order.
    order: Vec<usize>,
    /// Offset of each written record, parallel to `order`.
    offsets: Vec<u32>,
}

fn sorted_order(entries: &[Entry]) -> Result<Vec<usize>> {
    let keys: Vec<Vec<u8>> = entries.iter().map(|e| wordfile::bytes(&e.word)).collect::<Result<_>>()?;
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
    let mut seen = std::collections::HashSet::new();
    for &i in &order {
        ensure!(seen.insert((&keys[i], entries[i].kind)), "duplicate {:?} word {:?}", entries[i].kind, entries[i].word);
    }
    Ok(order)
}

fn build_words<'t>(
    layout: Layout,
    entries: &[Entry],
    targets: &dyn Fn(WordKind) -> Option<&'t BTreeMap<u16, Target>>,
    res_ctx: &Context,
) -> Result<Built> {
    let order = sorted_order(entries)?;
    let mut records = Vec::with_capacity(order.len());
    for &i in &order {
        let e = &entries[i];
        let map = targets(e.kind).ok_or_else(|| anyhow!("word {:?}: {:?} words are not allowed here", e.word, e.kind))?;
        let meanings = e
            .meanings
            .iter()
            .map(|s| {
                let t = map.get(&s.id).ok_or_else(|| anyhow!("word {:?}: unknown {:?} id {}", e.word, e.kind, s.id))?;
                RawMeaning::from_properties(s.id, &t.properties, layout, res_ctx)
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(meanings.len() < 256, "word {:?} has too many meanings", e.word);
        records.push(RawRecord {
            kind: layout.is_current().then(|| kind_bit(e.kind)),
            word: wordfile::bytes(&e.word)?,
            word_id: 0,
            meanings,
        });
    }
    let seq = wordfile::sequential_ids(&records);
    for (r, s) in records.iter_mut().zip(seq) {
        r.word_id = s;
    }
    let (bytes, offsets) = wordfile::write(layout, &records)?;
    Ok(Built { bytes, order, offsets })
}

impl Built {
    fn word_id_table(&self, entries: &[Entry], kind: Option<WordKind>) -> Vec<u8> {
        let v: Vec<u32> = self.order.iter().zip(&self.offsets).filter(|(i, _)| kind.is_none_or(|k| entries[**i].kind == k)).map(|(_, &o)| o).collect();
        offsets_to_bytes(&v)
    }

    fn jump_table(&self, entries: &[Entry], kind: WordKind, targets: &BTreeMap<u16, Target>, slots: u32) -> Vec<u8> {
        let mut holders: BTreeMap<u16, Vec<(usize, u32)>> = BTreeMap::new();
        for (&i, &off) in self.order.iter().zip(&self.offsets) {
            if entries[i].kind != kind {
                continue;
            }
            for s in &entries[i].meanings {
                let h = holders.entry(s.id).or_default();
                if !h.iter().any(|&(j, _)| j == i) {
                    h.push((i, off));
                }
            }
        }
        let n = holders.keys().next_back().map_or(0, |&m| m as u32 + 1).max(slots);
        let mut t = vec![0u32; n as usize];
        for (id, h) in &holders {
            let named = targets.get(id).and_then(|t| t.name.as_deref()).and_then(|n| h.iter().find(|&&(i, _)| entries[i].word == n));
            t[*id as usize] = named.unwrap_or(&h[0]).1;
        }
        offsets_to_bytes(&t)
    }

    /// Word id -> text for words of `kind` (all words if `None`).
    fn word_ids(&self, entries: &[Entry], kind: Option<WordKind>) -> Vec<String> {
        self.order.iter().filter(|&&i| kind.is_none_or(|k| entries[i].kind == k)).map(|&i| entries[i].word.clone()).collect()
    }
}

fn resolve_links(t: &[Translation], words: &[String]) -> Vec<WordLink> {
    t.iter()
        .map(|e| WordLink {
            key: e.key,
            word: match words.get(e.word_id as usize) {
                Some(w) => WordRef::Word(w.clone()),
                None => WordRef::Id(e.word_id),
            },
        })
        .collect()
}

fn unresolve_links(t: &[WordLink], words: &[String], what: &str) -> Result<Vec<Translation>> {
    let index: HashMap<&str, u32> = words.iter().enumerate().map(|(i, w)| (w.as_str(), i as u32)).collect();
    t.iter()
        .map(|l| {
            Ok(Translation {
                key: l.key,
                word_id: match &l.word {
                    WordRef::Id(i) => *i,
                    WordRef::Word(w) => {
                        *index.get(w.as_str()).ok_or_else(|| anyhow!("{what} key {} refers to missing word {w:?}", l.key))?
                    }
                },
            })
        })
        .collect()
}

/// A reader of logical resource paths (`None` = file does not exist).
pub trait ResourceSource {
    fn read(&mut self, logical: &str) -> Result<Option<Vec<u8>>>;
}

impl<F: FnMut(&str) -> Result<Option<Vec<u8>>>> ResourceSource for F {
    fn read(&mut self, logical: &str) -> Result<Option<Vec<u8>>> {
        self(logical)
    }
}

/// Reads logical paths from an unpacked tree (`extracted/`, where `a\b\c` is `a/b/c`).
pub fn dir_source(root: &Path) -> impl FnMut(&str) -> Result<Option<Vec<u8>>> + '_ {
    move |logical: &str| {
        let p = logical.split('\\').fold(root.to_path_buf(), |p, c| p.join(c));
        match std::fs::read(&p) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(anyhow!("reading {}: {e}", p.display())),
        }
    }
}

/// Write `(logical path, bytes)` pairs into an unpacked tree.
pub fn write_files(root: &Path, files: &[(String, Vec<u8>)]) -> Result<()> {
    for (logical, bytes) in files {
        let p = logical.split('\\').fold(root.to_path_buf(), |p, c| p.join(c));
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, bytes).with_context(|| format!("writing {}", p.display()))?;
    }
    Ok(())
}

fn require(src: &mut dyn ResourceSource, p: &str) -> Result<Vec<u8>> {
    src.read(p)?.ok_or_else(|| anyhow!("missing {p}"))
}

impl Dictionary {
    /// Load one language's dictionary from its files. `ctx` resolves resource indices (it is not
    /// used for the stale indices of legacy-layout dictionaries).
    pub fn load(language: &str, ctx: &Context, mut src: impl ResourceSource) -> Result<Dictionary> {
        let src: &mut dyn ResourceSource = &mut src;
        let d = DICTIONARY_DIR;
        let empty = Context::empty();
        let dict_path = path(d, &format!("object_dictionary_{language}"));
        let raw = require(src, &dict_path)?;
        let res_ctx = if wordfile::read(&raw)?.layout == Layout::Legacy { &empty } else { ctx };
        let mut l = load_words(&raw, WordKind::Object, res_ctx, &dict_path)?;
        let layout = l.layout;

        let current = layout.is_current();
        let owt = require(src, &path(d, &format!("object_word_id_table_{language}")))?;
        l.check_word_ids(&owt, current.then_some(WordKind::Object), "object_word_id_table")?;
        let ojt = require(src, &path(d, &format!("object_jumptable_{language}")))?;
        let object_id_slots = l.apply_names(&ojt, WordKind::Object, "object_jumptable")?;

        let mut adjective_id_slots = None;
        if let Some(ajt) = src.read(&path(d, &format!("adjective_jumptable_{language}")))? {
            ensure!(current, "adjective tables for a legacy-layout dictionary");
            let awt = require(src, &path(d, &format!("adjective_word_id_table_{language}")))?;
            l.check_word_ids(&awt, Some(WordKind::Adjective), "adjective_word_id_table")?;
            adjective_id_slots = Some(l.apply_names(&ajt, WordKind::Adjective, "adjective_jumptable")?);
        }
        for k in l.targets.keys() {
            ensure!(matches!(k, WordKind::Object | WordKind::Adjective), "unexpected {k:?} words in {dict_path}");
        }

        let tags = match src.read(&path(d, &format!("tag_dictionary_{language}")))? {
            None => None,
            Some(raw) => {
                let mut t = load_words(&raw, WordKind::Tag, ctx, "tag_dictionary")?;
                ensure!(t.targets.keys().all(|k| *k == WordKind::Tag), "tag dictionary has non-tag words");
                let twt = require(src, &path(d, &format!("tag_word_id_table_{language}")))?;
                t.check_word_ids(&twt, None, "tag_word_id_table")?;
                let tjt = require(src, &path(d, &format!("tag_jumptable_{language}")))?;
                let tag_id_slots = t.apply_names(&tjt, WordKind::Tag, "tag_jumptable")?;
                Some(TagDictionary { words: t.entries, tags: t.targets.remove(&WordKind::Tag).unwrap_or_default(), tag_id_slots })
            }
        };

        let mut dict = Dictionary {
            language: language.to_string(),
            layout,
            objects: l.targets.remove(&WordKind::Object).unwrap_or_default(),
            adjectives: l.targets.remove(&WordKind::Adjective).unwrap_or_default(),
            words: l.entries,
            object_id_slots,
            adjective_id_slots,
            tags,
            single_word_index: false,
            extra_details: None,
            translations: None,
        };

        if let Some(raw) = src.read(&path(d, &format!("object_single_word_{language}")))? {
            let have = SingleWordIndex::decode(&raw, ctx)?;
            ensure!(have == dict.single_word_list()?, "object_single_word_{language} is not derivable from the word lists");
            dict.single_word_index = true;
        }

        if let Some(raw) = src.read(&path(d, &format!("details_file_{language}")))? {
            let details = Details::decode(&raw, ctx)?;
            let mut by_word: HashMap<&str, Vec<usize>> = HashMap::new();
            for (i, e) in dict.words.iter().enumerate() {
                if e.meanings.len() > 1 {
                    by_word.entry(e.word.as_str()).or_default().push(i);
                }
            }
            let mut assign = Vec::new();
            let mut extra = Vec::new();
            for de in details.entries {
                let hit = by_word.get(de.word.as_str()).and_then(|v| {
                    v.iter().copied().find(|&i| {
                        let m = &dict.words[i].meanings;
                        m.len() == de.choices.len() && m.iter().zip(&de.choices).all(|(s, c)| s.id == c.id)
                    })
                });
                match hit {
                    Some(i) => assign.push((i, de)),
                    None => extra.push(de),
                }
            }
            for (i, de) in assign {
                for (s, c) in dict.words[i].meanings.iter_mut().zip(de.choices) {
                    s.label = Some(c.label);
                }
            }
            dict.extra_details = Some(extra);
        }

        if let Some(raw) = src.read(&path(d, &format!("{language}.dtm")))? {
            let dtm = TranslationMap::decode(&raw, ctx)?;
            let objs = dict.word_ids(WordKind::Object);
            let adjs = dict.word_ids(WordKind::Adjective);
            dict.translations = Some(Translations {
                object_words: resolve_links(&dtm.object_words, &objs),
                adjective_words: resolve_links(&dtm.adjective_words, &adjs),
                apply_adjectives_words: resolve_links(&dtm.apply_adjectives_words, &adjs),
            });
        }
        Ok(dict)
    }

    /// Load from an unpacked tree (the `extracted/` directory).
    pub fn load_dir(root: &Path, language: &str, ctx: &Context) -> Result<Dictionary> {
        Self::load(language, ctx, dir_source(root))
    }

    /// Word id -> word text for one kind, in the order the ids are assigned.
    pub fn word_ids(&self, kind: WordKind) -> Vec<String> {
        let current = self.layout.is_current();
        let order = sorted_order(&self.words).unwrap_or_else(|_| (0..self.words.len()).collect());
        order.into_iter().filter(|&i| !current || self.words[i].kind == kind).map(|i| self.words[i].word.clone()).collect()
    }

    fn single_word_list(&self) -> Result<SingleWordIndex> {
        let mut tokens: BTreeMap<Vec<u8>, u8> = BTreeMap::new();
        let tag_words = self.tags.as_ref().map_or(&[][..], |t| &t.words[..]);
        for e in self.words.iter().chain(tag_words) {
            for tok in wordfile::bytes(&e.word)?.split(|&c| c == b' ') {
                if !tok.is_empty() {
                    *tokens.entry(tok.to_vec()).or_default() |= kind_bit(e.kind);
                }
            }
        }
        Ok(SingleWordIndex { words: tokens.into_iter().map(|(w, k)| SingleWord { word: wordfile::text(&w), used_by: KindSet(k) }).collect() })
    }

    /// Regenerate every file of this dictionary as `(logical path, bytes)`.
    pub fn to_files(&self, ctx: &Context) -> Result<Vec<(String, Vec<u8>)>> {
        let d = DICTIONARY_DIR;
        let lang = &self.language;
        let empty = Context::empty();
        let current = self.layout.is_current();
        let res_ctx = if current { ctx } else { &empty };
        let mut out = Vec::new();

        let targets = |k: WordKind| match k {
            WordKind::Object => Some(&self.objects),
            WordKind::Adjective if current => Some(&self.adjectives),
            _ => None,
        };
        let built = build_words(self.layout, &self.words, &targets, res_ctx)?;
        out.push((path(d, &format!("object_dictionary_{lang}")), built.bytes.clone()));
        out.push((path(d, &format!("object_word_id_table_{lang}")), built.word_id_table(&self.words, current.then_some(WordKind::Object))));
        out.push((path(d, &format!("object_jumptable_{lang}")), built.jump_table(&self.words, WordKind::Object, &self.objects, self.object_id_slots)));
        if let Some(slots) = self.adjective_id_slots {
            out.push((path(d, &format!("adjective_word_id_table_{lang}")), built.word_id_table(&self.words, Some(WordKind::Adjective))));
            out.push((path(d, &format!("adjective_jumptable_{lang}")), built.jump_table(&self.words, WordKind::Adjective, &self.adjectives, slots)));
        }

        if let Some(t) = &self.tags {
            let tag_targets = |k: WordKind| (k == WordKind::Tag).then_some(&t.tags);
            let tb = build_words(Layout::Current, &t.words, &tag_targets, ctx)?;
            out.push((path(d, &format!("tag_dictionary_{lang}")), tb.bytes.clone()));
            out.push((path(d, &format!("tag_word_id_table_{lang}")), tb.word_id_table(&t.words, None)));
            out.push((path(d, &format!("tag_jumptable_{lang}")), tb.jump_table(&t.words, WordKind::Tag, &t.tags, t.tag_id_slots)));
        }

        if self.single_word_index {
            out.push((path(d, &format!("object_single_word_{lang}")), self.single_word_list()?.encode(ctx)?));
        }

        if let Some(extra) = &self.extra_details {
            let mut entries: Vec<DetailEntry> = built
                .order
                .iter()
                .map(|&i| &self.words[i])
                .filter(|e| e.meanings.len() > 1)
                .map(|e| DetailEntry {
                    word: e.word.clone(),
                    choices: e.meanings.iter().map(|s| Choice { id: s.id, label: s.label.clone().unwrap_or_default() }).collect(),
                })
                .collect();
            entries.extend(extra.iter().cloned());
            let keys: Vec<Vec<u8>> = entries.iter().map(|e| wordfile::bytes(&e.word)).collect::<Result<_>>()?;
            let mut idx: Vec<usize> = (0..entries.len()).collect();
            idx.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
            let entries = idx.into_iter().map(|i| entries[i].clone()).collect();
            out.push((path(d, &format!("details_file_{lang}")), Details { entries }.encode(ctx)?));
        }

        if let Some(t) = &self.translations {
            let objs = built.word_ids(&self.words, current.then_some(WordKind::Object));
            let adjs = built.word_ids(&self.words, Some(WordKind::Adjective));
            let dtm = TranslationMap {
                object_words: unresolve_links(&t.object_words, &objs, "object_words")?,
                adjective_words: unresolve_links(&t.adjective_words, &adjs, "adjective_words")?,
                apply_adjectives_words: unresolve_links(&t.apply_adjectives_words, &adjs, "apply_adjectives_words")?,
            };
            out.push((path(d, &format!("{lang}.dtm")), dtm.encode(ctx)?));
        }
        Ok(out)
    }

    /// Regenerate and write every file into an unpacked tree.
    pub fn write_dir(&self, root: &Path, ctx: &Context) -> Result<()> {
        write_files(root, &self.to_files(ctx)?)
    }

    fn list(&self, kind: WordKind) -> Result<&Vec<Entry>> {
        match kind {
            WordKind::Tag => Ok(&self.tags.as_ref().ok_or_else(|| anyhow!("{} has no tag dictionary", self.language))?.words),
            WordKind::Object => Ok(&self.words),
            WordKind::Adjective if self.layout.is_current() => Ok(&self.words),
            k => bail!("{} has no {k:?} words", self.language),
        }
    }

    fn list_mut(&mut self, kind: WordKind) -> Result<&mut Vec<Entry>> {
        self.list(kind)?;
        Ok(match kind {
            WordKind::Tag => &mut self.tags.as_mut().unwrap().words,
            _ => &mut self.words,
        })
    }

    /// Targets (id -> properties and display name) of a kind.
    pub fn targets(&self, kind: WordKind) -> Option<&BTreeMap<u16, Target>> {
        match kind {
            WordKind::Object => Some(&self.objects),
            WordKind::Adjective => Some(&self.adjectives),
            WordKind::Tag => self.tags.as_ref().map(|t| &t.tags),
            WordKind::Other(_) => None,
        }
    }

    pub fn targets_mut(&mut self, kind: WordKind) -> Option<&mut BTreeMap<u16, Target>> {
        match kind {
            WordKind::Object => Some(&mut self.objects),
            WordKind::Adjective => Some(&mut self.adjectives),
            WordKind::Tag => self.tags.as_mut().map(|t| &mut t.tags),
            WordKind::Other(_) => None,
        }
    }

    /// The word with this exact text and kind.
    pub fn find(&self, word: &str, kind: WordKind) -> Option<&Entry> {
        self.list(kind).ok()?.iter().find(|e| e.word == word && e.kind == kind)
    }

    pub fn find_mut(&mut self, word: &str, kind: WordKind) -> Option<&mut Entry> {
        self.list_mut(kind).ok()?.iter_mut().find(|e| e.word == word && e.kind == kind)
    }

    /// All words (any kind, including tags) whose normalized text equals `typed`'s.
    pub fn lookup(&self, typed: &str) -> Vec<&Entry> {
        let key = normalize_word(typed);
        let tag_words = self.tags.as_ref().map_or(&[][..], |t| &t.words[..]);
        self.words.iter().chain(tag_words).filter(|e| e.word == key).collect()
    }

    /// Ids of a kind whose resource is `resource` (a logical path such as
    /// `data\_game\scribbleobjects\mammal_large_hooved_cow.so`).
    pub fn ids_for_resource(&self, kind: WordKind, resource: &str) -> Vec<u16> {
        self.targets(kind)
            .map(|m| {
                m.iter()
                    .filter(|(_, t)| matches!(&t.properties.resource, Some(scribble_core::ResRef::Path(p)) if p.eq_ignore_ascii_case(resource)))
                    .map(|(&id, _)| id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The display name of an id.
    pub fn name_of(&self, kind: WordKind, id: u16) -> Option<&str> {
        self.targets(kind)?.get(&id)?.name.as_deref()
    }

    fn insert_sorted(list: &mut Vec<Entry>, e: Entry) -> Result<usize> {
        let key = wordfile::bytes(&e.word)?;
        let pos = list.partition_point(|x| wordfile::bytes(&x.word).map(|b| b <= key).unwrap_or(true));
        list.insert(pos, e);
        Ok(pos)
    }

    fn check_word(&self, word: &str, kind: WordKind, meanings: &[Sense]) -> Result<()> {
        ensure!(!word.is_empty(), "empty word");
        let b = wordfile::bytes(word)?;
        ensure!(!b.contains(&0), "word contains NUL");
        let (head, per) = if self.layout.is_current() || kind == WordKind::Tag { (8, 7) } else { (7, 6) };
        ensure!(b.len() + head + per * meanings.len() <= 255, "word {word:?} is too long for a dictionary record");
        let map = self.targets(kind).ok_or_else(|| anyhow!("no {kind:?} targets"))?;
        for s in meanings {
            ensure!(map.contains_key(&s.id), "unknown {kind:?} id {} (add it with add_target first)", s.id);
        }
        Ok(())
    }

    /// Add a word. The text is normalized with [`normalize_word`]; `meanings` are ids of existing
    /// targets of `kind` (see [`ids_for_resource`](Self::ids_for_resource)). Returns the stored text.
    pub fn add_word(&mut self, word: &str, kind: WordKind, meanings: Vec<Sense>) -> Result<String> {
        let word = normalize_word(word);
        ensure!(!meanings.is_empty(), "a word needs at least one meaning");
        self.check_word(&word, kind, &meanings)?;
        ensure!(self.find(&word, kind).is_none(), "{kind:?} word {word:?} already exists");
        Self::insert_sorted(self.list_mut(kind)?, Entry { word: word.clone(), kind, meanings })?;
        Ok(word)
    }

    /// Remove a word. Fails if a level translation (`.dtm`) still refers to it.
    pub fn remove_word(&mut self, word: &str, kind: WordKind) -> Result<Entry> {
        if let Some(t) = &self.translations {
            let tables: Vec<&Vec<WordLink>> = match kind {
                WordKind::Object => vec![&t.object_words],
                WordKind::Adjective => vec![&t.adjective_words, &t.apply_adjectives_words],
                _ => vec![],
            };
            for tab in tables {
                if let Some(l) = tab.iter().find(|l| l.word == WordRef::Word(word.to_string())) {
                    bail!("{word:?} is used by level word key {} in {}.dtm", l.key, self.language);
                }
            }
        }
        let list = self.list_mut(kind)?;
        let pos = list.iter().position(|e| e.word == word && e.kind == kind).ok_or_else(|| anyhow!("no {kind:?} word {word:?}"))?;
        let e = list.remove(pos);
        if let Some(m) = self.targets_mut(kind) {
            for t in m.values_mut() {
                if t.name.as_deref() == Some(word) {
                    t.name = None;
                }
            }
        }
        Ok(e)
    }

    /// Rename a word, keeping its meanings, display-name roles and level translations.
    pub fn rename_word(&mut self, old: &str, kind: WordKind, new: &str) -> Result<String> {
        let new = normalize_word(new);
        let e = self.find(old, kind).ok_or_else(|| anyhow!("no {kind:?} word {old:?}"))?.clone();
        ensure!(new == old || self.find(&new, kind).is_none(), "{kind:?} word {new:?} already exists");
        self.check_word(&new, kind, &e.meanings)?;
        let list = self.list_mut(kind)?;
        let pos = list.iter().position(|x| x.word == old && x.kind == kind).unwrap();
        let mut e = list.remove(pos);
        e.word = new.clone();
        Self::insert_sorted(list, e)?;
        if let Some(m) = self.targets_mut(kind) {
            for t in m.values_mut() {
                if t.name.as_deref() == Some(old) {
                    t.name = Some(new.clone());
                }
            }
        }
        if let Some(t) = &mut self.translations {
            let tables: Vec<&mut Vec<WordLink>> = match kind {
                WordKind::Object => vec![&mut t.object_words],
                WordKind::Adjective => vec![&mut t.adjective_words, &mut t.apply_adjectives_words],
                _ => vec![],
            };
            for tab in tables {
                for l in tab.iter_mut() {
                    if l.word == WordRef::Word(old.to_string()) {
                        l.word = WordRef::Word(new.clone());
                    }
                }
            }
        }
        Ok(new)
    }

    /// Replace what a word means.
    pub fn set_meanings(&mut self, word: &str, kind: WordKind, meanings: Vec<Sense>) -> Result<()> {
        self.check_word(word, kind, &meanings)?;
        let e = self.find_mut(word, kind).ok_or_else(|| anyhow!("no {kind:?} word {word:?}"))?;
        e.meanings = meanings;
        Ok(())
    }

    /// Register a new object/adjective/tag id (e.g. for a newly added `.so`). Fails if it exists.
    pub fn add_target(&mut self, kind: WordKind, id: u16, properties: Properties) -> Result<()> {
        let m = self.targets_mut(kind).ok_or_else(|| anyhow!("no {kind:?} targets"))?;
        ensure!(!m.contains_key(&id), "{kind:?} id {id} already exists");
        m.insert(id, Target { name: None, properties });
        Ok(())
    }
}

/// `related_objects_dictionary` + `related_objects_jumptable` with owners resolved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelatedObjects {
    /// Lists in file order (ascending object id).
    pub lists: Vec<RelatedList>,
    /// Entries in the jump table (grows automatically to max id + 1).
    pub object_id_slots: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelatedList {
    /// Owning object id; `None` for a list no jump-table entry reaches (the list at offset 0,
    /// which the engine treats as "no list").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<u16>,
    pub related: Vec<u16>,
}

impl RelatedObjects {
    pub const DICTIONARY: &'static str = "data\\_game\\scribbleobjects\\d\\related_objects_dictionary";
    pub const JUMPTABLE: &'static str = "data\\_game\\scribbleobjects\\d\\related_objects_jumptable";

    pub fn from_bytes(dictionary: &[u8], jumptable: &[u8]) -> Result<Self> {
        let (lists, offsets) = RelatedObjectsLists::decode_with_offsets(dictionary)?;
        let jt = OffsetTable::decode(jumptable, &Context::empty())?.offsets;
        let mut owner: HashMap<u32, u16> = HashMap::new();
        for (id, &o) in jt.iter().enumerate() {
            if o != 0 {
                ensure!(owner.insert(o, id as u16).is_none(), "two objects share related list at {o:#x}");
                ensure!(offsets.contains(&o), "related_objects_jumptable entry {id} points into a list");
            }
        }
        let lists = lists.lists.into_iter().zip(offsets).map(|(related, o)| RelatedList { object: owner.get(&o).copied(), related }).collect();
        Ok(RelatedObjects { lists, object_id_slots: jt.len() as u32 })
    }

    /// `(related_objects_dictionary, related_objects_jumptable)`.
    pub fn to_bytes(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        let lists = RelatedObjectsLists { lists: self.lists.iter().map(|l| l.related.clone()).collect() };
        let (dict, offsets) = lists.encode_with_offsets()?;
        let n = self.lists.iter().filter_map(|l| l.object).map(|o| o as u32 + 1).max().unwrap_or(0).max(self.object_id_slots);
        let mut jt = vec![0u32; n as usize];
        let mut seen = std::collections::HashSet::new();
        for (l, &o) in self.lists.iter().zip(&offsets) {
            if let Some(id) = l.object {
                ensure!(seen.insert(id), "object {id} has two related lists");
                jt[id as usize] = o;
            }
        }
        Ok((dict, offsets_to_bytes(&jt)))
    }

    pub fn load(mut src: impl ResourceSource) -> Result<Self> {
        let src: &mut dyn ResourceSource = &mut src;
        Self::from_bytes(&require(src, Self::DICTIONARY)?, &require(src, Self::JUMPTABLE)?)
    }

    pub fn to_files(&self) -> Result<Vec<(String, Vec<u8>)>> {
        let (d, j) = self.to_bytes()?;
        Ok(vec![(Self::DICTIONARY.to_string(), d), (Self::JUMPTABLE.to_string(), j)])
    }

    /// The related objects of an object id.
    pub fn related_to(&self, object: u16) -> Option<&[u16]> {
        self.lists.iter().find(|l| l.object == Some(object)).map(|l| &l.related[..])
    }

    /// Set (or add) the related list of an object, keeping lists in object-id order. A list is
    /// never placed at offset 0 (unreachable): an ownerless empty list is inserted first if needed.
    pub fn set_related(&mut self, object: u16, related: Vec<u16>) {
        if let Some(l) = self.lists.iter_mut().find(|l| l.object == Some(object)) {
            l.related = related;
            return;
        }
        let mut pos = self.lists.partition_point(|l| l.object.is_none_or(|o| o < object));
        if pos == 0 {
            self.lists.insert(0, RelatedList { object: None, related: Vec::new() });
            pos = 1;
        }
        self.lists.insert(pos, RelatedList { object: Some(object), related });
    }
}

/// A legacy adjective dictionary (`scribbleadjectives\dictionary\adjective_dictionary_<lang>`,
/// `adjective_jumptable_<lang>`, `adjective_word_id_table_<lang>`). Not used by the shipped
/// engine (which reads adjective words from `object_dictionary_<lang>`), but kept editable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LegacyAdjectiveDictionary {
    pub language: String,
    pub words: Vec<Entry>,
    pub adjectives: BTreeMap<u16, Target>,
    pub adjective_id_slots: u32,
}

impl LegacyAdjectiveDictionary {
    /// Languages with a legacy adjective dictionary.
    pub const LANGUAGES: [&'static str; 12] = [
        "danish",
        "dutch",
        "english",
        "english_uk",
        "finnish",
        "french",
        "german",
        "italian",
        "norwegian",
        "portuguese_brazil",
        "spanish",
        "swedish",
    ];

    pub fn load(language: &str, ctx: &Context, mut src: impl ResourceSource) -> Result<Self> {
        let src: &mut dyn ResourceSource = &mut src;
        let a = LEGACY_ADJECTIVE_DIR;
        let dict_path = path(a, &format!("adjective_dictionary_{language}"));
        let mut l = load_words(&require(src, &dict_path)?, WordKind::Adjective, ctx, &dict_path)?;
        ensure!(l.layout == Layout::Legacy && l.targets.keys().all(|k| *k == WordKind::Adjective), "{dict_path} is not a legacy adjective dictionary");
        l.check_word_ids(&require(src, &path(a, &format!("adjective_word_id_table_{language}")))?, None, "adjective_word_id_table")?;
        let slots = l.apply_names(&require(src, &path(a, &format!("adjective_jumptable_{language}")))?, WordKind::Adjective, "adjective_jumptable")?;
        Ok(LegacyAdjectiveDictionary {
            language: language.to_string(),
            adjectives: l.targets.remove(&WordKind::Adjective).unwrap_or_default(),
            words: l.entries,
            adjective_id_slots: slots,
        })
    }

    pub fn to_files(&self, ctx: &Context) -> Result<Vec<(String, Vec<u8>)>> {
        let a = LEGACY_ADJECTIVE_DIR;
        let lang = &self.language;
        let targets = |k: WordKind| (k == WordKind::Adjective).then_some(&self.adjectives);
        let b = build_words(Layout::Legacy, &self.words, &targets, ctx)?;
        Ok(vec![
            (path(a, &format!("adjective_dictionary_{lang}")), b.bytes.clone()),
            (path(a, &format!("adjective_word_id_table_{lang}")), b.word_id_table(&self.words, None)),
            (path(a, &format!("adjective_jumptable_{lang}")), b.jump_table(&self.words, WordKind::Adjective, &self.adjectives, self.adjective_id_slots)),
        ])
    }
}
