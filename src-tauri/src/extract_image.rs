//! Raster images → normalized PNG base64 for vision LLM ingest.

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use image::GenericImageView;
use image::ImageFormat;

#[derive(Debug, Clone)]
pub struct IngestImage {
    pub mime: String,
    pub base64: String,
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
}

pub fn prepare_image_for_vision(
    bytes: &[u8],
    vision_max_bytes: usize,
    vision_max_edge_px: u32,
) -> Result<IngestImage> {
    if bytes.len() > vision_max_bytes {
        anyhow::bail!(
            "image exceeds vision_max_bytes ({} > {})",
            bytes.len(),
            vision_max_bytes
        );
    }

    let kind = infer::get(bytes).context("could not detect image type from bytes")?;
    let _mime_in = kind.mime_type();

    let img = image::load_from_memory(bytes).context("decode image")?;
    let (w, h) = img.dimensions();
    let img = if w > vision_max_edge_px || h > vision_max_edge_px {
        let max_edge = vision_max_edge_px.max(1);
        let scale = (max_edge as f32 / w.max(h) as f32).min(1.0);
        let nw = ((w as f32) * scale).round() as u32;
        let nh = ((h as f32) * scale).round() as u32;
        img.resize(nw.max(1), nh.max(1), image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    let (w, h) = img.dimensions();
    let mut encoded: Vec<u8> = Vec::new();
    {
        let mut c = std::io::Cursor::new(&mut encoded);
        img.write_to(&mut c, ImageFormat::Png)
            .context("encode png")?;
    }

    if encoded.len() > vision_max_bytes {
        anyhow::bail!(
            "encoded PNG still exceeds vision_max_bytes ({} > {})",
            encoded.len(),
            vision_max_bytes
        );
    }

    Ok(IngestImage {
        mime: "image/png".to_string(),
        base64: STANDARD.encode(&encoded),
        width: w,
        height: h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_png_roundtrip() {
        // 1x1 transparent PNG
        const PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00,
            0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63,
            0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00,
            0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let img = prepare_image_for_vision(PNG, 1_000_000, 4096).unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 1);
        assert_eq!(img.mime, "image/png");
        assert!(!img.base64.is_empty());
    }
}
