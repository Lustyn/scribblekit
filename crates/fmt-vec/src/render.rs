//! Rendering-oriented view of [`VectorArt`] and a small dependency-free software rasterizer.
//!
//! Vector art is untextured: every triangle is Gouraud-shaded from its vertices' palette
//! colours, and the transparent "edge" colours on the outermost vertices give anti-aliased
//! borders. Texture coordinates are what the engine derives for the art's own texture
//! (`uv = position + 0.5`, `FUN_00733ea0`), i.e. `(0,0)` is the top-left of the
//! [`VectorArt::texture_size`] image an animated object's skinned `.anim` mesh samples.

use crate::vec::{Bounds, VectorArt};

/// A vertex ready for drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshVertex {
    /// Normalised art coordinates (about `[-0.5, 0.5]`, y down).
    pub position: [f32; 2],
    /// `position + 0.5`: where this point lies in the art's texture (0..1, v down).
    pub uv: [f32; 2],
    /// Straight-alpha RGBA.
    pub color: [u8; 4],
    /// Recolourable region of the vertex colour.
    pub region: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Triangle {
    pub vertices: [MeshVertex; 3],
    /// Bone of the first part that contains this triangle, if any.
    pub bone: Option<u16>,
}

/// The triangles of one part, to be transformed by `bone`.
#[derive(Clone, Debug, PartialEq)]
pub struct PartMesh {
    pub bone: u16,
    pub triangles: Vec<Triangle>,
}

impl VectorArt {
    pub fn mesh_vertex(&self, index: u16) -> MeshVertex {
        let v = self.vertices[index as usize];
        MeshVertex {
            position: [v.x, v.y],
            uv: [v.x + 0.5, v.y + 0.5],
            color: self.slot_color(v.color_slot).rgba(),
            region: self.slot_region(v.color_slot),
        }
    }

    fn triangle(&self, t: usize, bone: Option<u16>) -> Triangle {
        let [a, b, c] = self.triangles[t];
        Triangle { vertices: [self.mesh_vertex(a), self.mesh_vertex(b), self.mesh_vertex(c)], bone }
    }

    /// Every triangle once, in file (draw) order — what the art's texture looks like.
    pub fn draw_triangles(&self) -> Vec<Triangle> {
        let mut bone_of = vec![None; self.triangles.len()];
        for p in &self.parts {
            for t in p.triangle_range[0] as usize..(p.triangle_range[1] as usize).min(self.triangles.len()) {
                bone_of[t].get_or_insert(p.bone);
            }
        }
        (0..self.triangles.len()).map(|t| self.triangle(t, bone_of[t])).collect()
    }

    /// One mesh per part (parts sharing geometry each get a copy), for skinned drawing.
    pub fn part_meshes(&self) -> Vec<PartMesh> {
        self.parts
            .iter()
            .map(|p| PartMesh {
                bone: p.bone,
                triangles: (p.triangle_range[0] as usize..(p.triangle_range[1] as usize).min(self.triangles.len()))
                    .map(|t| self.triangle(t, Some(p.bone)))
                    .collect(),
            })
            .collect()
    }

    /// The part a `.so` mesh-part node (by its tree-order index) / `.anim` track addresses:
    /// parts are keyed by their
    /// [`Part::bone`](crate::Part::bone) id (a `std::map<u16, ...>` in the engine), not by list
    /// position.
    pub fn part(&self, bone: u16) -> Option<&crate::Part> {
        self.parts.iter().find(|p| p.bone == bone)
    }

    /// Drawable triangles of the part with this bone id.
    pub fn part_mesh(&self, bone: u16) -> Option<PartMesh> {
        let p = self.part(bone)?;
        let range = p.triangle_range[0] as usize..(p.triangle_range[1] as usize).min(self.triangles.len());
        Some(PartMesh { bone, triangles: range.map(|t| self.triangle(t, Some(bone))).collect() })
    }

    /// Bounding box of one part's triangles, in normalised art coordinates (its rectangle in the
    /// art's texture: `uv = position + 0.5`).
    pub fn part_bounds(&self, bone: u16) -> Option<Bounds> {
        let p = self.part(bone)?;
        let tris = self.triangles.get(p.triangle_range[0] as usize..(p.triangle_range[1] as usize).min(self.triangles.len()))?;
        bounds_of(tris.iter().flatten().map(|&i| self.vertices[i as usize]))
    }

    /// The bone's bounding box in world units, when the art stores bone bounds.
    pub fn bone_bounds_of(&self, bone: u16) -> Option<Bounds> {
        self.bone_bounds.as_ref()?.get(bone as usize).copied()
    }

    /// Convert normalised art coordinates to art pixels (origin top-left).
    pub fn to_pixels(&self, p: [f32; 2]) -> [f32; 2] {
        [(p[0] + 0.5) * self.width, (p[1] + 0.5) * self.height]
    }

    /// Bounding box of all triangle vertices, in normalised art coordinates.
    pub fn bounds(&self) -> Option<Bounds> {
        bounds_of(self.triangles.iter().flatten().map(|&i| self.vertices[i as usize]))
    }

    /// A part's outline as closed polylines (normalised coordinates).
    pub fn outline_polylines(&self, bone: u16) -> Vec<Vec<[f32; 2]>> {
        let Some(p) = self.part(bone) else { return Vec::new() };
        let verts: Vec<[f32; 2]> = p
            .outline_ranges
            .iter()
            .flat_map(|r| r[0]..r[1])
            .filter_map(|i| self.vertices.get(i as usize).map(|v| [v.x, v.y]))
            .collect();
        let mut out = Vec::new();
        let mut at = 0;
        for &len in &p.outline_loops {
            let end = (at + len as usize).min(verts.len());
            out.push(verts[at..end].to_vec());
            at = end;
        }
        out
    }
}

fn bounds_of(mut it: impl Iterator<Item = crate::Vertex>) -> Option<Bounds> {
    let first = it.next()?;
    let mut b = Bounds { min: [first.x, first.y], max: [first.x, first.y] };
    for v in it {
        b.min = [b.min[0].min(v.x), b.min[1].min(v.y)];
        b.max = [b.max[0].max(v.x), b.max[1].max(v.y)];
    }
    Some(b)
}

/// What part of art space [`render_to_rgba`] shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    /// The whole `[-0.5, 0.5]²` square, stretched to the image — the art's texture as the
    /// engine rasterizes it (use [`atlas_dimensions`] for the right aspect).
    Atlas,
    /// Fit the drawn triangles' bounds into the image, keeping the art's pixel aspect, with
    /// `padding` pixels around.
    Fit { padding: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    pub view: View,
    /// Straight-alpha RGBA clear colour.
    pub background: [u8; 4],
    /// Samples per pixel along each axis (1 = none).
    pub supersample: u32,
    /// Also draw each part's outline loops as 1-pixel lines in this colour.
    pub outline_color: Option<[u8; 4]>,
    /// Draw (and, for [`View::Fit`], frame) only the part with this bone id.
    pub only_bone: Option<u16>,
}

impl RenderOptions {
    pub fn new(width: u32, height: u32) -> Self {
        RenderOptions { width, height, view: View::Fit { padding: 4 }, background: [0, 0, 0, 0], supersample: 2, outline_color: None, only_bone: None }
    }
}

/// Image dimensions with the art's aspect ratio whose larger side is `max_side`.
pub fn atlas_dimensions(art: &VectorArt, max_side: u32) -> (u32, u32) {
    let (w, h) = (art.width.max(1.0), art.height.max(1.0));
    let s = max_side as f32 / w.max(h);
    (((w * s).round() as u32).max(1), ((h * s).round() as u32).max(1))
}

/// Rasterize the art into a straight-alpha RGBA8 buffer of `width * height * 4` bytes.
pub fn render_to_rgba(art: &VectorArt, opts: &RenderOptions) -> Vec<u8> {
    let ss = opts.supersample.max(1);
    let (w, h) = (opts.width * ss, opts.height * ss);
    // Premultiplied float accumulation buffer.
    let bg = premul(opts.background);
    let mut buf = vec![bg; (w * h) as usize];

    // Map normalised art coordinates to (supersampled) pixels.
    let (sx, sy, ox, oy) = match opts.view {
        View::Atlas => (w as f32, h as f32, 0.5 * w as f32, 0.5 * h as f32),
        View::Fit { padding } => {
            let b = match opts.only_bone {
                Some(bone) => art.part_bounds(bone),
                None => art.bounds(),
            }
            .unwrap_or(Bounds { min: [-0.5, -0.5], max: [0.5, 0.5] });
            // Art pixels per normalised unit differ per axis (width x height stretch).
            let (ax, ay) = (art.width.max(1.0), art.height.max(1.0));
            let bw = ((b.max[0] - b.min[0]) * ax).max(1e-6);
            let bh = ((b.max[1] - b.min[1]) * ay).max(1e-6);
            let pad = (padding * ss) as f32;
            let s = ((w as f32 - 2.0 * pad) / bw).min((h as f32 - 2.0 * pad) / bh).max(1e-6);
            let (sx, sy) = (s * ax, s * ay);
            let cx = 0.5 * (b.min[0] + b.max[0]);
            let cy = 0.5 * (b.min[1] + b.max[1]);
            (sx, sy, 0.5 * w as f32 - cx * sx, 0.5 * h as f32 - cy * sy)
        }
    };
    let to_px = |p: [f32; 2]| [p[0] * sx + ox, p[1] * sy + oy];

    let triangles = match opts.only_bone {
        Some(bone) => art.part_mesh(bone).map(|m| m.triangles).unwrap_or_default(),
        None => art.draw_triangles(),
    };
    for t in triangles {
        let p = t.vertices.map(|v| to_px(v.position));
        let c = t.vertices.map(|v| [v.color[0] as f32, v.color[1] as f32, v.color[2] as f32, v.color[3] as f32 / 255.0]);
        fill_triangle(&mut buf, w, h, p, c);
    }

    if let Some(col) = opts.outline_color {
        let src = premul(col);
        for part in art.parts.iter().filter(|p| opts.only_bone.is_none_or(|b| b == p.bone)) {
            for poly in art.outline_polylines(part.bone) {
                for i in 0..poly.len() {
                    let a = to_px(poly[i]);
                    let b = to_px(poly[(i + 1) % poly.len()]);
                    draw_line(&mut buf, w, h, a, b, src, ss as f32);
                }
            }
        }
    }

    // Downsample and un-premultiply.
    let mut out = vec![0u8; (opts.width * opts.height * 4) as usize];
    let n = (ss * ss) as f32;
    for y in 0..opts.height {
        for x in 0..opts.width {
            let mut acc = [0f32; 4];
            for dy in 0..ss {
                for dx in 0..ss {
                    let px = buf[((y * ss + dy) * w + x * ss + dx) as usize];
                    for k in 0..4 {
                        acc[k] += px[k];
                    }
                }
            }
            let a = acc[3] / n;
            let o = ((y * opts.width + x) * 4) as usize;
            if a > 0.0 {
                for k in 0..3 {
                    out[o + k] = (acc[k] / n / a).round().clamp(0.0, 255.0) as u8;
                }
            }
            out[o + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// RGBA8 -> premultiplied [r, g, b] in 0..255 with alpha in 0..1.
fn premul(c: [u8; 4]) -> [f32; 4] {
    let a = c[3] as f32 / 255.0;
    [c[0] as f32 * a, c[1] as f32 * a, c[2] as f32 * a, a]
}

fn blend(dst: &mut [f32; 4], src: [f32; 4]) {
    let k = 1.0 - src[3];
    for i in 0..4 {
        dst[i] = src[i] + dst[i] * k;
    }
}

/// Gouraud-shaded triangle with straight-alpha vertex colours (interpolated like the GPU does),
/// blended source-over; pixel centres sampled, top-left fill rule approximated by `>= 0`.
fn fill_triangle(buf: &mut [[f32; 4]], w: u32, h: u32, p: [[f32; 2]; 3], c: [[f32; 4]; 3]) {
    let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
    if area.abs() < 1e-12 {
        return;
    }
    let minx = p.iter().map(|q| q[0]).fold(f32::INFINITY, f32::min).floor().max(0.0) as i64;
    let maxx = p.iter().map(|q| q[0]).fold(f32::NEG_INFINITY, f32::max).ceil().min(w as f32 - 1.0) as i64;
    let miny = p.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min).floor().max(0.0) as i64;
    let maxy = p.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max).ceil().min(h as f32 - 1.0) as i64;
    let edge = |a: [f32; 2], b: [f32; 2], x: f32, y: f32| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
    for y in miny..=maxy {
        for x in minx..=maxx {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(p[1], p[2], fx, fy) / area;
            let w1 = edge(p[2], p[0], fx, fy) / area;
            let w2 = edge(p[0], p[1], fx, fy) / area;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let mut col = [0f32; 4];
            for k in 0..4 {
                col[k] = c[0][k] * w0 + c[1][k] * w1 + c[2][k] * w2;
            }
            let a = col[3];
            blend(&mut buf[(y as u32 * w + x as u32) as usize], [col[0] * a, col[1] * a, col[2] * a, a]);
        }
    }
}

fn draw_line(buf: &mut [[f32; 4]], w: u32, h: u32, a: [f32; 2], b: [f32; 2], src: [f32; 4], thickness: f32) {
    let len = ((b[0] - a[0]).hypot(b[1] - a[1])).max(1.0);
    let steps = (len * 2.0) as usize + 1;
    let r = (thickness / 2.0).max(0.5);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let (cx, cy) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
        for y in (cy - r).floor() as i64..=(cy + r).floor() as i64 {
            for x in (cx - r).floor() as i64..=(cx + r).floor() as i64 {
                if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                    let px = &mut buf[(y as u32 * w + x as u32) as usize];
                    *px = src;
                }
            }
        }
    }
}
