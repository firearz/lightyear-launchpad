//! `kestryx-skins`: Local skin & cape library, image validation (`64x64` & legacy `64x32`),
//! automatic `64x32` -> `64x64` transparent-layer converter, Mojang public profile importer,
//! and opt-in Offline Skin Bridge resourcepack generator (`kestryx-offline-skin.zip`).

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use image::{GenericImage, GenericImageView, RgbaImage};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkinModel {
    Classic, // 4px wide arms ("default")
    Slim,    // 3px wide arms ("slim")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinEntry {
    pub id: String,
    pub name: String,
    pub model: SkinModel,
    pub folder: String,
    pub tags: Vec<String>,
    pub blake3: String,
    pub file_path: PathBuf,
    pub was_legacy_64x32: bool,
    pub created_at: String,
}

/// Validates a PNG skin buffer and upgrades legacy `64x32` skins to modern `64x64` RGBA format
/// by copying the right leg (`0,16..16,32`) to the left leg (`16,48..32,64`) and right arm
/// (`40,16..56,32`) to the left arm (`32,48..48,64`), matching vanilla Minecraft's converter.
pub fn validate_and_normalize_skin_png(png_bytes: &[u8]) -> Result<( Vec<u8>, bool )> {
    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .context("Invalid PNG image for Minecraft skin")?;
    let (w, h) = img.dimensions();

    if w == 64 && h == 64 {
        return Ok((png_bytes.to_vec(), false));
    }
    if w != 64 || h != 32 {
        bail!(
            "Invalid skin dimensions {}x{} (expected 64x64 or legacy 64x32)",
            w,
            h
        );
    }

    // Upgrade 64x32 -> 64x64
    let src = img.to_rgba8();
    let mut dst = RgbaImage::new(64, 64);
    dst.copy_from(&src, 0, 0)?;

    // Copy right leg (x:0..16, y:16..32) -> left leg (x:16..32, y:48..64)
    for y in 0..16 {
        for x in 0..16 {
            let px = *src.get_pixel(x, 16 + y);
            dst.put_pixel(16 + x, 48 + y, px);
        }
    }
    // Copy right arm (x:40..56, y:16..32) -> left arm (x:32..48, y:48..64)
    for y in 0..16 {
        for x in 0..16 {
            let px = *src.get_pixel(40 + x, 16 + y);
            dst.put_pixel(32 + x, 48 + y, px);
        }
    }

    let mut out_buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut out_buf);
    dst.write_to(&mut cursor, image::ImageFormat::Png)?;
    Ok((out_buf, true))
}

/// Generates an opt-in local resource pack (`kestryx-offline-skin.zip`) inside the instance's
/// `resourcepacks/` folder so offline accounts display their custom skin in-game without requiring
/// online sessionserver authentication.
///
/// UI disclosure Note: Visible to the local player in Singleplayer and on LAN/servers using
/// Kestryx's local skin bridge; public online servers without a server skin plugin show default skins to others.
pub fn generate_offline_skin_resourcepack(
    instance_game_dir: &Path,
    skin_png_bytes: &[u8],
) -> Result<PathBuf> {
    let (normalized_png, _) = validate_and_normalize_skin_png(skin_png_bytes)?;
    let rp_dir = instance_game_dir.join("resourcepacks");
    fs::create_dir_all(&rp_dir)?;
    let zip_path = rp_dir.join("kestryx-offline-skin.zip");

    let file = File::create(&zip_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mcmeta = serde_json::json!({
        "pack": {
            "pack_format": 34,
            "supported_formats": { "min_inclusive": 1, "max_inclusive": 99 },
            "description": "Kestryx Managed Offline Skin Override (Local Visibility)"
        }
    });
    zip.start_file("pack.mcmeta", opts)?;
    zip.write_all( serde_json::to_string_pretty(&mcmeta)?.as_bytes() )?;

    // Override both legacy entity paths (`steve.png`/`alex.png`) and modern 1.19.3+/26.x wide/slim entity paths
    let skin_paths = [
        "assets/minecraft/textures/entity/steve.png",
        "assets/minecraft/textures/entity/alex.png",
        "assets/minecraft/textures/entity/player/wide/steve.png",
        "assets/minecraft/textures/entity/player/slim/alex.png",
    ];
    for p in skin_paths {
        zip.start_file(p, opts)?;
        zip.write_all(&normalized_png)?;
    }

    zip.finish()?;
    Ok(zip_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legacy_64x32_skin_conversion_and_offline_resourcepack() -> Result<()> {
        let mut legacy = RgbaImage::new(64, 32);
        legacy.put_pixel(2, 18, image::Rgba([255, 0, 0, 255]));
        let mut raw_png = Vec::new();
        legacy.write_to(&mut std::io::Cursor::new(&mut raw_png), image::ImageFormat::Png)?;

        let (upgraded_bytes, was_legacy) = validate_and_normalize_skin_png(&raw_png)?;
        assert!(was_legacy);
        let upgraded_img = image::load_from_memory(&upgraded_bytes)?;
        assert_eq!(upgraded_img.dimensions(), (64, 64));
        // Check that right leg pixel copied to left leg
        assert_eq!(upgraded_img.get_pixel(16 + 2, 48 + 2), image::Rgba([255, 0, 0, 255]));

        let dir = tempfile::tempdir()?;
        let pack_path = generate_offline_skin_resourcepack(dir.path(), &upgraded_bytes)?;
        assert!(pack_path.exists());
        Ok(())
    }
}
