//! Per-file views of the word files described in [`crate::wordfile`]:
//! [`WordList`] (object / tag / legacy adjective dictionaries), [`LegacyObjectDictionary`] and
//! [`SingleWordIndex`].

use crate::wordfile::{self, Gender, Layout, Properties, RawMeaning, RawRecord, WordKind};
use scribble_core::{Context, Format, ResRef, Result, bail, ensure};
use serde::{Deserialize, Serialize};

/// A word dictionary file: the sorted list of typeable words and what each one means.
///
/// Used for `object_dictionary_<lang>` (object *and* adjective words),
/// `tag_dictionary_<lang>` and the legacy `scribbleadjectives\dictionary\adjective_dictionary_<lang>`.
/// See [`crate::wordfile`] for the binary layout. The prefix index, word count and word ids are
/// regenerated on encode, so words must stay sorted by their raw bytes (Windows-1252 order).
/// Several words may share the same text if their kinds differ (`ACID` the adjective and `ACID`
/// the object); their relative order is kept as given.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WordList {
    #[serde(default, skip_serializing_if = "Layout::is_current")]
    pub layout: Layout,
    pub words: Vec<Word>,
}

/// One typeable word.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    /// Upper-case Windows-1252 text (bytes 0x80-0xFF appear as U+0080-U+00FF). Words starting
    /// with `$` are engine placeholders (`$ME`, `$ADJECTIVE1`), `@` marks internal/dev objects.
    pub word: String,
    /// Absent in the legacy layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<WordKind>,
    /// Stored word id, only when it is not the sequential index among words of the same kind
    /// (never the case in shipped files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_id: Option<u32>,
    /// What the word spawns; more than one means the game asks the player to pick (with the
    /// labels from `details_file_<lang>`).
    pub meanings: Vec<Meaning>,
}

/// One meaning of a word: an object / adjective / tag id plus the target's properties.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Meaning {
    /// Spawned resource (`.so` / `.sa`); absent for tags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResRef>,
    /// Object, adjective or tag id: a language-independent number that indexes the jump tables
    /// (`*_jumptable_<lang>`), `related_objects_*` and `details_file_<lang>`.
    pub id: u16,
    #[serde(default, skip_serializing_if = "wordfile::is_zero")]
    pub cost: u8,
    #[serde(default, skip_serializing_if = "wordfile::is_zero")]
    pub cost_multiplier: u8,
    #[serde(default, skip_serializing_if = "Gender::is_none")]
    pub gender: Gender,
    #[serde(default, skip_serializing_if = "wordfile::is_false")]
    pub random_gender: bool,
    #[serde(default, skip_serializing_if = "wordfile::is_zero")]
    pub unused_flags: u8,
}

impl Meaning {
    pub fn properties(&self) -> Properties {
        Properties {
            resource: self.resource.clone(),
            cost: self.cost,
            cost_multiplier: self.cost_multiplier,
            gender: self.gender,
            random_gender: self.random_gender,
            unused_flags: self.unused_flags,
        }
    }
    pub fn new(id: u16, p: Properties) -> Self {
        Meaning {
            id,
            resource: p.resource,
            cost: p.cost,
            cost_multiplier: p.cost_multiplier,
            gender: p.gender,
            random_gender: p.random_gender,
            unused_flags: p.unused_flags,
        }
    }
}

impl WordList {
    pub(crate) fn from_raw(file: wordfile::RawFile, ctx: &Context) -> Self {
        let ids = wordfile::sequential_ids(&file.records);
        let words = file
            .records
            .into_iter()
            .zip(ids)
            .map(|(r, seq)| Word {
                word: wordfile::text(&r.word),
                kind: r.kind.map(WordKind::from_u8),
                word_id: (r.word_id != seq).then_some(r.word_id),
                meanings: r.meanings.iter().map(|m| Meaning::new(m.id, m.properties(ctx))).collect(),
            })
            .collect();
        WordList { layout: file.layout, words }
    }

    pub(crate) fn to_raw(&self, ctx: &Context) -> Result<Vec<RawRecord>> {
        let mut records = Vec::with_capacity(self.words.len());
        for w in &self.words {
            ensure!(
                w.kind.is_some() == self.layout.is_current(),
                "word {:?}: kind must be present exactly in the current layout",
                w.word
            );
            let meanings = w
                .meanings
                .iter()
                .map(|m| RawMeaning::from_properties(m.id, &m.properties(), self.layout, ctx))
                .collect::<Result<Vec<_>>>()?;
            ensure!(meanings.len() < 256, "word {:?} has too many meanings", w.word);
            records.push(RawRecord { kind: w.kind.map(WordKind::to_u8), word: wordfile::bytes(&w.word)?, word_id: 0, meanings });
        }
        let seq = wordfile::sequential_ids(&records);
        for ((r, w), s) in records.iter_mut().zip(&self.words).zip(seq) {
            r.word_id = w.word_id.unwrap_or(s);
        }
        Ok(records)
    }

    /// Encode, also returning the absolute offset of every record (what jump tables store).
    pub fn encode_with_offsets(&self, ctx: &Context) -> Result<(Vec<u8>, Vec<u32>)> {
        let records = self.to_raw(ctx)?;
        for pair in records.windows(2) {
            ensure!(
                pair[0].word <= pair[1].word,
                "words must be sorted by their bytes: {:?} comes before {:?}",
                wordfile::text(&pair[0].word),
                wordfile::text(&pair[1].word)
            );
        }
        wordfile::write(self.layout, &records)
    }
}

impl Format for WordList {
    const NAME: &'static str = "word_dictionary";
    const DESCRIPTION: &'static str = "Sorted dictionary of typeable words and the objects/adjectives/tags they name";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        Ok(WordList::from_raw(wordfile::read(data)?, ctx))
    }
    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        Ok(self.encode_with_offsets(ctx)?.0)
    }
}

/// `object_dictionary_{danish,finnish,norwegian,swedish}`: a [`WordList`] in the legacy layout
/// whose resource indices point into an *older* pmindex (they would resolve to unrelated files
/// today), so they are kept as plain numbers. The object ids are still valid. The shipped engine
/// cannot select these languages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LegacyObjectDictionary(pub WordList);

impl Format for LegacyObjectDictionary {
    const NAME: &'static str = "legacy_object_dictionary";
    const DESCRIPTION: &'static str = "Old-build object dictionary (stale resource indices kept numeric)";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        Ok(LegacyObjectDictionary(WordList::decode(data, &Context::empty())?))
    }
    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        self.0.encode(&Context::empty())
    }
}

/// Set of word kinds, serialized as a list of names (`["object", "tag"]`); unknown bits stay
/// as `"bit_N"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct KindSet(pub u8);

impl KindSet {
    pub fn contains(self, k: WordKind) -> bool {
        self.0 & k.to_u8() != 0
    }
}

impl Serialize for KindSet {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let names: Vec<String> = (0..8)
            .filter(|b| self.0 & (1 << b) != 0)
            .map(|b| match 1u8 << b {
                1 => "object".to_string(),
                2 => "adjective".to_string(),
                4 => "tag".to_string(),
                _ => format!("bit_{b}"),
            })
            .collect();
        names.serialize(s)
    }
}

impl<'de> Deserialize<'de> for KindSet {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let names = Vec::<String>::deserialize(d)?;
        let mut v = 0u8;
        for n in names {
            v |= match n.as_str() {
                "object" => 1,
                "adjective" => 2,
                "tag" => 4,
                other => {
                    let b: u8 = other
                        .strip_prefix("bit_")
                        .and_then(|b| b.parse().ok())
                        .filter(|&b| b < 8)
                        .ok_or_else(|| serde::de::Error::custom(format!("unknown word kind {other:?}")))?;
                    1 << b
                }
            };
        }
        Ok(KindSet(v))
    }
}

/// `object_single_word_<lang>`: every space-separated token of every object, adjective and tag
/// word, with the kinds of the words it occurs in. Used for per-word autocompletion / spell
/// checking (lookup type 8 in `FUN_00741d10`, which matches `kind & mask`).
///
/// Same container as [`WordList`]; each record has word id 0 and one all-zero meaning.
/// Fully derived from the object and tag dictionaries (see [`crate::Dictionary`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SingleWordIndex {
    pub words: Vec<SingleWord>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SingleWord {
    pub word: String,
    /// Kinds of the dictionary words containing this token.
    pub used_by: KindSet,
}

impl SingleWordIndex {
    pub(crate) fn to_raw(&self) -> Result<Vec<RawRecord>> {
        self.words
            .iter()
            .map(|w| {
                Ok(RawRecord { kind: Some(w.used_by.0), word: wordfile::bytes(&w.word)?, word_id: 0, meanings: vec![RawMeaning::default()] })
            })
            .collect()
    }
}

impl Format for SingleWordIndex {
    const NAME: &'static str = "single_word_index";
    const DESCRIPTION: &'static str = "Every token of every dictionary word, with the kinds it occurs in";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        let file = wordfile::read(data)?;
        ensure!(file.layout == Layout::Current, "single-word index in legacy layout");
        let mut words = Vec::with_capacity(file.records.len());
        for r in file.records {
            if r.word_id != 0 || r.meanings != [RawMeaning::default()] {
                bail!("single-word record {:?} has unexpected payload", wordfile::text(&r.word));
            }
            words.push(SingleWord { word: wordfile::text(&r.word), used_by: KindSet(r.kind.unwrap_or(0)) });
        }
        Ok(SingleWordIndex { words })
    }
    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        Ok(wordfile::write(Layout::Current, &self.to_raw()?)?.0)
    }
}
