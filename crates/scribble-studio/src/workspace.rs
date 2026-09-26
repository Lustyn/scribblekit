//! The unpacked game directory being edited: resource list, name context, reads and writes.

use anyhow::{Context as _, Result};
use scribble_core::Context;
use scribble_pack::{Manifest, ManifestFile};
use std::path::{Path, PathBuf};

pub struct Workspace {
    pub root: PathBuf,
    pub manifest: Manifest,
    pub ctx: Context,
    /// Indices into `manifest.files` of resources that have data.
    pub present: Vec<usize>,
}

impl Workspace {
    pub fn open(root: &Path) -> Result<Self> {
        let manifest = Manifest::load(root).with_context(|| {
            format!("{} is not an unpacked game (run `scribble unpack <game> {}` first)", root.display(), root.display())
        })?;
        let ctx = scribble_formats::load_context(root)?;
        let present = manifest.files.iter().enumerate().filter(|(_, f)| f.pack.is_some()).map(|(i, _)| i).collect();
        Ok(Workspace { root: root.to_path_buf(), manifest, ctx, present })
    }

    pub fn file(&self, i: usize) -> &ManifestFile {
        &self.manifest.files[i]
    }

    pub fn find(&self, logical: &str) -> Option<usize> {
        let idx = self.ctx.index(logical)?;
        self.manifest.files.iter().position(|f| f.index == idx)
    }

    pub fn read(&self, i: usize) -> Result<Vec<u8>> {
        let f = self.file(i);
        std::fs::read(f.disk_path(&self.root)).with_context(|| format!("reading {}", f.path))
    }

    pub fn read_logical(&self, logical: &str) -> Result<Vec<u8>> {
        let i = self.find(logical).with_context(|| format!("no resource {logical}"))?;
        self.read(i)
    }

    pub fn write(&self, i: usize, data: &[u8]) -> Result<()> {
        let f = self.file(i);
        std::fs::write(f.disk_path(&self.root), data).with_context(|| format!("writing {}", f.path))
    }

    /// Rebuild the game's pack files from the workspace into `out`.
    pub fn export_game(&self, out: &Path) -> Result<()> {
        scribble_pack::pack(&self.root, out, |_, _| {})
    }
}
