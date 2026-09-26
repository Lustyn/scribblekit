//! `.nbtc`: collision tileset — the collision shape of every tile index used by a tile map's
//! collision grid (`data\_game\_mapart\tilesets\*.nbtc`).
//!
//! Loaded by `FUN_005de450(this, res_a, res_b)`, called only as
//! `FUN_005d25c0(level+0x1ec, level+0x1f0)` from `FUN_004bfe00`. A shape is a list of edges in
//! tile pixel coordinates (0..16); each edge carries an index into the engine's edge-normal
//! table. The first pass reads only the tile count, the tile -> shape table and the shapes; the
//! per-tile destruction tables are read only `if (param_3 != 0)`, from the second resource
//! `level+0x1f0`, which is only ever zeroed (`FUN_0049fd50`, `FUN_004bfe00`). So on PC the
//! tables are never loaded, and the trailer lies beyond anything the loader reads.
//!
//! ```text
//! u16 tile_count
//! u8  shape_of_tile[tile_count]
//! u8  shape_count
//! shape_count x { u8 n; n x { u8 x1, y1, x2, y2, normal } }
//! u8  indestructible[tile_count]           // -> tileset+0x34
//! u8  neighbour_replacement[tile_count][8] // -> tileset+0x38
//! u16 neighbour_transform[tile_count]      // -> tileset+0x3c
//! u8  unused_trailer[...]                  // rest of the file (2560 bytes)
//! ```

use scribble_core::{Context, Format, Hex, Reader, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollisionTileset {
    /// Collision shapes; tiles refer to them by index.
    pub shapes: Vec<Vec<Edge>>,
    /// Per-tile data, indexed by tile number.
    pub tiles: Vec<TileInfo>,
    /// Bytes after the per-tile tables: never read (the second pass of `FUN_005de450` returns
    /// after the transform masks). In both shipped tilesets it is the same 2560 bytes =
    /// 256 x 8 + 256 x 2, shaped like a second `neighbour_replacement` + `neighbour_transform`
    /// pair (data-only observation).
    pub unused_trailer: Hex,
}

/// One collision edge from `from` to `to`, in pixels within the 16x16 tile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: [u8; 2],
    pub to: [u8; 2],
    /// Index into the edge-normal table `DAT_00897f88` (20.12 `(x, y)` pairs): 1 up (floor,
    /// `(0,-1)`), 2 down, 3 left, 4 right, 5-8 the 45° diagonals up-left/up-right/down-left/
    /// down-right, 9-12 the same four for 26.6° slopes. Read by `FUN_005bcc90` (a surface is
    /// walkable when normal y <= -0.6), `FUN_005c9330`, `FUN_005e01e0`.
    pub normal: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileInfo {
    /// Index into [`CollisionTileset::shapes`] (the only per-tile value the PC game loads).
    pub shape: u8,
    /// Would go to tileset+0x34: nonzero keeps the tile from being carved out
    /// (`FUN_005e21b0`: `layer1[i] != 0 || flag[tile] == 0 || param_6`). Never loaded on PC,
    /// and both callers pass `param_6 = 1` anyway. 1 on most empty tiles, 0 on solid ones.
    pub indestructible: u8,
    /// Would go to tileset+0x38: per direction (0 (x,y+1), 1 (x-1,y+1), 2 (x-1,y),
    /// 3 (x-1,y-1), 4 (x,y-1), 5 (x+1,y-1), 6 (x+1,y), 7 (x+1,y+1)) the tile this one becomes
    /// when that neighbour is carved, 0 = carved too (`FUN_005e2450`:
    /// `*(byte *)((tile*8 | dir) + *(param_1+0x38))`). Never loaded on PC, and the path is
    /// dead (its only caller `FUN_005e4d20` passes `param_7 = 0`).
    pub neighbour_replacement: [u8; 8],
    /// Would go to tileset+0x3c: 2 bits per direction, XORed with the tile's transform to give
    /// the replacement's transform (`FUN_005e2450`: `(mask >> dir*2) & 3 ^ transform`). Same
    /// dead path as `neighbour_replacement`.
    pub neighbour_transform: u16,
}

impl Format for CollisionTileset {
    const NAME: &'static str = "nbtc";
    const DESCRIPTION: &'static str = "Collision tileset: collision shape of every tile index";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let n = r.u16()? as usize;
        let shape_of: Vec<u8> = r.bytes(n)?.to_vec();
        let shape_count = r.u8()?;
        let mut shapes = Vec::with_capacity(shape_count as usize);
        for _ in 0..shape_count {
            let k = r.u8()?;
            let mut edges = Vec::with_capacity(k as usize);
            for _ in 0..k {
                let [x1, y1, x2, y2, normal] = r.array()?;
                edges.push(Edge { from: [x1, y1], to: [x2, y2], normal });
            }
            shapes.push(edges);
        }
        let flags = r.bytes(n)?.to_vec();
        let eights = r.bytes(n * 8)?.to_vec();
        let mut tiles = Vec::with_capacity(n);
        let mut masks = Vec::with_capacity(n);
        for _ in 0..n {
            masks.push(r.u16()?);
        }
        for i in 0..n {
            tiles.push(TileInfo {
                shape: shape_of[i],
                indestructible: flags[i],
                neighbour_replacement: eights[i * 8..i * 8 + 8].try_into().unwrap(),
                neighbour_transform: masks[i],
            });
        }
        let unused_trailer = Hex(r.rest().to_vec());
        Ok(CollisionTileset { shapes, tiles, unused_trailer })
    }

    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u16(u16::try_from(self.tiles.len())?);
        for t in &self.tiles {
            w.u8(t.shape);
        }
        w.u8(u8::try_from(self.shapes.len())?);
        for s in &self.shapes {
            w.u8(u8::try_from(s.len())?);
            for e in s {
                w.u8(e.from[0]).u8(e.from[1]).u8(e.to[0]).u8(e.to[1]).u8(e.normal);
            }
        }
        for t in &self.tiles {
            w.u8(t.indestructible);
        }
        for t in &self.tiles {
            w.bytes(&t.neighbour_replacement);
        }
        for t in &self.tiles {
            w.u16(t.neighbour_transform);
        }
        w.bytes(&self.unused_trailer.0);
        Ok(w.into_inner())
    }
}
