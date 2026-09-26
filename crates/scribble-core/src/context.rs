//! Cross-file knowledge a codec may need: the resource table (pmindex index <-> logical path)
//! and named id namespaces (taxonomy levels, merits, ...) built from other files.
//!
//! Codecs never load files themselves; whoever builds the [`Context`] (the CLI, the GUI, tests;
//! see `scribble_formats::load_context`) fills it once from the unpacked game.

use std::collections::HashMap;

#[derive(Default, Clone)]
pub struct Context {
    names: Vec<Option<String>>,
    by_name: HashMap<String, u32>,
    namespaces: HashMap<String, IdNames>,
    platform: Platform,
}

/// The build a tree of resources comes from, for the few layouts that differ between builds
/// and cannot be told from the file itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Platform {
    /// Windows (Steam) release: 7 text languages.
    #[default]
    #[serde(rename = "pc")]
    Pc,
    /// Wii U (USA) release: 4 text languages (English, French, Portuguese, Spanish).
    #[serde(rename = "wiiu")]
    WiiU,
}

impl Platform {
    pub fn is_pc(&self) -> bool {
        *self == Platform::Pc
    }
}

impl Context {
    /// An empty context: resource references and ids stay numeric.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_names(entries: impl IntoIterator<Item = (u32, String)>) -> Self {
        let mut ctx = Context::default();
        for (idx, name) in entries {
            let i = idx as usize;
            if ctx.names.len() <= i {
                ctx.names.resize(i + 1, None);
            }
            ctx.by_name.insert(name.clone(), idx);
            ctx.names[i] = Some(name);
        }
        ctx
    }

    /// Logical path of a resource index.
    pub fn name(&self, index: u32) -> Option<&str> {
        self.names.get(index as usize)?.as_deref()
    }

    /// Resource index of a logical path.
    pub fn index(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    /// Every known `(index, logical path)`, in index order.
    pub fn resources(&self) -> impl Iterator<Item = (u32, &str)> {
        self.names.iter().enumerate().filter_map(|(i, n)| Some((i as u32, n.as_deref()?)))
    }

    /// Number of known resources.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// A named id namespace (e.g. `"object.category"`), if one was loaded.
    pub fn namespace(&self, name: &str) -> Option<&IdNames> {
        self.namespaces.get(name)
    }

    /// Add or replace a named id namespace.
    pub fn set_namespace(&mut self, name: impl Into<String>, ids: IdNames) {
        self.namespaces.insert(name.into(), ids);
    }

    /// The build these resources come from.
    pub fn platform(&self) -> Platform {
        self.platform
    }

    pub fn set_platform(&mut self, platform: Platform) {
        self.platform = platform;
    }

    /// Names of the loaded namespaces.
    pub fn namespace_names(&self) -> impl Iterator<Item = &str> {
        self.namespaces.keys().map(String::as_str)
    }
}

/// Names for the ids of one namespace, e.g. one level of the object taxonomy.
///
/// Names need not be unique: an id may also record the id of its *parent* (the level above in
/// a hierarchy), and a name only has to be unique among the children of one parent (`other` is
/// a subcategory of many categories). [`IdNames::display`] and [`IdNames::parse`] form a
/// bijection: an id prints as its bare name only when parsing that name back (with the same
/// parent) yields the id again, as `name#id` when the name is ambiguous, and as its decimal
/// number when it has no name.
#[derive(Default, Clone, Debug)]
pub struct IdNames {
    by_id: HashMap<u32, (String, Option<u32>)>,
    by_name: HashMap<String, Vec<u32>>,
}

impl IdNames {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `name` can be used as a printed name (it must not look like the other forms).
    pub fn is_valid_name(name: &str) -> bool {
        !name.is_empty() && name != "*" && !name.bytes().all(|b| b.is_ascii_digit()) && !name.contains(['/', '#']) && name.trim() == name
    }

    /// Name `id` (with an optional parent id for hierarchical lookups). Invalid names (see
    /// [`IdNames::is_valid_name`]) are ignored; re-inserting an id replaces its name.
    pub fn insert(&mut self, id: u32, name: impl Into<String>, parent: Option<u32>) {
        let name = name.into();
        if !Self::is_valid_name(&name) {
            return;
        }
        if let Some((old, _)) = self.by_id.remove(&id)
            && let Some(v) = self.by_name.get_mut(&old)
        {
            v.retain(|&x| x != id);
        }
        self.by_name.entry(name.clone()).or_default().push(id);
        self.by_id.insert(id, (name, parent));
    }

    pub fn name(&self, id: u32) -> Option<&str> {
        self.by_id.get(&id).map(|(n, _)| n.as_str())
    }

    pub fn parent(&self, id: u32) -> Option<u32> {
        self.by_id.get(&id).and_then(|(_, p)| *p)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Every `(id, name, parent)`, sorted by id.
    pub fn entries(&self) -> Vec<(u32, &str, Option<u32>)> {
        let mut v: Vec<_> = self.by_id.iter().map(|(id, (n, p))| (*id, n.as_str(), *p)).collect();
        v.sort_by_key(|e| e.0);
        v
    }

    /// Every `(id, name)`, unordered.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &str)> {
        self.by_id.iter().map(|(id, (n, _))| (*id, n.as_str()))
    }

    /// The id a bare name stands for: the unique id with that name under `parent` if there is
    /// one, else the unique id with that name overall.
    pub fn resolve(&self, name: &str, parent: Option<u32>) -> Option<u32> {
        let ids = self.by_name.get(name)?;
        if let Some(p) = parent {
            let mut under = ids.iter().filter(|&&id| self.parent(id) == Some(p));
            if let (Some(&id), None) = (under.next(), under.next()) {
                return Some(id);
            }
        }
        match ids.as_slice() {
            [id] => Some(*id),
            _ => None,
        }
    }

    /// Printed form of `id`: `name`, `name#id` or the decimal id.
    pub fn display(&self, id: u32, parent: Option<u32>) -> String {
        match self.name(id) {
            Some(n) if self.resolve(n, parent) == Some(id) => n.to_string(),
            Some(n) => format!("{n}#{id}"),
            None => id.to_string(),
        }
    }

    /// Inverse of [`IdNames::display`].
    pub fn parse(&self, text: &str, parent: Option<u32>) -> crate::Result<u32> {
        parse_component(Some(self), text, parent)
    }
}

/// Parse one printed id (`name`, `name#id` or a decimal number) against an optional namespace.
pub(crate) fn parse_component(ns: Option<&IdNames>, text: &str, parent: Option<u32>) -> crate::Result<u32> {
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        return text.parse().map_err(|_| anyhow::anyhow!("id {text:?} out of range"));
    }
    if let Some((name, id)) = text.rsplit_once('#') {
        let id: u32 = id.parse().map_err(|_| anyhow::anyhow!("bad id in {text:?}"))?;
        if let Some(known) = ns.and_then(|ns| ns.name(id)) {
            anyhow::ensure!(known == name, "{text:?}: id {id} is named {known:?}, not {name:?}");
        }
        return Ok(id);
    }
    let ns = ns.ok_or_else(|| anyhow::anyhow!("unknown name {text:?} (no names loaded; use the number)"))?;
    ns.resolve(text, parent).ok_or_else(|| {
        if ns.by_name.contains_key(text) {
            anyhow::anyhow!("name {text:?} is ambiguous here; write it as {text}#<id>")
        } else {
            anyhow::anyhow!("unknown name {text:?}")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_parse_bijective() {
        let mut ns = IdNames::new();
        ns.insert(1, "other", Some(10));
        ns.insert(2, "other", Some(20));
        ns.insert(3, "large", Some(10));
        ns.insert(4, "123", None); // invalid, ignored
        for (id, parent) in [(1, Some(10)), (2, Some(20)), (1, None), (3, None), (4, None), (99, None), (2, Some(10))] {
            let s = ns.display(id, parent);
            assert_eq!(ns.parse(&s, parent).unwrap(), id, "{s}");
        }
        assert_eq!(ns.display(1, Some(10)), "other");
        assert_eq!(ns.display(1, None), "other#1");
        assert_eq!(ns.display(3, None), "large");
        assert_eq!(ns.display(4, None), "4");
    }
}

/// Names of the well-known id namespaces, shared by the codecs that print ids and the loader
/// that fills them (`scribble_formats::load_context`).
pub mod ns {
    /// Object taxonomy, level 1 (`mammal`); from `.so` file names and headers.
    pub const OBJECT_CATEGORY: &str = "object.category";
    /// Object taxonomy, level 2 (`large`), parent = category.
    pub const OBJECT_SUBCATEGORY: &str = "object.subcategory";
    /// Object taxonomy, level 3 (`hooved`), parent = subcategory.
    pub const OBJECT_GROUP: &str = "object.group";
    /// Object taxonomy, level 4 = dictionary object word id (`cow`), parent = group.
    pub const OBJECT: &str = "object";
    /// The four object levels, in path order.
    pub const OBJECT_PATH: [&str; 4] = [OBJECT_CATEGORY, OBJECT_SUBCATEGORY, OBJECT_GROUP, OBJECT];
    /// Adjective taxonomy, level 1 (`color`); from `customfilters\asadjbycat_*.exf`.
    pub const ADJECTIVE_CATEGORY: &str = "adjective.category";
    /// Adjective taxonomy, level 2; from `customfilters\asadjbysubcat_*.exf`.
    pub const ADJECTIVE_GROUP: &str = "adjective.group";
    /// Adjective taxonomy, level 3 = adjective word id (`red`); from `.sa` file names.
    pub const ADJECTIVE: &str = "adjective";
    /// The three adjective levels, in path order.
    pub const ADJECTIVE_PATH: [&str; 3] = [ADJECTIVE_CATEGORY, ADJECTIVE_GROUP, ADJECTIVE];
    /// Object tags (`animal`, `farm_animal`); from the English tag dictionary.
    pub const TAG: &str = "tag";
    /// Merit ids; from the merit databases (`.mdb`).
    pub const MERIT: &str = "merit";
}
