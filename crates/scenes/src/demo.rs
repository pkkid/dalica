//! A demo library: folders of camera-like JPEGs with EXIF dates, orientations cycling through all
//! eight values, and embedded thumbnails. The plan is returned as (relative path, bytes) pairs;
//! the caller writes them (through `dalica-fs` in the product, layer rule 4).

use std::path::PathBuf;

use dalica_core::DateTime;

use crate::exif::Exif;
use crate::render::Scene;
use crate::{Error, Result, jpeg};

/// One photo of a demo library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoFile {
    /// Relative to the library root: `<folder>/<name>`.
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    /// EXIF orientation 1–8.
    pub orientation: u8,
    pub date_time_original: DateTime,
    /// Size as displayed (after orientation).
    pub width: u32,
    pub height: u32,
}

/// Names for demo folders, after the date.
const PLACES: [&str; 8] = ["Harbour", "Hills", "Garden", "Market", "Coast", "Forest", "River", "Old Town"];
/// 2026-06-04T09:00:00, the first photo's date.
const FIRST_PHOTO: i64 = 1_780_563_600;
/// Displayed size of landscape photos (portrait ones swap it).
const SIZE: (u32, u32) = (480, 320);
const QUALITY: u8 = 85;
const THUMB: u32 = 160;
/// The most photos one demo library may have.
pub const MAX_PHOTOS: usize = 100_000;

/// `photos` JPEGs spread evenly over `folders` folders (folders that would get none are left out),
/// all decided by `seed`. Photo `i` (from 0) is `IMG_<i+1>.jpg` with orientation `1 + i % 8`, taken
/// 7 minutes after the previous one (a day later in each new folder); every fifth is portrait.
pub fn demo_library(folders: usize, photos: usize, seed: u64) -> Result<Vec<DemoFile>> {
    if photos > MAX_PHOTOS {
        return Err(Error::TooManyPhotos(photos));
    }
    let folders = folders.clamp(1, photos.max(1));
    let mut out = Vec::with_capacity(photos);
    for i in 0..photos {
        let folder = i * folders / photos;
        // Both counts are at most MAX_PHOTOS, so these stay far inside years 1–9999.
        let day = FIRST_PHOTO + folder as i64 * 86_400;
        let date = DateTime::from_unix_secs(day + i as i64 * 7 * 60).ok_or(Error::TooManyPhotos(photos))?;
        let folder_date = DateTime::from_unix_secs(day).ok_or(Error::TooManyPhotos(photos))?.date();
        let folder_name = format!("{folder_date} {}", PLACES.get(folder % PLACES.len()).copied().unwrap_or("Photos"));
        let (width, height) = if i % 5 == 4 { (SIZE.1, SIZE.0) } else { SIZE };
        let orientation = 1 + (i % 8) as u8;
        let exif = Exif {
            orientation,
            date_time_original: Some(date),
            make: Some("Dalica".into()),
            model: Some("Scenes 1".into()),
            thumbnail: Some(THUMB),
            ..Exif::default()
        };
        let scene = Scene::new(seed.wrapping_mul(1_000_003).wrapping_add(i as u64), width, height);
        out.push(DemoFile {
            path: PathBuf::from(folder_name).join(format!("IMG_{:04}.jpg", i + 1)),
            bytes: jpeg(&scene, QUALITY, Some(&exif))?,
            orientation,
            date_time_original: date,
            width,
            height,
        });
    }
    Ok(out)
}
