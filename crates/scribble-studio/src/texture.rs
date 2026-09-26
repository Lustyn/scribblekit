//! DDS -> egui texture, via the `image_dds` crate.

use anyhow::Result;
use eframe::egui;

pub fn decode_dds(data: &[u8]) -> Result<egui::ColorImage> {
    let dds = image_dds::ddsfile::Dds::read(data)?;
    let img = image_dds::image_from_dds(&dds, 0)?;
    Ok(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}
