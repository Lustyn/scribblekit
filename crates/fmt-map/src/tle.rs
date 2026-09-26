//! `.tle`: tile map — the level's art pages and its collision tile grid.
//!
//! Loaded by `FUN_004b96d0` (level descriptor +0x00). The map is `width x height` tiles
//! (16 px each). Art is drawn from big texture "pages" (`_mapart\singles\<level>.ab`, `.ba`,
//! ...), collision from a grid of tile indices into a collision tileset (`.nbtc`), each with a
//! 2-bit transform, bit 0 = mirror horizontally, bit 1 = mirror vertically: the final shape is
//! `DAT_0083c418[shape * 4 + transform]`, and that table swaps left/right shapes for 1
//! (3<->4, 5<->6, ...), floor/ceiling shapes for 2 (1<->2, 5<->7, ...) and both for 3.
//!
//! ```text
//! u32 flags         // bit0 layers present, bit1 paths present, bit2 layers carry alternate pages
//! u16 width, height // tiles
//! -- if flags & 1:
//! u8 layer_count
//! layer_count x {
//!     u8 layer_flags            // bit0 art pages, bit1 collision grid
//!     -- if layer_flags & 1:
//!     u8 n; n x { u32 texture (0xFFFFFFFF = empty page); i32 -1 (never read) }
//!     -- if flags & 4:  u8 n; n x { u32 texture; i32 -1 }   (second page set, skipped on PC)
//!     -- if layer_flags & 2:
//!     u32 tileset_texture       // draws the grid when the layer has no art pages
//!     u32 tileset               // .nbtc (collision shapes per tile)
//!     u8  tiles[width*height]   // row-major tile index
//!     u8  transforms[width*height/4]  // 2 bits per tile, low bits first
//! }
//! -- if flags & 2:
//! u8 path_count
//! path_count x { u8 unused; u8 n; n x { i32 x, y } }   // 16.16 tile coordinates
//! ```
//!
//! Pages are placed row-major on a 256 px grid (`FUN_006f34d0`: `(width*16+255) >> 8`
//! columns); the loader reads only the first word of each page entry (`iVar6 += 8`).

use crate::common::{opt_res, opt_res_index};
use scribble_core::{bail, ensure, Context, Format, Fx16, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileMap {
    /// Width in tiles.
    pub width: u16,
    /// Height in tiles.
    pub height: u16,
    /// File flag bit 2: textured layers carry a second ("alternate") page list, which
    /// `FUN_004b96d0` skips unread (`iVar6 = iVar6 + 1 + count*8`). Set in almost every map,
    /// even ones whose layers have no pages.
    pub alternate_pages: bool,
    /// Drawing/collision layers (file flag bit 0); `null` when the map has no layer block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<TileLayer>>,
    /// Polylines (file flag bit 1), each sorted right to left by the loader
    /// (`FUN_004b1490`/`FUN_004b13a0`) and pushed onto the list at level+0x2b4
    /// (`FUN_004b8780`). No reader of that list was found; in the data they trace walkable
    /// surfaces (platform tops, ledges) in tile coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<TilePath>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileLayer {
    /// Art pages laid out over the map (`null` = empty page).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<Vec<Option<ResRef>>>,
    /// Second page list (present when [`TileMap::alternate_pages`] is set); skipped unread by
    /// the PC loader. In the data it holds the same textures laid out on a denser grid
    /// (ceil(W/204.8) columns instead of ceil(W/256)), probably another platform's layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate_pages: Option<Vec<Option<ResRef>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision: Option<CollisionGrid>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollisionGrid {
    /// Tileset texture: passed to the layer renderer (+0x54, `FUN_006f2700`), which draws the
    /// collision grid with it when the map has no art pages (`FUN_006f3700`:
    /// `if (*(layer+0x30) == 0 && tex) FUN_007175d0(tex, grid, 0xd20)`; e.g. `c_lava`,
    /// `c_stadium`).
    pub tileset_texture: ResRef,
    /// Collision tileset (`.nbtc`) giving each tile index its collision shape.
    pub tileset: ResRef,
    /// `height` rows, each a line of `width` tile indices (decimal, space separated, aligned
    /// so the rows read as a picture of the map).
    pub tiles: Vec<String>,
    /// `height` rows of `width` transform digits (0..3) of each tile's collision shape: bit 0
    /// mirrors it horizontally, bit 1 vertically (`DAT_0083c418[shape * 4 + transform]`;
    /// `FUN_005e2450` remaps directions the same way).
    pub transforms: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TilePath {
    /// Leading byte of the record; skipped by the loader (at 0x4b9a04 the count is read from
    /// byte 1: `movzx edx, byte [esi+eax+1]; add esi, 2`; always 0).
    #[serde(default, skip_serializing_if = "crate::common::is_zero_u8")]
    pub unused: u8,
    /// Points as `[x, y]` in tiles (16.16 fixed point).
    pub points: Vec<[Fx16; 2]>,
}

fn tile_row(row: &[u8]) -> String {
    row.iter().map(|t| format!("{t:3}")).collect::<Vec<_>>().join(" ")
}

fn parse_tile_row(row: &str) -> Result<Vec<u8>> {
    row.split_whitespace().map(|t| t.parse::<u8>().map_err(|_| anyhow::anyhow!("bad tile index {t:?}"))).collect()
}

fn read_pages(r: &mut Reader, ctx: &Context) -> Result<Vec<Option<ResRef>>> {
    let n = r.u8()?;
    let mut v = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let tex = r.u32()?;
        let second = r.i32()?;
        ensure!(second == -1, "page second word is {second}, expected -1");
        v.push(opt_res(tex, ctx));
    }
    Ok(v)
}

fn write_pages(w: &mut Writer, pages: &[Option<ResRef>], ctx: &Context) -> Result<()> {
    w.u8(u8::try_from(pages.len())?);
    for p in pages {
        w.u32(opt_res_index(p, ctx)?).i32(-1);
    }
    Ok(())
}

impl Format for TileMap {
    const NAME: &'static str = "tle";
    const DESCRIPTION: &'static str = "Tile map: art pages and collision tile grid of a level";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let flags = r.u32()?;
        ensure!(flags & !7 == 0, "unknown tile map flags {flags:#x}");
        let width = r.u16()?;
        let height = r.u16()?;
        let n = width as usize * height as usize;
        ensure!(n.is_multiple_of(4), "tile count {n} not a multiple of 4");
        let alternate_pages = flags & 4 != 0;
        let mut layers = None;
        if flags & 1 != 0 {
            let count = r.u8()?;
            let mut v = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let lf = r.u8()?;
                ensure!(lf & !3 == 0, "unknown layer flags {lf:#x}");
                let mut layer = TileLayer { pages: None, alternate_pages: None, collision: None };
                if lf & 1 != 0 {
                    layer.pages = Some(read_pages(&mut r, ctx)?);
                    if alternate_pages {
                        layer.alternate_pages = Some(read_pages(&mut r, ctx)?);
                    }
                }
                if lf & 2 != 0 {
                    let tileset_texture = ResRef::from_index(r.u32()?, ctx);
                    let tileset = ResRef::from_index(r.u32()?, ctx);
                    let tiles = r.bytes(n)?.chunks(width as usize).map(tile_row).collect();
                    let packed = r.bytes(n / 4)?;
                    let flat: Vec<u8> = (0..n).map(|i| (packed[i / 4] >> ((i % 4) * 2)) & 3).collect();
                    let transforms = flat.chunks(width as usize).map(|c| c.iter().map(|t| (b'0' + t) as char).collect()).collect();
                    layer.collision = Some(CollisionGrid { tileset_texture, tileset, tiles, transforms });
                }
                v.push(layer);
            }
            layers = Some(v);
        }
        let mut paths = None;
        if flags & 2 != 0 {
            let count = r.u8()?;
            let mut v = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let unused = r.u8()?;
                let n = r.u8()?;
                let mut points = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    points.push([Fx16(r.i32()?), Fx16(r.i32()?)]);
                }
                v.push(TilePath { unused, points });
            }
            paths = Some(v);
        }
        r.expect_end()?;
        Ok(TileMap { width, height, alternate_pages, layers, paths })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        let flags = self.layers.is_some() as u32 | (self.paths.is_some() as u32) << 1 | (self.alternate_pages as u32) << 2;
        w.u32(flags).u16(self.width).u16(self.height);
        let (width, height) = (self.width as usize, self.height as usize);
        if let Some(layers) = &self.layers {
            w.u8(u8::try_from(layers.len())?);
            for l in layers {
                w.u8(l.pages.is_some() as u8 | (l.collision.is_some() as u8) << 1);
                if let Some(p) = &l.pages {
                    write_pages(&mut w, p, ctx)?;
                    match (&l.alternate_pages, self.alternate_pages) {
                        (Some(a), true) => write_pages(&mut w, a, ctx)?,
                        (None, false) => {}
                        _ => bail!("layer alternate_pages must be present exactly when the map's alternate_pages flag is set"),
                    }
                }
                if let Some(c) = &l.collision {
                    w.u32(c.tileset_texture.to_index(ctx)?).u32(c.tileset.to_index(ctx)?);
                    ensure!(c.tiles.len() == height, "tiles must have {height} rows");
                    ensure!(c.transforms.len() == height, "transforms must have {height} rows");
                    for (y, row) in c.tiles.iter().enumerate() {
                        let vals = parse_tile_row(row).map_err(|e| e.context(format!("tiles row {y}")))?;
                        ensure!(vals.len() == width, "tiles row {y} has {} entries, expected {width}", vals.len());
                        w.bytes(&vals);
                    }
                    let mut flat = Vec::with_capacity(width * height);
                    for (y, row) in c.transforms.iter().enumerate() {
                        ensure!(row.len() == width, "transforms row {y} has {} digits, expected {width}", row.len());
                        for ch in row.bytes() {
                            ensure!((b'0'..=b'3').contains(&ch), "transforms row {y}: {:?} is not a digit 0..3", ch as char);
                            flat.push(ch - b'0');
                        }
                    }
                    for chunk in flat.chunks(4) {
                        let mut b = 0u8;
                        for (i, t) in chunk.iter().enumerate() {
                            b |= t << (i * 2);
                        }
                        w.u8(b);
                    }
                }
            }
        }
        if let Some(paths) = &self.paths {
            w.u8(u8::try_from(paths.len())?);
            for p in paths {
                w.u8(p.unused).u8(u8::try_from(p.points.len())?);
                for [x, y] in &p.points {
                    w.i32(x.0).i32(y.0);
                }
            }
        }
        Ok(w.into_inner())
    }
}
