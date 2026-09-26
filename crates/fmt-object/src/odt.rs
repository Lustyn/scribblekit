//! `.odt` - `data\_game\metadata\scribbleobject.odt` (resource 9426 = `0x24d2`): per-object
//! metadata flags, loaded once by `FUN_0068b9e0` into a map keyed by object resource index and
//! queried through `FUN_0068b6c0` (user-made objects, index bit 15, get theirs from the object
//! file instead, `FUN_006bb910`).
//!
//! ```text
//! u32 count
//! count x {
//!     u32 object      pmindex index of the .so/.sao
//!     u16 length
//!     length bytes    serialized protobuf-lite message "ObjectDetails" (ObjectDetails.pb.cpp,
//!                     MergePartialFromCodedStream = FUN_0057c910): 7 optional bool fields
//! }
//! ```
//!
//! The executable only embeds the lite runtime (no `FileDescriptorProto`, no `.proto` string),
//! so the protobuf field names are lost; every field is parsed as a bool (`FUN_00503ef0` for 1
//! and 2, `!= 0` for 3-7) into bytes `+4..+10` of the message. The names below come from the
//! engine code that reads each byte, the editor's writer for user objects (`FUN_006b1610`) and
//! the objects that carry them. Readers (all copy the message, then test bytes):
//!
//! ```text
//! FUN_004e8150  rope                                    -> static_objectproperties[108] "CANNOT BE USED AS A STAMP"
//! FUN_00583750  rope|group|attached_objects|sky_object|uncopyable -> [9]   "CANNOT BE PLACED IN A CONTAINER"
//! FUN_00590640  same five                               -> [101] "CANNOT BE USED AS A PROJECTILE"
//! FUN_0058b510  same five                               -> [107] "CANNOT BE USED THAT WAY"
//! FUN_006cf6f0  same five                               -> the calling script's message
//! FUN_0059d9f0  group|uncopyable -> static_objecteditor[7] "OBJECT CANNOT BE COPIED.";
//!               else maxwell_brother -> [24] "SORRY, NO MAXWELL BROTHERS ALLOWED!"
//! ```

use scribble_core::{bail, ensure, Context, Format, Hex, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

/// The whole table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectDetailsTable {
    pub entries: Vec<ObjectDetails>,
}

/// Metadata for one object. Each field is protobuf field N (all optional bools; absent = not
/// set; shipped files only ever store `true`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectDetails {
    pub object: ResRef,
    /// Field 1 (`+4`): spawns a group of objects (`crowd`, `sextuplets`, rooms, ponds: all 44
    /// have attached-object nodes and an `attached` sub-budget). Blocks containers,
    /// projectiles, "that way" and copying (see the module table); user objects write 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<bool>,
    /// Field 2 (`+5`): one of the 40 Maxwell brothers (`human_player_brothers_*`): the object
    /// editor refuses to copy it with "SORRY, NO MAXWELL BROTHERS ALLOWED!" (`FUN_0059d9f0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxwell_brother: Option<bool>,
    /// Field 3 (`+6`): read exactly like `group` (blocks containers, projectiles and copying:
    /// "OBJECT CANNOT BE COPIED.", `FUN_0059d9f0`); set by no shipped object and written as 0
    /// by the editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncopyable: Option<bool>,
    /// Field 4 (`+7`): sky object (stars, clouds, zodiac signs; static_objectproperties[24]
    /// "SKY OBJECT"). The editor writes the object's layer flag `+0x245` bit 2 here
    /// (`FUN_006b1610`), and it is set on exactly the 56 objects whose `.so` layer flags have
    /// that bit. Blocks containers and projectiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sky_object: Option<bool>,
    /// Field 5 (`+8`): has attached objects: the editor writes "its save collected sub-objects"
    /// (`FUN_006b1610`, list count `!= 0`); set on exactly the non-`group` objects with type-8
    /// attached-object nodes (vehicles, trees, armed humans). Blocks containers and projectiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached_objects: Option<bool>,
    /// Field 6 (`+9`): rope-like (ropes, chains, whips): the editor writes `+0x6d4 != 0`; set
    /// on exactly the 72 objects with `rope_segments`. The only flag that blocks stamps
    /// (`FUN_004e8150`); also blocks containers and projectiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rope: Option<bool>,
    /// Field 7 (`+10`): set on exactly the 692 objects of gender `both` (a male and a female
    /// body; humans and some creatures). Parsed but read by none of the flag consumers, and
    /// written as 0 by the editor. (Formerly `humanoid`, which 70 creatures and 4 easter
    /// eggs contradict).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub two_genders: Option<bool>,
    /// Message bytes that do not follow the canonical encoding (field order 1-7, one byte per
    /// value), kept verbatim. A codec fallback only: no shipped entry needs it (formerly `raw`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbatim_message: Option<Hex>,
}

fn varint(b: &[u8], p: &mut usize) -> Result<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let Some(&x) = b.get(*p) else { bail!("truncated varint") };
        *p += 1;
        v |= ((x & 0x7f) as u64) << shift;
        if x < 0x80 {
            return Ok(v);
        }
        shift += 7;
        ensure!(shift < 64, "varint too long");
    }
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

impl ObjectDetails {
    /// Details for `object` with no flags set.
    pub fn new(object: ResRef) -> Self {
        ObjectDetails {
            object,
            group: None,
            maxwell_brother: None,
            uncopyable: None,
            sky_object: None,
            attached_objects: None,
            rope: None,
            two_genders: None,
            verbatim_message: None,
        }
    }

    fn message(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut field = |n: u8, v: Option<u64>| {
            if let Some(v) = v {
                out.push(n << 3);
                put_varint(&mut out, v);
            }
        };
        field(1, self.group.map(u64::from));
        field(2, self.maxwell_brother.map(u64::from));
        field(3, self.uncopyable.map(u64::from));
        field(4, self.sky_object.map(u64::from));
        field(5, self.attached_objects.map(u64::from));
        field(6, self.rope.map(u64::from));
        field(7, self.two_genders.map(u64::from));
        out
    }

    fn parse(object: ResRef, blob: &[u8]) -> Self {
        let mut d = ObjectDetails::new(object.clone());
        let mut p = 0;
        let ok = (|| -> Result<()> {
            while p < blob.len() {
                let tag = varint(blob, &mut p)?;
                ensure!(tag & 7 == 0, "non-varint field");
                let v = varint(blob, &mut p)?;
                let b = |v: u64| -> Result<bool> {
                    ensure!(v <= 1, "bool out of range");
                    Ok(v == 1)
                };
                match tag >> 3 {
                    1 => d.group = Some(b(v)?),
                    2 => d.maxwell_brother = Some(b(v)?),
                    3 => d.uncopyable = Some(b(v)?),
                    4 => d.sky_object = Some(b(v)?),
                    5 => d.attached_objects = Some(b(v)?),
                    6 => d.rope = Some(b(v)?),
                    7 => d.two_genders = Some(b(v)?),
                    f => bail!("unknown field {f}"),
                }
            }
            Ok(())
        })();
        if ok.is_err() || d.message() != blob {
            return ObjectDetails { verbatim_message: Some(Hex(blob.to_vec())), ..ObjectDetails::new(object) };
        }
        d
    }
}

impl Format for ObjectDetailsTable {
    const NAME: &'static str = "odt";
    const DESCRIPTION: &'static str = "Object metadata flags keyed by object resource";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let n = r.u32()?;
        let mut entries = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let object = ResRef::from_index(r.u32()?, ctx);
            let len = r.u16()? as usize;
            entries.push(ObjectDetails::parse(object, r.bytes(len)?));
        }
        r.expect_end()?;
        Ok(ObjectDetailsTable { entries })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u32(self.entries.len() as u32);
        for e in &self.entries {
            w.u32(e.object.to_index(ctx)?);
            let msg = match &e.verbatim_message {
                Some(h) => h.0.clone(),
                None => e.message(),
            };
            w.u16(u16::try_from(msg.len())?);
            w.bytes(&msg);
        }
        Ok(w.into_inner())
    }
}
