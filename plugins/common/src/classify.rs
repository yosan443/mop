use std::fs::File;
use std::io::Read;
use std::path::Path;

use libarchive2::FileType;
use libarchive2::ReadArchive;

use crate::archive;

const VIDEO_EXTS: &[&str] = &[
    "mp4", "m4v", "mkv", "webm", "avi", "mov", "wmv", "flv", "mpg", "mpeg", "m2v", "ts", "mts",
    "m2ts", "vob", "ogv", "3gp", "3g2", "asf", "rm", "rmvb", "f4v", "y4m",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Manga,
    Video,
    Unknown,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Manga => "manga",
            Kind::Video => "video",
            Kind::Unknown => "unknown",
        }
    }
}

pub fn is_video_ext(ext: Option<&str>) -> bool {
    ext.map(|e| e.to_ascii_lowercase())
        .map(|e| VIDEO_EXTS.contains(&e.as_str()))
        .unwrap_or(false)
}

pub fn is_image_ext(ext: Option<&str>) -> bool {
    matches!(
        ext.map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "avif" | "heic" | "webp")
    )
}

fn magic_is_video(b: &[u8]) -> bool {
    if b.len() >= 8 && &b[4..8] == b"ftyp" {
        return true;
    }
    if b.len() >= 4 && b[..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        return true;
    }
    if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"AVI " {
        return true;
    }
    if b.len() >= 3 && &b[0..3] == b"FLV" {
        return true;
    }
    if b.len() >= 4 && (b[0..4] == [0x00, 0x00, 0x01, 0xBA] || b[0..4] == [0x00, 0x00, 0x01, 0xB3])
    {
        return true;
    }
    false
}

pub fn is_video_file(path: &Path) -> bool {
    if is_video_ext(path.extension().and_then(|e| e.to_str())) {
        return true;
    }
    let mut buf = [0u8; 16];
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let n = f.read(&mut buf).unwrap_or(0);
    magic_is_video(&buf[..n])
}

pub fn decide(images: usize, videos: usize, threshold: usize) -> Kind {
    if images >= threshold {
        Kind::Manga
    } else if videos > 0 {
        Kind::Video
    } else {
        Kind::Unknown
    }
}

pub fn classify(input: &Path, manga_image_threshold: usize, remove_macos_metadata: bool) -> Kind {
    match archive::open(input, None) {
        Ok(mut ar) => classify_archive(&mut ar, manga_image_threshold, remove_macos_metadata),
        Err(_) => {
            if is_video_file(input) {
                Kind::Video
            } else {
                Kind::Unknown
            }
        }
    }
}

fn classify_archive(
    ar: &mut ReadArchive<'static>,
    manga_image_threshold: usize,
    remove_macos_metadata: bool,
) -> Kind {
    let mut images = 0usize;
    let mut videos = 0usize;
    loop {
        let entry = match ar.next_entry() {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(_) => return Kind::Manga, // e.g. encrypted archive
        };
        if entry.file_type() == FileType::Directory {
            continue;
        }
        let name = entry.pathname().unwrap_or_default();
        if remove_macos_metadata && archive::is_macos_metadata(&name) {
            continue;
        }
        let ext = Path::new(&name).extension().and_then(|e| e.to_str());
        if is_image_ext(ext) {
            images += 1;
        } else if is_video_ext(ext) {
            videos += 1;
        }
    }
    decide(images, videos, manga_image_threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decide() {
        assert_eq!(decide(5, 0, 5), Kind::Manga);
        assert_eq!(decide(10, 2, 5), Kind::Manga);
        assert_eq!(decide(4, 1, 5), Kind::Video);
        assert_eq!(decide(0, 1, 5), Kind::Video);
        assert_eq!(decide(2, 0, 5), Kind::Unknown);
        assert_eq!(decide(0, 0, 5), Kind::Unknown);
    }

    #[test]
    fn test_is_video_ext() {
        assert!(is_video_ext(Some("mp4")));
        assert!(is_video_ext(Some("MKV")));
        assert!(is_video_ext(Some("avi")));
        assert!(!is_video_ext(Some("png")));
        assert!(!is_video_ext(Some("zip")));
        assert!(!is_video_ext(None));
    }

    #[test]
    fn test_is_image_ext() {
        assert!(is_image_ext(Some("png")));
        assert!(is_image_ext(Some("JPG")));
        assert!(is_image_ext(Some("webp")));
        assert!(!is_image_ext(Some("mp4")));
        assert!(!is_image_ext(None));
    }
}
