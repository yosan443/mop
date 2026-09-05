use std::fs;
use std::path::Path;

use tracing::{info, warn};

use crate::classify::{self, Kind};
use crate::config::MangaConfig;
use crate::convert::{self, ConvertOptions, ConvertResult};
use crate::error::Reason;

/// Classify an input and run the matching pipeline:
/// - Manga: archive -> CBZ conversion;
/// - Video: skipped (M6 deferred; NEVER delete original);
/// - Unknown: moved to `unknown_dir`.
pub fn process(input: &Path, cfg: &MangaConfig, opts: &ConvertOptions) -> ConvertResult {
    let kind = classify::classify(input, cfg.manga_image_threshold, cfg.remove_macos_metadata);
    info!(
        "status=classified kind={} input={}",
        kind.as_str(),
        input.display()
    );

    match kind {
        Kind::Manga => {
            let res = convert::convert_one(input, cfg, opts);
            delete_original(input, cfg, &res);
            res
        }
        Kind::Video => {
            // Video transcoding is handled by mop.video plugin.
            // IMPORTANT: Never delete the original file on skipped video inputs.
            let res = ConvertResult {
                status: "skipped",
                reason: Some(Reason::UnsupportedFileType),
                output: None,
                outputs: Vec::new(),
                pages: None,
                videos: None,
                error: Some("Video transcoding handled by mop.video plugin".to_string()),
            };
            convert::log_status(&res, input);
            res
        }
        Kind::Unknown => {
            let res = move_unknown(input, cfg, opts.dry_run);
            convert::log_status(&res, input);
            res
        }
    }
}

fn delete_original(input: &Path, cfg: &MangaConfig, res: &ConvertResult) {
    if !cfg.delete_original {
        return;
    }
    let processed = matches!(res.status, "success")
        || (res.status == "skipped" && res.reason == Some(Reason::OutputExists));
    if !processed {
        return;
    }
    match fs::remove_file(input) {
        Ok(()) => info!("status=deleted input={}", input.display()),
        Err(e) => warn!("failed to delete original {}: {e}", input.display()),
    }
}

fn move_unknown(input: &Path, cfg: &MangaConfig, dry_run: bool) -> ConvertResult {
    let dest_dir = &cfg.unknown_dir;
    let file_name = input.file_name().unwrap_or_default();
    let planned = dest_dir.join(file_name);

    if dry_run {
        return ConvertResult {
            status: "success",
            reason: None,
            output: Some(planned),
            outputs: Vec::new(),
            pages: None,
            videos: None,
            error: None,
        };
    }

    if let Err(e) = fs::create_dir_all(dest_dir) {
        return ConvertResult {
            status: "failed",
            reason: Some(Reason::ConvertError),
            output: None,
            outputs: Vec::new(),
            pages: None,
            videos: None,
            error: Some(format!("create unknown_dir: {e}")),
        };
    }

    let target = crate::paths::unique_dest(&planned);
    match crate::paths::move_file(input, &target) {
        Ok(()) => ConvertResult {
            status: "success",
            reason: None,
            output: Some(target),
            outputs: Vec::new(),
            pages: None,
            videos: None,
            error: None,
        },
        Err(e) => ConvertResult {
            status: "failed",
            reason: Some(Reason::ConvertError),
            output: None,
            outputs: Vec::new(),
            pages: None,
            videos: None,
            error: Some(format!("move to unknown_dir: {e}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_video_skipped_and_never_deleted() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("sample_clip.mp4");
        {
            let mut f = File::create(&video_path).unwrap();
            writeln!(f, "fake video data").unwrap();
        }
        assert!(video_path.exists());

        let mut cfg = MangaConfig::default();
        cfg.delete_original = true; // explicitly enabled!

        let opts = ConvertOptions {
            password: None,
            keep_work_dir_on_error: false,
            dry_run: false,
        };

        let res = process(&video_path, &cfg, &opts);
        assert_eq!(res.status, "skipped");
        assert_eq!(res.reason, Some(Reason::UnsupportedFileType));

        // Critical safety invariant: original video file MUST NOT be deleted
        assert!(
            video_path.exists(),
            "Original video file must not be deleted even when delete_original is true"
        );
    }
}
