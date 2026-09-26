//! `.vec`: vector art — a vertex-coloured 2D triangle mesh, split into per-bone parts.
//!
//! Every object in the game is drawn from one of these (`[platform]\datavector\...`). The engine
//! loads them as a kind of texture resource (`FUN_004cd6a0` "GRAPHICS CREATION - VECTOR" ->
//! constructor `FUN_00735b40` -> parser `FUN_00734ae0`), builds a GPU vertex buffer from the
//! parts (`FUN_00733ea0`) and draws it with per-vertex colours. Vectors named `*_texture.vec`
//! lay the separate body parts of an animated object out side by side like a texture atlas;
//! each part is skinned to one bone of the object's `.anim` skeleton.
//!
//! ## Units
//!
//! * Vertex positions are normalised: the art spans `[-0.5, 0.5]` on both axes (y points down),
//!   stretched over [`VectorArt::width`] x [`VectorArt::height`] art pixels, so
//!   `pixel = (position + 0.5) * size`. The engine sends them to the GPU as `short(pos * 8191)`
//!   (`DAT_0084a1b8` = 8191.0) and derives texture coordinates as `uv = position + 0.5`
//!   (`DAT_00824a10` = 0.5), both in `FUN_00733ea0`. Quantized files are decoded with a
//!   different constant, 1/8196 ([`QUANT_SCALE`], `FUN_00734ae0`): the 8191 only packs the
//!   already-decoded float for the GPU, so 8196 is the one that defines the file's unit.
//! * [`VectorArt::bone_bounds`] are in engine world units — the unit `.anim` positions use
//!   (the loader converts them to 20.12 fixed point, `round(v * 4096)`). Across all shipped
//!   files a part's art-pixel size is about 4 (3.5-5) times its world-unit bounds, i.e. roughly
//!   4 art pixels per world unit; the exact placement of a part on its bone comes from the
//!   object/animation data, not from the `.vec`.
//!
//! ## Parts
//!
//! Parts are addressed by [`Part::bone`] (the loader keeps them in a `std::map<u16, ...>`,
//! looked up by `FUN_007349d0`) — use [`VectorArt::part`], not the list position. For an atlas
//! the bone id is the *tree-order index* of the `.so` mesh-part node drawing it, which is also
//! the `.anim` track part index (`FUN_0069ea20` -> `FUN_005afce0` -> `FUN_007355e0`); the mesh
//! part node's own u16 is its draw rank, not a bone. A whole-art vector node takes its outline from bone 0
//! (`FUN_005afa80`). Static objects have a single part, bone 0.
//!
//! ```text
//! u8   flags            bit 7: bone bounds present, bit 6: quantized positions,
//!                       bits 0-5: format version (always 2; never read by the loader)
//! u16  width  * 2       authored size in half pixels (engine uses floor(size/4)*4)
//! u16  height * 2
//! u16  palette_count    number of palette entries (colour pairs); 0 = empty art, file ends here
//!                       (the loader only tests it for zero and re-reads the size at color_count)
//! u8   mesh_count       distinct triangle ranges among the parts (skipped by the loader)
//! u16  vertex_count     0 = empty art, file ends here
//! vertices:
//!   quantized (bit 6):  runs until vertex_count vertices were read:
//!                         u16 n, u16 color_slot, n x { i16 x, i16 y }   (value / 8196)
//!   float:              vertex_count x { f32 x, f32 y, u16 color_slot }
//! u16  index_count      0 = file ends here
//! u16  indices[index_count]                  triangle list
//! if flags bit 7:
//!   u16 bone_count, bone_count x { u16 bone (= position), f32 min_x, min_y, max_x, max_y }
//! u16  part_count
//! part_count x { u16 bone, u16 first_index, u16 end_index }
//! part_count x { u16 bone, u16 n, n/2 x { u16 first_vertex, u16 end_vertex } }   outline
//! part_count x { u16 bone, u16 n, n x u16 loop_length }                         outline loops
//! u16  color_count      (= 2 * palette_count; at most 0x400 fit the engine's table)
//! u32  colors[color_count]    0xAARRGGBB; pairs of (fill, transparent edge)
//! u16  regions[color_count]   paint region of each colour
//! ```
//!
//! Loader `FUN_00734ae0` reads exactly this sequence: `flags >> 6 & 1` selects the vertex
//! encoding, `(char)flags < 0` the bone bounds; `width`/`height` become `(raw >> 3) * 4`; bytes
//! 5-6 are only tested for non-zero; byte 7 is never read; vertices start at byte 10. Colours go
//! to `+0x8bc` (with an untouched copy at `+0x18bc` for restoring), regions to `+0x28bc`.

use scribble_core::{ensure, Context, Format, Reader, Result, Writer};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Scale of quantized vertex coordinates: `x = raw * QUANT_SCALE` (the engine's constant at
/// `0x0084a1d8`, `(float)(1/8196)` widened to a double, loaded as a float in `FUN_00734ae0`).
/// Note the GPU buffer uses 8191, not 8196 (see the module docs).
pub const QUANT_SCALE: f64 = f64::from_bits(0x3F1F_FC00_8000_0000);

const FLAG_BONE_BOUNDS: u8 = 0x80;
const FLAG_QUANTIZED: u8 = 0x40;
const VERSION_MASK: u8 = 0x3F;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorArt {
    /// Format version (low 6 bits of the flags byte); 2 in every shipped file. The loader never
    /// reads these bits (`FUN_00734ae0` tests only bits 6 and 7).
    pub version: u8,
    /// Authored width in pixels (half-pixel precision). The engine rounds it down to a multiple
    /// of 4 for its texture size, see [`VectorArt::texture_size`].
    pub width: f32,
    /// Authored height in pixels (half-pixel precision).
    pub height: f32,
    /// How vertex positions are stored on disk.
    pub position_encoding: PositionEncoding,
    /// Colours. A vertex's `color_slot` `s` uses entry `s / 2`: its `color` when `s` is even,
    /// its (fully transparent) `edge_color` when `s` is odd — used by the anti-aliasing fringe.
    /// The engine relies on the pairing: a slot whose alpha is 0 stands for the fill slot before
    /// it (`FUN_00735580`, `FUN_00565c40`), and recolouring writes both slots of a pair.
    pub palette: Vec<PaletteEntry>,
    /// Body parts: each one is a range of triangles drawn skinned to one skeleton bone.
    pub parts: Vec<Part>,
    /// Per-bone bounding boxes in world units (index = bone), when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bone_bounds: Option<Vec<Bounds>>,
    /// Stored "distinct meshes" count (header byte 7) when it differs from the number of
    /// distinct part triangle ranges (one shipped file, `conveyorbelt.vec`). The loader skips
    /// the byte, so it only matters for byte-exact round trips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_count: Option<u8>,
    /// `[x, y, color_slot]`.
    pub vertices: Vec<Vertex>,
    /// Triangle list, as indices into `vertices`.
    pub triangles: Vec<[u16; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionEncoding {
    /// `i16` pairs scaled by [`QUANT_SCALE`] (1/8196), grouped into runs of equal colour.
    Quantized,
    /// Raw `f32` pairs with a colour per vertex.
    Float,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "(f32, f32, u16)", into = "(f32, f32, u16)")]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
    /// Palette slot: `2 * entry` for the fill colour, `2 * entry + 1` for the transparent edge.
    pub color_slot: u16,
}

impl From<(f32, f32, u16)> for Vertex {
    fn from((x, y, color_slot): (f32, f32, u16)) -> Self {
        Vertex { x, y, color_slot }
    }
}
impl From<Vertex> for (f32, f32, u16) {
    fn from(v: Vertex) -> Self {
        (v.x, v.y, v.color_slot)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaletteEntry {
    pub color: Color,
    /// Paint region this colour belongs to: the in-game colour tool (`FUN_00565f20`) picks the
    /// region under the cursor, finds its dominant colour (`FUN_00733310`) and shifts every
    /// colour pair of that region; `FUN_00728d90` restores a region from the original colours
    /// and `FUN_00728ca0` sets a per-colour attribute by region. Region 0 gets no special
    /// treatment in that code (it is mostly the black outline colour in the shipped art).
    pub region: u16,
    /// Edge colour when it isn't the usual derived one (see [`Color::edge`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_color: Option<Color>,
    /// Edge region when it differs from `region`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_region: Option<u16>,
}

impl PaletteEntry {
    pub fn edge_color(&self) -> Color {
        self.edge_color.unwrap_or_else(|| self.color.edge())
    }
    pub fn edge_region(&self) -> u16 {
        self.edge_region.unwrap_or(self.region)
    }
}

/// A body part: triangles skinned to one bone, plus its outline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// Bone id: the tree-order index of the mesh part that draws it (= `.anim` track part).
    /// Several parts may share the same triangles (e.g. the cow's two pairs of legs), drawing
    /// the geometry once per bone.
    pub bone: u16,
    /// Half-open range of triangle numbers (`triangles[start..end]`).
    pub triangle_range: [u16; 2],
    /// Half-open vertex ranges holding this part's outline contour vertices.
    pub outline_ranges: Vec<[u16; 2]>,
    /// Lengths of the closed loops the outline vertices split into (sums to the number of
    /// outline vertices).
    pub outline_loops: Vec<u16>,
}

/// Axis-aligned box in world units (about 1/8 of an art pixel).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

/// A colour stored as `0xAARRGGBB`, written `"#rrggbb"` (opaque) or `"#rrggbbaa"`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color(pub u32);

impl Color {
    pub fn a(self) -> u8 {
        (self.0 >> 24) as u8
    }
    pub fn r(self) -> u8 {
        (self.0 >> 16) as u8
    }
    pub fn g(self) -> u8 {
        (self.0 >> 8) as u8
    }
    pub fn b(self) -> u8 {
        self.0 as u8
    }
    pub fn rgba(self) -> [u8; 4] {
        [self.r(), self.g(), self.b(), self.a()]
    }
    /// The transparent fringe colour the exporter pairs with a fill colour: alpha 0 and, as
    /// shipped, green and blue swapped (true for all 258,762 pairs in the shipped files). This is
    /// an exporter artifact, not engine behaviour: when the engine recolours a pair it writes
    /// `fill | 0xff000000` and the same RGB with alpha 0, unswapped (`FUN_00565f20`,
    /// `FUN_006788d0`).
    pub fn edge(self) -> Color {
        Color((self.0 & 0x00FF_0000) | ((self.b() as u32) << 8) | self.g() as u32)
    }
}

impl fmt::Debug for Color {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r(), self.g(), self.b())?;
        if self.a() != 0xFF {
            write!(f, "{:02x}", self.a())?;
        }
        Ok(())
    }
}

impl Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let hex = s.strip_prefix('#').unwrap_or(&s);
        let v = u32::from_str_radix(hex, 16).map_err(serde::de::Error::custom)?;
        match hex.len() {
            6 => Ok(Color(0xFF00_0000 | v)),
            8 => Ok(Color((v >> 8) | (v << 24))),
            _ => Err(serde::de::Error::custom(format!("expected #rrggbb or #rrggbbaa, got {s:?}"))),
        }
    }
}

fn read_u16s(r: &mut Reader, n: usize) -> Result<Vec<u16>> {
    (0..n).map(|_| r.u16()).collect()
}

fn half_pixels(v: f32, what: &str) -> Result<u16> {
    let raw = v * 2.0;
    ensure!(raw.fract() == 0.0 && (0.0..=65535.0).contains(&raw), "{what} {v} is not a multiple of 0.5 in 0..32767.5");
    Ok(raw as u16)
}

pub fn quantize(v: f32) -> Result<i16> {
    let q = (v as f64 / QUANT_SCALE).round();
    ensure!((i16::MIN as f64..=i16::MAX as f64).contains(&q), "coordinate {v} out of range for quantized positions");
    Ok(q as i16)
}

pub fn dequantize(q: i16) -> f32 {
    (q as f64 * QUANT_SCALE) as f32
}

impl Format for VectorArt {
    const NAME: &'static str = "vec";
    const DESCRIPTION: &'static str = "Vector art: vertex-coloured triangle mesh split into per-bone parts";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let flags = r.u8()?;
        let width = r.u16()? as f32 / 2.0;
        let height = r.u16()? as f32 / 2.0;
        let palette_count = r.u16()? as usize;
        let stored_mesh_count = r.u8()?;
        let vertex_count = r.u16()? as usize;
        let position_encoding = if flags & FLAG_QUANTIZED != 0 { PositionEncoding::Quantized } else { PositionEncoding::Float };
        let mut art = VectorArt {
            version: flags & VERSION_MASK,
            width,
            height,
            position_encoding,
            palette: Vec::new(),
            parts: Vec::new(),
            bone_bounds: None,
            mesh_count: None,
            vertices: Vec::with_capacity(vertex_count),
            triangles: Vec::new(),
        };
        let has_bounds = flags & FLAG_BONE_BOUNDS != 0;
        if palette_count == 0 || vertex_count == 0 {
            ensure!(!has_bounds && stored_mesh_count == 0, "empty vector art with bounds flag or meshes");
            ensure!(palette_count == 0 && vertex_count == 0, "vector art with only one of palette/vertices is not representable");
            r.expect_end()?;
            return Ok(art);
        }

        match position_encoding {
            PositionEncoding::Quantized => {
                while art.vertices.len() < vertex_count {
                    let n = r.u16()?;
                    ensure!(n > 0, "empty vertex run at {:#x}", r.pos());
                    let color_slot = r.u16()?;
                    if let Some(prev) = art.vertices.last() {
                        ensure!(prev.color_slot != color_slot, "adjacent vertex runs with equal colour");
                    }
                    for _ in 0..n {
                        let (x, y) = (r.i16()?, r.i16()?);
                        art.vertices.push(Vertex { x: dequantize(x), y: dequantize(y), color_slot });
                    }
                }
                ensure!(art.vertices.len() == vertex_count, "vertex runs overshoot the vertex count");
            }
            PositionEncoding::Float => {
                for _ in 0..vertex_count {
                    let (x, y, color_slot) = (r.f32()?, r.f32()?, r.u16()?);
                    art.vertices.push(Vertex { x, y, color_slot });
                }
            }
        }

        let index_count = r.u16()? as usize;
        ensure!(index_count % 3 == 0 && index_count > 0, "index count {index_count} is not a positive multiple of 3");
        let indices = read_u16s(&mut r, index_count)?;
        art.triangles = indices.chunks(3).map(|t| [t[0], t[1], t[2]]).collect();

        if has_bounds {
            let n = r.u16()?;
            let mut bounds = Vec::with_capacity(n as usize);
            for i in 0..n {
                let bone = r.u16()?;
                ensure!(bone == i, "bone bounds out of order ({bone} at position {i})");
                let v = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                bounds.push(Bounds { min: [v[0], v[1]], max: [v[2], v[3]] });
            }
            art.bone_bounds = Some(bounds);
        }

        let part_count = r.u16()? as usize;
        for _ in 0..part_count {
            let bone = r.u16()?;
            let (start, end) = (r.u16()?, r.u16()?);
            ensure!(start % 3 == 0 && end % 3 == 0, "part index range {start}..{end} not triangle aligned");
            art.parts.push(Part { bone, triangle_range: [start / 3, end / 3], outline_ranges: Vec::new(), outline_loops: Vec::new() });
        }
        for i in 0..part_count {
            let bone = r.u16()?;
            ensure!(bone == art.parts[i].bone, "outline list order differs from part order");
            let n = r.u16()? as usize;
            ensure!(n % 2 == 0, "odd outline range list");
            let v = read_u16s(&mut r, n)?;
            art.parts[i].outline_ranges = v.chunks(2).map(|c| [c[0], c[1]]).collect();
        }
        for i in 0..part_count {
            let bone = r.u16()?;
            ensure!(bone == art.parts[i].bone, "outline loop list order differs from part order");
            let n = r.u16()? as usize;
            art.parts[i].outline_loops = read_u16s(&mut r, n)?;
        }

        let color_count = r.u16()? as usize;
        ensure!(color_count == 2 * palette_count, "{color_count} colours for {palette_count} palette entries");
        let colors: Vec<Color> = (0..color_count).map(|_| r.u32().map(Color)).collect::<Result<_>>()?;
        let regions = read_u16s(&mut r, color_count)?;
        art.palette = (0..palette_count)
            .map(|i| {
                let (color, edge) = (colors[2 * i], colors[2 * i + 1]);
                let (region, edge_region) = (regions[2 * i], regions[2 * i + 1]);
                PaletteEntry {
                    color,
                    region,
                    edge_color: (edge != color.edge()).then_some(edge),
                    edge_region: (edge_region != region).then_some(edge_region),
                }
            })
            .collect();
        r.expect_end()?;

        if stored_mesh_count as usize != art.distinct_mesh_count() {
            art.mesh_count = Some(stored_mesh_count);
        }
        Ok(art)
    }

    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        ensure!(self.version <= VERSION_MASK, "version {} does not fit in 6 bits", self.version);
        let quantized = self.position_encoding == PositionEncoding::Quantized;
        let flags = self.version
            | if quantized { FLAG_QUANTIZED } else { 0 }
            | if self.bone_bounds.is_some() { FLAG_BONE_BOUNDS } else { 0 };
        w.u8(flags);
        w.u16(half_pixels(self.width, "width")?);
        w.u16(half_pixels(self.height, "height")?);
        let empty = self.palette.is_empty() || self.vertices.is_empty();
        if empty {
            ensure!(
                self.palette.is_empty() && self.vertices.is_empty() && self.triangles.is_empty() && self.parts.is_empty() && self.bone_bounds.is_none(),
                "vector art without palette or vertices must be completely empty"
            );
            w.u16(0).u8(self.mesh_count.unwrap_or(0)).u16(0);
            return Ok(w.into_inner());
        }
        w.u16(u16::try_from(self.palette.len())?);
        w.u8(match self.mesh_count {
            Some(n) => n,
            None => u8::try_from(self.distinct_mesh_count())?,
        });
        w.u16(u16::try_from(self.vertices.len())?);

        if quantized {
            let mut i = 0;
            while i < self.vertices.len() {
                let slot = self.vertices[i].color_slot;
                let run = self.vertices[i..].iter().take(u16::MAX as usize).take_while(|v| v.color_slot == slot).count();
                w.u16(run as u16).u16(slot);
                for v in &self.vertices[i..i + run] {
                    w.i16(quantize(v.x)?).i16(quantize(v.y)?);
                }
                i += run;
            }
        } else {
            for v in &self.vertices {
                w.f32(v.x).f32(v.y).u16(v.color_slot);
            }
        }

        ensure!(!self.triangles.is_empty(), "vector art with vertices needs at least one triangle");
        w.u16(u16::try_from(self.triangles.len() * 3)?);
        for t in &self.triangles {
            for &i in t {
                w.u16(i);
            }
        }

        if let Some(bounds) = &self.bone_bounds {
            w.u16(u16::try_from(bounds.len())?);
            for (i, b) in bounds.iter().enumerate() {
                w.u16(i as u16).f32(b.min[0]).f32(b.min[1]).f32(b.max[0]).f32(b.max[1]);
            }
        }

        w.u16(u16::try_from(self.parts.len())?);
        for p in &self.parts {
            let idx = |t: u16| u16::try_from(t as u32 * 3).map_err(|_| scribble_core::anyhow!("triangle {t} out of u16 index range"));
            w.u16(p.bone).u16(idx(p.triangle_range[0])?).u16(idx(p.triangle_range[1])?);
        }
        for p in &self.parts {
            w.u16(p.bone).u16(u16::try_from(p.outline_ranges.len() * 2)?);
            for r in &p.outline_ranges {
                w.u16(r[0]).u16(r[1]);
            }
        }
        for p in &self.parts {
            w.u16(p.bone).u16(u16::try_from(p.outline_loops.len())?);
            for &l in &p.outline_loops {
                w.u16(l);
            }
        }

        w.u16(u16::try_from(self.palette.len() * 2)?);
        for e in &self.palette {
            w.u32(e.color.0).u32(e.edge_color().0);
        }
        for e in &self.palette {
            w.u16(e.region).u16(e.edge_region());
        }
        Ok(w.into_inner())
    }
}

impl VectorArt {
    /// Number of distinct triangle ranges among the parts (what the header's mesh count holds).
    pub fn distinct_mesh_count(&self) -> usize {
        let mut ranges: Vec<[u16; 2]> = self.parts.iter().map(|p| p.triangle_range).collect();
        ranges.sort();
        ranges.dedup();
        ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    /// The size the engine uses for this art's texture: each dimension rounded down to a
    /// multiple of 4 (`(raw >> 3) * 4` in `FUN_00734ae0`).
    pub fn texture_size(&self) -> (u32, u32) {
        let f = |v: f32| ((v * 2.0) as u32 >> 3) * 4;
        (f(self.width), f(self.height))
    }

    /// Colour of a palette slot (even = fill, odd = transparent edge). Out-of-range slots are
    /// magenta.
    pub fn slot_color(&self, slot: u16) -> Color {
        match self.palette.get(slot as usize / 2) {
            Some(e) if slot % 2 == 0 => e.color,
            Some(e) => e.edge_color(),
            None => Color(0xFFFF_00FF),
        }
    }

    /// Recolourable region of a palette slot.
    pub fn slot_region(&self, slot: u16) -> u16 {
        match self.palette.get(slot as usize / 2) {
            Some(e) if slot % 2 == 0 => e.region,
            Some(e) => e.edge_region(),
            None => 0,
        }
    }
}
