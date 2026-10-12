// SPDX-License-Identifier: GPL-3.0-or-later

//! The parts of the image cache that touch no UI: where a downloaded picture
//! is stored, how old ones are removed and how bytes become pixels.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::ColorImage;

/// Extension of cached downloads (the format is detected from the bytes).
const CACHE_EXTENSION: &str = "img";

/// FNV-1a, 64 bits: a stable hash (unlike `DefaultHasher`) for file names
/// and for choosing a placeholder color.
pub fn stable_hash(text: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    text.bytes().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

/// Where the picture at `url` is cached inside `dir`.
pub fn cache_file(dir: &Path, url: &str) -> PathBuf {
    dir.join(format!("{:016x}.{CACHE_EXTENSION}", stable_hash(url)))
}

/// Removes the oldest cached pictures in `dir` until the folder holds at most
/// `limit` bytes. A missing folder is fine. Only files UnLauncher cached are
/// touched.
pub fn prune(dir: &Path, limit: u64) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|e| e == CACHE_EXTENSION)
        })
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| {
                (
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    meta.len(),
                    entry.path(),
                )
            })
        })
        .collect();
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    files.sort();
    for (_, len, path) in files {
        if total <= limit {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
    Ok(())
}

/// Decodes a PNG, JPEG or WebP and shrinks it to fit `max_side` pixels,
/// keeping its proportions. Smaller pictures are not enlarged.
pub fn decode(bytes: &[u8], max_side: u32) -> Result<ColorImage, image::ImageError> {
    let picture = image::load_from_memory(bytes)?;
    let picture = if picture.width() > max_side || picture.height() > max_side {
        picture.thumbnail(max_side, max_side)
    } else {
        picture
    };
    let rgba = picture.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::io::Cursor;
    use std::time::Duration;

    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let picture = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 100, 50, 255]));
        let mut bytes = Vec::new();
        picture
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn the_hash_does_not_change_between_runs() {
        // Placeholder colors and cache names must survive restarts.
        assert_eq!(stable_hash("KartRacer"), stable_hash("KartRacer"));
        assert_eq!(stable_hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(stable_hash("a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn cache_names_are_per_url_and_stay_inside_the_folder() {
        let dir = Path::new("/cache");
        let a = cache_file(dir, "https://cdn.example/a.jpg?x=../../etc");
        let b = cache_file(dir, "https://cdn.example/b.jpg");
        assert_ne!(a, b);
        assert_eq!(a.parent(), Some(dir));
        assert_eq!(a, cache_file(dir, "https://cdn.example/a.jpg?x=../../etc"));
    }

    #[test]
    fn large_pictures_shrink_and_keep_their_proportions() {
        let picture = decode(&png(2048, 1152), 512).unwrap();
        assert_eq!(picture.size, [512, 288]);
    }

    #[test]
    fn small_pictures_are_not_enlarged() {
        let picture = decode(&png(100, 50), 512).unwrap();
        assert_eq!(picture.size, [100, 50]);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode(b"<html>not an image</html>", 512).is_err());
    }

    #[test]
    fn pruning_removes_the_oldest_files_first() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        for (name, age) in [("old", 300), ("mid", 200), ("new", 100)] {
            let path = dir.path().join(format!("{name}.{CACHE_EXTENSION}"));
            fs::write(&path, vec![0_u8; 100]).unwrap();
            let file = fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(now - Duration::from_secs(age)).unwrap();
        }
        fs::write(dir.path().join("notes.txt"), vec![0_u8; 1000]).unwrap();

        prune(dir.path(), 150).unwrap();

        assert!(!dir.path().join("old.img").exists());
        assert!(!dir.path().join("mid.img").exists());
        assert!(dir.path().join("new.img").exists());
        assert!(dir.path().join("notes.txt").exists(), "foreign files stay");
    }

    #[test]
    fn pruning_a_missing_folder_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        prune(&dir.path().join("none"), 0).unwrap();
    }
}
