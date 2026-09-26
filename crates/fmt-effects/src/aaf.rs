//! `.aaf`: audio metadata (`[platform]\audio\sfx\audiometadata.aaf`, pmindex 0x6153) — per-sound
//! playback settings, each a protobuf-lite `AudioMetaData.AudioItem` message
//! (`ToolTown\AA\AudioItem.pb.cpp`).
//!
//! Loaded once by the audio system init (`FUN_0051e180`), which parses every item
//! (`FUN_00441850` = `AudioItem::MergePartialFromCodedStream`; members at `+4` type, `+8` volume,
//! `+0xc` field 3, `+0x10..+0x28` fields 4..10, `+0x30` has-bits) into a map keyed by sound
//! resource (global `DAT_008ad7f0`). Defaults (`Clear`, `FUN_00440a60`): type 0, volume 1.0,
//! field 3 = 0.2, fields 4..10 = 18.
//!
//! The map is referenced only by `FUN_0051e180` (fill), `FUN_0051c5c0` (reads the node's volume)
//! and `FUN_0051d120` (play a sound: copies the item, reads type `+4`, volume `+8` and field 6
//! `+0x18`). The item pointer `FUN_0051d120` passes on to `FUN_0051ced0` is never dereferenced
//! (only the sound id reaches `FUN_00519d80`/`FUN_0051e5d0`), so fields 3, 4, 5 and 7..10 are
//! never read by the game.
//!
//! ```text
//! u32 count
//! count x {
//!     u32 sound    // pmindex index of the .wav (0xFFFFFFFF = none); many point at sounds not shipped
//!     u32 size
//!     u8  item[size]   // AudioItem protobuf
//! }
//!
//! message AudioItem {                       // proto2, lite runtime; no descriptor is embedded,
//!   optional SoundType          f1  = 1;    // field names below are descriptive, not original
//!   optional float              f2  = 2;    // volume (0..1)
//!   optional float              f3  = 3;    // never read
//!   optional SpeakerDestination f4  = 4;    // .. through field 10, each validated 0..18;
//! }                                         // only field 6 is read
//! enum SoundType { 0..4 }  enum SpeakerDestination { 0..18 }   // names from IsValid() asserts
//! ```

use scribble_core::{bail, Context, Format, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioMetadata {
    pub items: Vec<AudioItem>,
}

/// Playback settings for one sound.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct AudioItem {
    /// The sound (`.wav` resource), `null` for none: the map key (node `+0x40`, compared by
    /// `FUN_0051c2a0`) that `FUN_0051d120`'s sound id is looked up by.
    pub sound: Option<ResRef>,
    /// Field 1 (`AudioMetaData::SoundType`, name from the `SoundType_IsValid` CHECK in setter
    /// `FUN_00441160`). Absent means the default (0). Read by `FUN_0051d120`: `== 1` adds
    /// playback flags `0x2008000`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound_type: Option<SoundType>,
    /// Field 2: base volume (default 1.0). `FUN_0051d120` stores it as the voice volume
    /// (`+0xf0`, passed to `FUN_0051e5d0`); `FUN_0051c5c0` scales it by screen distance and
    /// clamps to 0..1.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::float::opt")]
    pub volume: Option<f32>,
    /// Field 3: float (default 0.2, `FUN_00440a60`) that the game never reads: item member
    /// `+0xc` is touched only by the generated parse/copy/serialize code (see module docs).
    /// Never set in shipped data.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::float::opt")]
    pub unused_float_3: Option<f32>,
    /// Field 4: `AudioMetaData::SpeakerDestination` (0..18, default 18; `SpeakerDestination_IsValid`
    /// CHECK in setter `FUN_00441210`). Never read (member `+0x10`); never set in shipped data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_speaker_destination_4: Option<i32>,
    /// Field 5: `SpeakerDestination` (setter `FUN_004412c0`), never read (member `+0x14`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_speaker_destination_5: Option<i32>,
    /// Field 6: `SpeakerDestination` (setter `FUN_00441370`), the only destination the game reads:
    /// `FUN_0051d120` adds playback flag `0x8000000` when it is 1, which `FUN_00517bb0` (surround
    /// routing, for voices flagged `0x4000000` when the speaker mode is not 0/5) turns into
    /// `AIL_set_sample_channel_levels` sending L/R to front L/R at 1.0 and to the LFE at 0.3.
    /// Set to 1 on 13 jingles / menu sounds. (The 19 valid values plausibly mirror Miles'
    /// 18 `MSS_SPEAKER` indices plus 18 = none, the default; not confirmed by code.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_destination_6: Option<i32>,
    /// Field 7: `SpeakerDestination` (setter `FUN_00441420`), never read (member `+0x1c`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_speaker_destination_7: Option<i32>,
    /// Field 8: `SpeakerDestination` (setter `FUN_004414d0`), never read (member `+0x20`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_speaker_destination_8: Option<i32>,
    /// Field 9: `SpeakerDestination` (setter `FUN_00441580`), never read (member `+0x24`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_speaker_destination_9: Option<i32>,
    /// Field 10: `SpeakerDestination` (setter `FUN_00441630`), never read (member `+0x28`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_speaker_destination_10: Option<i32>,
}

/// `AudioMetaData::SoundType` (valid values 0..4, `FUN_00441160`). The game only tests for 1
/// (`FUN_0051d120`); the other names are inferred from which sounds use them (all 134 type-2
/// items are `audio\music\mus_*`, the single type-3 item is `mus_forest_woods_ambience`, untyped
/// items are `audio\sfx\*` world sounds).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundType {
    /// 0 (the default): world sound effects (no special handling).
    Effect,
    /// 1: interface sounds (35 `ui_*`, jingles, `gui_cash`). `FUN_0051d120` adds flags
    /// `0x2008000` — the same bits it adds for callers passing the UI-sound flag `0x10` (buttons,
    /// menus: `FUN_00523d80`..`FUN_00537d10`) — and bit `0x8000` exempts the voice from the
    /// `0x40`/`0x80` volume fades of `FUN_0051fea0`.
    Interface,
    /// 2: music tracks (data-only name; value not tested by the game).
    Music,
    /// 3: ambience loops (data-only name; one item; value not tested by the game).
    Ambience,
    #[serde(untagged)]
    Other(i32),
}

impl SoundType {
    fn from_i32(v: i32) -> Self {
        match v {
            0 => SoundType::Effect,
            1 => SoundType::Interface,
            2 => SoundType::Music,
            3 => SoundType::Ambience,
            v => SoundType::Other(v),
        }
    }
    fn to_i32(self) -> i32 {
        match self {
            SoundType::Effect => 0,
            SoundType::Interface => 1,
            SoundType::Music => 2,
            SoundType::Ambience => 3,
            SoundType::Other(v) => v,
        }
    }
}

/// Wire form of `AudioMetaData.AudioItem`, encoded/decoded with `prost`.
#[derive(Clone, PartialEq, prost::Message)]
struct AudioItemProto {
    #[prost(int32, optional, tag = "1")]
    sound_type: Option<i32>,
    #[prost(float, optional, tag = "2")]
    volume: Option<f32>,
    #[prost(float, optional, tag = "3")]
    unused_float_3: Option<f32>,
    #[prost(int32, optional, tag = "4")]
    unused_speaker_destination_4: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    unused_speaker_destination_5: Option<i32>,
    #[prost(int32, optional, tag = "6")]
    speaker_destination_6: Option<i32>,
    #[prost(int32, optional, tag = "7")]
    unused_speaker_destination_7: Option<i32>,
    #[prost(int32, optional, tag = "8")]
    unused_speaker_destination_8: Option<i32>,
    #[prost(int32, optional, tag = "9")]
    unused_speaker_destination_9: Option<i32>,
    #[prost(int32, optional, tag = "10")]
    unused_speaker_destination_10: Option<i32>,
}

const NO_SOUND: u32 = u32::MAX;

impl AudioItem {
    fn from_proto(sound: Option<ResRef>, p: AudioItemProto) -> Self {
        AudioItem {
            sound,
            sound_type: p.sound_type.map(SoundType::from_i32),
            volume: p.volume,
            unused_float_3: p.unused_float_3,
            unused_speaker_destination_4: p.unused_speaker_destination_4,
            unused_speaker_destination_5: p.unused_speaker_destination_5,
            speaker_destination_6: p.speaker_destination_6,
            unused_speaker_destination_7: p.unused_speaker_destination_7,
            unused_speaker_destination_8: p.unused_speaker_destination_8,
            unused_speaker_destination_9: p.unused_speaker_destination_9,
            unused_speaker_destination_10: p.unused_speaker_destination_10,
        }
    }
    fn to_proto(&self) -> AudioItemProto {
        AudioItemProto {
            sound_type: self.sound_type.map(SoundType::to_i32),
            volume: self.volume,
            unused_float_3: self.unused_float_3,
            unused_speaker_destination_4: self.unused_speaker_destination_4,
            unused_speaker_destination_5: self.unused_speaker_destination_5,
            speaker_destination_6: self.speaker_destination_6,
            unused_speaker_destination_7: self.unused_speaker_destination_7,
            unused_speaker_destination_8: self.unused_speaker_destination_8,
            unused_speaker_destination_9: self.unused_speaker_destination_9,
            unused_speaker_destination_10: self.unused_speaker_destination_10,
        }
    }
}

impl Format for AudioMetadata {
    const NAME: &'static str = "aaf";
    const DESCRIPTION: &'static str = "Audio metadata: per-sound type, volume and routing (AudioItem protobufs)";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        use prost::Message;
        let mut r = Reader::new(data);
        let n = r.u32()?;
        let mut items = Vec::new();
        for _ in 0..n {
            let index = r.u32()?;
            let size = r.u32()? as usize;
            let at = r.pos();
            let bytes = r.bytes(size)?;
            let proto = AudioItemProto::decode(bytes)?;
            if proto.encode_to_vec() != bytes {
                bail!("AudioItem at {at:#x} is not in canonical protobuf form (unknown or repeated fields)");
            }
            let sound = (index != NO_SOUND).then(|| ResRef::from_index(index, ctx));
            items.push(AudioItem::from_proto(sound, proto));
        }
        r.expect_end()?;
        Ok(AudioMetadata { items })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        use prost::Message;
        let mut w = Writer::new();
        w.u32(self.items.len() as u32);
        for item in &self.items {
            w.u32(match &item.sound {
                Some(s) => s.to_index(ctx)?,
                None => NO_SOUND,
            });
            let bytes = item.to_proto().encode_to_vec();
            w.u32(bytes.len() as u32).bytes(&bytes);
        }
        Ok(w.into_inner())
    }
}
