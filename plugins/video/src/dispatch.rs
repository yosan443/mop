use std::path::Path;

use mop_plugin_common::classify::{classify, Kind};
use mop_plugin_common::error::Reason;
use tracing::info;

use crate::config::VideoConfig;
use crate::video::{convert_one, VideoConvertOptions, VideoResult};

/// Dispatch an ingested file for the video plugin.
/// - Kind::Video -> transcode to video_dir
/// - Kind::Manga -> skipped; NEVER deleted, NEVER moved
/// - Kind::Unknown -> ignored; NEVER deleted, NEVER moved
pub fn process(input: &Path, cfg: &VideoConfig, opts: &VideoConvertOptions) -> VideoResult {
    let kind = classify(input, 5, true);

    match kind {
        Kind::Video => {
            info!("Ingested video candidate: {}", input.display());
            convert_one(input, cfg, opts)
        }
        Kind::Manga => {
            info!(
                "Skipping manga archive in video plugin (owned by mop.manga): {}",
                input.display()
            );
            VideoResult {
                status: "skipped",
                reason: Some(Reason::UnsupportedFileType),
                output: None,
                outputs: Vec::new(),
                error: Some("manga archive skipped by video plugin".to_string()),
            }
        }
        Kind::Unknown => {
            info!(
                "Ignoring unrecognized file in video plugin: {}",
                input.display()
            );
            VideoResult {
                status: "skipped",
                reason: Some(Reason::UnsupportedFileType),
                output: None,
                outputs: Vec::new(),
                error: Some("unrecognized file type ignored by video plugin".to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_manga_file_skipped_and_never_deleted() {
        let dir = tempdir().unwrap();
        let zip_path = dir.path().join("sample_manga.zip");
        {
            let mut f = File::create(&zip_path).unwrap();
            // Build a small zip with dummy images
            use zip::write::SimpleFileOptions;
            use zip::{CompressionMethod, ZipWriter};
            let mut zip = ZipWriter::new(&mut f);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            for i in 0..6 {
                zip.start_file(format!("page_{i}.jpg"), opts).unwrap();
                zip.write_all(b"fake image data").unwrap();
            }
            zip.finish().unwrap();
        }
        assert!(zip_path.exists());

        let mut cfg = VideoConfig::default();
        cfg.delete_original = true; // explicitly enabled!

        let opts = VideoConvertOptions {
            password: None,
            keep_work_dir_on_error: false,
            dry_run: false,
        };

        let res = process(&zip_path, &cfg, &opts);
        assert_eq!(res.status, "skipped");
        assert_eq!(res.reason, Some(Reason::UnsupportedFileType));

        // Critical invariant: original manga archive must NOT be deleted
        assert!(
            zip_path.exists(),
            "Manga archive must NOT be deleted even when delete_original is true in video plugin"
        );
    }

    #[test]
    fn test_unknown_file_ignored_and_never_deleted() {
        let dir = tempdir().unwrap();
        let unknown_path = dir.path().join("random_notes.txt");
        fs::write(&unknown_path, b"some text notes").unwrap();
        assert!(unknown_path.exists());

        let mut cfg = VideoConfig::default();
        cfg.delete_original = true;

        let opts = VideoConvertOptions {
            password: None,
            keep_work_dir_on_error: false,
            dry_run: false,
        };

        let res = process(&unknown_path, &cfg, &opts);
        assert_eq!(res.status, "skipped");
        assert!(
            unknown_path.exists(),
            "Unknown file must NOT be deleted or moved by video plugin"
        );
    }
}
