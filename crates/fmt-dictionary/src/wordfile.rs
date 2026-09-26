//! Word dictionary files: `object_dictionary_<lang>`, `tag_dictionary_<lang>`,
//! `object_single_word_<lang>` (in `data\_game\scribbleobjects\d\`) and the legacy
//! `adjective_dictionary_<lang>` (in `data\_game\scribbleadjectives\dictionary\`).
//!
//! All of them share one container: a sorted list of word records behind a two-letter prefix
//! index. Loader: `FUN_00741d10` (the typed-word lookup) reads the alphabet, jumps through the
//! prefix table and scans records; `FUN_0073cd60`/`FUN_0073d3d0` read single records by offset.
//!
//! ```text
//! u8   n                     alphabet size
//! u8   alphabet[n]           every byte that occurs as 1st or 2nd character of a word, ascending
//! u32  word_count            number of records
//! u32  prefix[n][n]          absolute offset of the first record whose word starts with
//!                            alphabet[i] alphabet[j]; prefix[i][0] is additionally the first
//!                            record starting with alphabet[i] (used by the accent-folding
//!                            fuzzy search); single-letter words use j = 0; 0 = none
//! records, sorted by the raw bytes of the word:
//!   current layout (every language the engine supports):
//!     u8  record_len         total bytes of this record
//!     u8  kind               1 object, 2 adjective, 4 tag (single-word index: bit mask)
//!     u8  word_len
//!     u8  word[word_len]     upper-case Windows-1252, no terminator
//!     u32 word_id            index of this word among the words of its kind (sequential)
//!     u8  count              number of meanings
//!     u16 resource[count]    pmindex index of the .so/.sa spawned; 0xFFFF for tags
//!     u16 id[count]          object / adjective / tag id (language independent)
//!     u8  cost[count]        budget cost of the target (see [`Properties::cost`])
//!     u8  cost_multiplier[count]
//!     u8  flags[count]       bit0 random gender, bits1-2 gender (see [`Gender`]), bits 3-7 unused
//!   legacy layout (danish/finnish/norwegian/swedish objects, scribbleadjectives dictionaries):
//!     u8 record_len, u8 word_len, word, u32 word_id, u8 count,
//!     u16 resource[count], u16 id[count], u8 cost[count], u8 cost_multiplier[count]
//! ```
//!
//! The header (alphabet, count, prefix table) and the word ids are pure functions of the sorted
//! record list and are regenerated on encode.

use scribble_core::bin::{latin1, to_latin1};
use scribble_core::{Context, ResRef, Result, bail, ensure};
use serde::{Deserialize, Serialize};

/// Record layout of a word file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// With a kind byte and a per-meaning flags byte. Used by every language the shipped engine
    /// can select (english, dutch, french, german, italian, spanish_mexico, portuguese_brazil)
    /// and by the unused english_uk / spanish files.
    #[default]
    Current,
    /// No kind byte, no flags byte. Left over from an older build: the object dictionaries of
    /// danish/finnish/norwegian/swedish (whose resource indices refer to an older pmindex) and the
    /// adjective dictionaries in `scribbleadjectives\dictionary`.
    Legacy,
}

impl Layout {
    pub fn is_current(&self) -> bool {
        *self == Layout::Current
    }
}

/// What a word names. Stored as a byte; the single-word index stores a bit mask of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordKind {
    /// Spawns a scribble object (`.so`).
    Object,
    /// Applies an adjective (`.sa`).
    Adjective,
    /// A tag (object category such as `FOOD`, used by filters and "any X" constraints).
    Tag,
    #[serde(untagged)]
    Other(u8),
}

impl WordKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => WordKind::Object,
            2 => WordKind::Adjective,
            4 => WordKind::Tag,
            v => WordKind::Other(v),
        }
    }
    pub fn to_u8(self) -> u8 {
        match self {
            WordKind::Object => 1,
            WordKind::Adjective => 2,
            WordKind::Tag => 4,
            WordKind::Other(v) => v,
        }
    }
}

/// Gender of a target, bits 1-2 of the flags byte. The lookup `FUN_00741d10` stores
/// `flags >> 1 & 3` for every meaning in its result (vector at result `+0x54`, pushed by
/// `FUN_0073f740`), but no reader of that vector was found, so the names are inferred from which
/// objects carry each value (data only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gender {
    /// Not a person / not gendered.
    #[default]
    None,
    /// Male-only word (`GRANDPA`, `MALE WAITER`).
    Male,
    /// Female-only word (`BRIDE`, `COW`).
    Female,
    /// A person of either gender: named individuals (with `random_gender` off) and generic
    /// humans like `PILOT` (with `random_gender` on).
    Either,
}

impl Gender {
    pub fn is_none(&self) -> bool {
        *self == Gender::None
    }
    fn from_bits(v: u8) -> Self {
        match v & 3 {
            0 => Gender::None,
            1 => Gender::Male,
            2 => Gender::Female,
            _ => Gender::Either,
        }
    }
    fn to_bits(self) -> u8 {
        match self {
            Gender::None => 0,
            Gender::Male => 1,
            Gender::Female => 2,
            Gender::Either => 3,
        }
    }
}

/// The properties the engine reads for one meaning of a word. They are properties of the
/// target (the same for every word naming the same id, in every language), but the file repeats
/// them in every record.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Properties {
    /// The resource spawned/applied: `.so` for objects, `.sa` for adjectives; `null` (0xFFFF)
    /// for tags. In the legacy object dictionaries this is an index into an older pmindex and is
    /// kept numeric.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResRef>,
    /// Budget cost. `FUN_00740f30` computes an object's total = object.cost +
    /// Σ object.cost_multiplier × adjective.cost over the (up to 10) adjectives looked up with it,
    /// but nothing calls it (only an unreferenced incremental-link thunk at `0x00413e21` jumps
    /// there), so the value is loaded into the lookup result (`+0x28`) and otherwise unused.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cost: u8,
    /// Multiplier applied to the cost of adjectives on this object (`FUN_00740f30`); the equally
    /// uncalled `FUN_00740fe0` sums object.cost_multiplier × adjective.cost_multiplier. Loaded
    /// into the lookup result (`+0x38`) and otherwise unused.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cost_multiplier: u8,
    /// Gender (flags bits 1-2). Current layout only.
    #[serde(default, skip_serializing_if = "Gender::is_none")]
    pub gender: Gender,
    /// Flags bit 0: the object randomly spawns as male or female (generic humans such as
    /// `PILOT`, `LIFEGUARD`). Current layout only. `FUN_00741d10` stores `flags & 1` in its
    /// result (vector at `+0x44`); no reader was found, the name comes from the data.
    #[serde(default, skip_serializing_if = "is_false")]
    pub random_gender: bool,
    /// Flags bits 3-7: masked off by the lookup (`FUN_00741d10` only uses `& 1` and
    /// `>> 1 & 3`) and never set in shipped files; kept for losslessness.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unused_flags: u8,
}

pub(crate) fn is_zero(v: &u8) -> bool {
    *v == 0
}
pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}

impl Properties {
    pub(crate) fn flags_byte(&self) -> u8 {
        (self.random_gender as u8) | (self.gender.to_bits() << 1) | (self.unused_flags << 3)
    }
    fn set_flags(&mut self, v: u8) {
        self.random_gender = v & 1 != 0;
        self.gender = Gender::from_bits(v >> 1);
        self.unused_flags = v >> 3;
    }
    fn has_flags(&self) -> bool {
        self.random_gender || !self.gender.is_none() || self.unused_flags != 0
    }
}

pub(crate) const NO_RESOURCE: u16 = 0xFFFF;

/// One meaning as stored in a record (raw, before resource resolution).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct RawMeaning {
    pub resource: u16,
    pub id: u16,
    pub cost: u8,
    pub cost_multiplier: u8,
    pub flags: u8,
}

impl RawMeaning {
    pub fn properties(&self, ctx: &Context) -> Properties {
        let mut p = Properties {
            resource: (self.resource != NO_RESOURCE).then(|| ResRef::from_index(self.resource as u32, ctx)),
            cost: self.cost,
            cost_multiplier: self.cost_multiplier,
            ..Default::default()
        };
        p.set_flags(self.flags);
        p
    }
    pub fn from_properties(id: u16, p: &Properties, layout: Layout, ctx: &Context) -> Result<Self> {
        let resource = match &p.resource {
            None => NO_RESOURCE,
            Some(r) => {
                let i = r.to_index(ctx)?;
                ensure!(i < NO_RESOURCE as u32, "resource index {i} does not fit in 16 bits");
                i as u16
            }
        };
        ensure!(p.unused_flags < 32, "unused_flags must fit in 5 bits");
        if layout == Layout::Legacy {
            ensure!(!p.has_flags(), "legacy layout has no gender/flags byte (id {id})");
        }
        Ok(RawMeaning { resource, id, cost: p.cost, cost_multiplier: p.cost_multiplier, flags: p.flags_byte() })
    }
}

/// One record as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawRecord {
    /// `None` in the legacy layout.
    pub kind: Option<u8>,
    pub word: Vec<u8>,
    pub word_id: u32,
    pub meanings: Vec<RawMeaning>,
}

/// A parsed word file: its layout, the records and the offset of each record.
pub(crate) struct RawFile {
    pub layout: Layout,
    pub records: Vec<RawRecord>,
    pub offsets: Vec<u32>,
}

fn parse_records(data: &[u8], mut p: usize, layout: Layout) -> Option<(Vec<RawRecord>, Vec<u32>)> {
    let mut records = Vec::new();
    let mut offsets = Vec::new();
    let (head, per) = match layout {
        Layout::Current => (3usize, 7usize),
        Layout::Legacy => (2, 6),
    };
    while p < data.len() {
        let len = *data.get(p)? as usize;
        let kind = match layout {
            Layout::Current => Some(*data.get(p + 1)?),
            Layout::Legacy => None,
        };
        let wl = *data.get(p + head - 1)? as usize;
        let mut q = p + head;
        let word = data.get(q..q + wl)?.to_vec();
        q += wl;
        let word_id = u32::from_le_bytes(data.get(q..q + 4)?.try_into().ok()?);
        q += 4;
        let count = *data.get(q)? as usize;
        q += 1;
        if q + per * count != p + len || p + len > data.len() {
            return None;
        }
        let u16_at = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]);
        let meanings = (0..count)
            .map(|k| RawMeaning {
                resource: u16_at(q + 2 * k),
                id: u16_at(q + 2 * count + 2 * k),
                cost: data[q + 4 * count + k],
                cost_multiplier: data[q + 5 * count + k],
                flags: if per == 7 { data[q + 6 * count + k] } else { 0 },
            })
            .collect();
        offsets.push(p as u32);
        records.push(RawRecord { kind, word, word_id, meanings });
        p += len;
    }
    Some((records, offsets))
}

/// Alphabet: every byte used as 1st or 2nd character, ascending.
fn alphabet_of<'a>(words: impl Iterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut used = [false; 256];
    for w in words {
        for &c in w.iter().take(2) {
            used[c as usize] = true;
        }
    }
    (0..=255u8).filter(|&c| used[c as usize]).collect()
}

fn prefix_table(alphabet: &[u8], records: &[RawRecord], offsets: &[u32]) -> Vec<u32> {
    let n = alphabet.len();
    let mut idx = [0usize; 256];
    for (i, &c) in alphabet.iter().enumerate() {
        idx[c as usize] = i;
    }
    let mut t = vec![0u32; n * n];
    for (r, &off) in records.iter().zip(offsets) {
        let Some(&c0) = r.word.first() else { continue };
        let i = idx[c0 as usize];
        let j = r.word.get(1).map_or(0, |&c| idx[c as usize]);
        if t[i * n + j] == 0 {
            t[i * n + j] = off;
        }
        if t[i * n] == 0 {
            t[i * n] = off;
        }
    }
    t
}

/// Parse a word file and check that its header is exactly the one [`write`] would generate.
pub(crate) fn read(data: &[u8]) -> Result<RawFile> {
    ensure!(!data.is_empty(), "empty word file");
    let n = data[0] as usize;
    let start = 1 + n + 4 + 4 * n * n;
    ensure!(data.len() >= start, "word file shorter than its header");
    let alphabet = &data[1..1 + n];
    let count = u32::from_le_bytes(data[1 + n..5 + n].try_into().unwrap());
    let (layout, (records, offsets)) = if let Some(r) = parse_records(data, start, Layout::Current) {
        (Layout::Current, r)
    } else if let Some(r) = parse_records(data, start, Layout::Legacy) {
        (Layout::Legacy, r)
    } else {
        bail!("word records do not parse in either the current or the legacy layout");
    };
    ensure!(count as usize == records.len(), "header word count {count} != {} records", records.len());
    let derived = alphabet_of(records.iter().map(|r| r.word.as_slice()));
    ensure!(derived == alphabet, "alphabet is not the set of leading characters");
    let table = prefix_table(alphabet, &records, &offsets);
    for (k, v) in table.iter().enumerate() {
        let stored = u32::from_le_bytes(data[5 + n + 4 * k..9 + n + 4 * k].try_into().unwrap());
        ensure!(stored == *v, "prefix table entry {k} is {stored:#x}, derived {v:#x}");
    }
    Ok(RawFile { layout, records, offsets })
}

/// Serialize records (which must already be sorted); returns the bytes and each record's offset.
pub(crate) fn write(layout: Layout, records: &[RawRecord]) -> Result<(Vec<u8>, Vec<u32>)> {
    let alphabet = alphabet_of(records.iter().map(|r| r.word.as_slice()));
    let n = alphabet.len();
    ensure!(n < 256, "too many distinct leading characters ({n})");
    let start = 1 + n + 4 + 4 * n * n;
    let mut body = Vec::new();
    let mut offsets = Vec::with_capacity(records.len());
    for r in records {
        ensure!(!r.word.is_empty(), "empty word");
        ensure!(!r.word.contains(&0), "word contains NUL");
        let c = r.meanings.len();
        let len = match layout {
            Layout::Current => 3 + r.word.len() + 5 + 7 * c,
            Layout::Legacy => 2 + r.word.len() + 5 + 6 * c,
        };
        ensure!(len <= 255, "record for {:?} is {len} bytes (max 255): shorten the word or drop meanings", latin1(&r.word));
        offsets.push((start + body.len()) as u32);
        body.push(len as u8);
        match layout {
            Layout::Current => body.push(r.kind.ok_or_else(|| scribble_core::anyhow!("current layout needs a kind for {:?}", latin1(&r.word)))?),
            Layout::Legacy => ensure!(r.kind.is_none(), "legacy layout has no kind byte ({:?})", latin1(&r.word)),
        }
        body.push(r.word.len() as u8);
        body.extend_from_slice(&r.word);
        body.extend_from_slice(&r.word_id.to_le_bytes());
        body.push(c as u8);
        for m in &r.meanings {
            body.extend_from_slice(&m.resource.to_le_bytes());
        }
        for m in &r.meanings {
            body.extend_from_slice(&m.id.to_le_bytes());
        }
        body.extend(r.meanings.iter().map(|m| m.cost));
        body.extend(r.meanings.iter().map(|m| m.cost_multiplier));
        match layout {
            Layout::Current => body.extend(r.meanings.iter().map(|m| m.flags)),
            Layout::Legacy => ensure!(r.meanings.iter().all(|m| m.flags == 0), "legacy layout has no flags byte"),
        }
    }
    let table = prefix_table(&alphabet, records, &offsets);
    let mut out = Vec::with_capacity(start + body.len());
    out.push(n as u8);
    out.extend_from_slice(&alphabet);
    out.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for v in table {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&body);
    Ok((out, offsets))
}

/// Sequential word ids: the index of each record among the records of the same kind.
pub(crate) fn sequential_ids(records: &[RawRecord]) -> Vec<u32> {
    let mut next = std::collections::HashMap::new();
    records
        .iter()
        .map(|r| {
            let n = next.entry(r.kind).or_insert(0u32);
            *n += 1;
            *n - 1
        })
        .collect()
}

pub(crate) fn text(bytes: &[u8]) -> String {
    latin1(bytes)
}

pub(crate) fn bytes(s: &str) -> Result<Vec<u8>> {
    to_latin1(s)
}
