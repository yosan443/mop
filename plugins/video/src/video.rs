use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use mop_plugin_common::archive::{self, ExtractLimits, ExtractTracker};
use mop_plugin_common::classify::is_video_file;
use mop_plugin_common::error::Reason;
use mop_plugin_common::paths::{output_base, unique_dest};
use tracing::{info, warn};

use crate::config::VideoConfig;

#[derive(Debug, Clone)]
pub struct VideoConvertOptions {
    pub password: Option<String>,
    pub dry_run: bool,
    pub keep_work_dir_on_error: bool,
}

#[derive(Debug, Clone)]
pub struct VideoResult {
    pub status: &'static str,
    pub reason: Option<Reason>,
    pub output: Option<PathBuf>,
    #[allow(dead_code)]
    pub outputs: Vec<PathBuf>,
    pub error: Option<String>,
}

/// Transcode an individual input video file to HEVC MP4 in dest_dir.
pub fn transcode_single_file(
    input_path: &Path,
    dest_path: &Path,
    work_dir: &Path,
    cfg: &VideoConfig,
    dry_run: bool,
) -> Result<PathBuf, String> {
    if dry_run {
        return Ok(dest_path.to_path_buf());
    }

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create dest dir: {e}"))?;
    }
    fs::create_dir_all(work_dir).map_err(|e| format!("create work dir: {e}"))?;

    let tmp_dest = tempfile::Builder::new()
        .prefix("transcode-")
        .suffix(".mp4")
        .tempfile_in(work_dir)
        .map_err(|e| format!("create tempfile: {e}"))?;
    let tmp_path = tmp_dest.into_temp_path();

    // Command: ffmpeg -y -i <input> -c:v libx265 -profile:v main10 -pix_fmt yuv420p10le
    //                 -preset <preset> -crf <crf> -c:a aac -b:a 192k -movflags +faststart <out>
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-c:v")
        .arg("libx265")
        .arg("-profile:v")
        .arg("main10")
        .arg("-pix_fmt")
        .arg("yuv420p10le")
        .arg("-preset")
        .arg(&cfg.preset)
        .arg("-crf")
        .arg(cfg.crf.to_string())
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("192k")
        .arg("-movflags")
        .arg("+faststart")
        .arg(&tmp_path);

    let output = cmd
        .output()
        .map_err(|e| format!("failed to run ffmpeg: {e}"))?;
    if !output.status.success() {
        let err_str = String::from_utf8_lossy(&output.stderr);
        let last_lines: Vec<&str> = err_str.lines().rev().take(10).collect();
        let summary = last_lines.into_iter().rev().collect::<Vec<_>>().join(" | ");
        return Err(format!("ffmpeg failed: {summary}"));
    }

    // Persist temporary transcode to final dest_path
    tmp_path
        .persist(dest_path)
        .map_err(|e| format!("failed to persist output {}: {e}", dest_path.display()))?;

    Ok(dest_path.to_path_buf())
}

/// Convert a single video or video archive into HEVC MP4 in cfg.video_dir.
pub fn convert_one(input: &Path, cfg: &VideoConfig, opts: &VideoConvertOptions) -> VideoResult {
    if !input.exists() {
        return VideoResult {
            status: "failed",
            reason: Some(Reason::OpenFailed),
            output: None,
            outputs: Vec::new(),
            error: Some(format!("input does not exist: {}", input.display())),
        };
    }

    let input_name = input
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("video");

    // Case 1: Standalone video file
    if is_video_file(input) {
        let stem = input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("video");
        let dest_filename = format!("{stem}.mp4");
        let mut final_dest = cfg.video_dir.join(&dest_filename);

        if final_dest.exists() && !cfg.overwrite {
            final_dest = unique_dest(&final_dest);
        }

        info!(
            "Transcoding video {} -> {}",
            input.display(),
            final_dest.display()
        );

        match transcode_single_file(input, &final_dest, &cfg.work_dir, cfg, opts.dry_run) {
            Ok(out) => {
                if cfg.delete_original && !opts.dry_run {
                    if let Err(e) = fs::remove_file(input) {
                        warn!("Failed to delete original file {}: {e}", input.display());
                    } else {
                        info!("Deleted original file {}", input.display());
                    }
                }
                return VideoResult {
                    status: "success",
                    reason: None,
                    output: Some(out.clone()),
                    outputs: vec![out],
                    error: None,
                };
            }
            Err(e) => {
                return VideoResult {
                    status: "failed",
                    reason: Some(Reason::UnsupportedFileType),
                    output: None,
                    outputs: Vec::new(),
                    error: Some(e),
                };
            }
        }
    }

    // Case 2: Archive potentially containing video(s)
    let work_sub = cfg
        .work_dir
        .join(format!("extract_{}_{}", std::process::id(), input_name));
    if let Err(e) = fs::create_dir_all(&work_sub) {
        return VideoResult {
            status: "failed",
            reason: None,
            output: None,
            outputs: Vec::new(),
            error: Some(format!("create work subdir: {e}")),
        };
    }

    let extract_res = (|| -> anyhow::Result<Vec<PathBuf>> {
        let mut ar = archive::open(input, opts.password.as_deref())
            .context("open archive with libarchive")?;
        let limits = ExtractLimits::default();
        let mut tracker = ExtractTracker::new(&limits);

        archive::extract(
            &mut ar,
            &work_sub,
            &limits,
            opts.password.is_some(),
            &mut tracker,
        )
        .map_err(|e| anyhow::anyhow!("streaming extraction: {e}"))?;

        let mut found_videos = Vec::new();
        for entry in walkdir::WalkDir::new(&work_sub)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let p = entry.path();
            if p.is_file() && is_video_file(p) {
                found_videos.push(p.to_path_buf());
            }
        }
        Ok(found_videos)
    })();

    let extracted_videos = match extract_res {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_dir_all(&work_sub);
            return VideoResult {
                status: "failed",
                reason: Some(Reason::ConvertError),
                output: None,
                outputs: Vec::new(),
                error: Some(format!("archive extract: {e}")),
            };
        }
    };

    if extracted_videos.is_empty() {
        let _ = fs::remove_dir_all(&work_sub);
        return VideoResult {
            status: "skipped",
            reason: Some(Reason::UnsupportedFileType),
            output: None,
            outputs: Vec::new(),
            error: Some("no video files found inside archive".to_string()),
        };
    }

    let archive_base = output_base(input);
    let mut outputs = Vec::new();

    for vid in &extracted_videos {
        let dest_filename = if extracted_videos.len() == 1 {
            format!("{archive_base}.mp4")
        } else {
            let stem = vid.file_stem().and_then(|s| s.to_str()).unwrap_or("part");
            format!("{archive_base}_{stem}.mp4")
        };

        let mut final_dest = cfg.video_dir.join(&dest_filename);
        if final_dest.exists() && !cfg.overwrite {
            final_dest = unique_dest(&final_dest);
        }

        info!(
            "Transcoding extracted video {} -> {}",
            vid.display(),
            final_dest.display()
        );

        match transcode_single_file(vid, &final_dest, &cfg.work_dir, cfg, opts.dry_run) {
            Ok(out) => outputs.push(out),
            Err(e) => {
                if !opts.keep_work_dir_on_error {
                    let _ = fs::remove_dir_all(&work_sub);
                }
                return VideoResult {
                    status: "failed",
                    reason: Some(Reason::UnsupportedFileType),
                    output: None,
                    outputs,
                    error: Some(e),
                };
            }
        }
    }

    let _ = fs::remove_dir_all(&work_sub);

    if cfg.delete_original && !opts.dry_run {
        let _ = fs::remove_file(input);
    }

    let first = outputs.first().cloned();
    VideoResult {
        status: "success",
        reason: None,
        output: first,
        outputs,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_video(path: &Path) {
        let status = Command::new("ffmpeg")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("testsrc=duration=1:size=160x120:rate=1")
            .arg("-c:v")
            .arg("mpeg4")
            .arg(path)
            .output()
            .unwrap();
        assert!(status.status.success());
    }

    #[test]
    fn test_transcode_synthetic_video_to_hevc_mp4() {
        let tmp = tempdir().unwrap();
        let input = tmp.path().join("input.avi");
        create_test_video(&input);

        let mut cfg = VideoConfig::default();
        cfg.video_dir = tmp.path().join("out");
        cfg.work_dir = tmp.path().join("work");
        cfg.preset = "ultrafast".to_string();
        cfg.crf = 28;

        let opts = VideoConvertOptions {
            password: None,
            dry_run: false,
            keep_work_dir_on_error: false,
        };

        let res = convert_one(&input, &cfg, &opts);
        assert_eq!(res.status, "success");
        let out_path = res.output.unwrap();
        assert!(out_path.exists());
        assert_eq!(out_path.extension().and_then(|e| e.to_str()), Some("mp4"));

        // Verify output codec is hevc via ffprobe / ffmpeg
        let probe = Command::new("ffmpeg")
            .arg("-i")
            .arg(&out_path)
            .output()
            .unwrap();
        let probe_str = String::from_utf8_lossy(&probe.stderr);
        assert!(
            probe_str.contains("hevc"),
            "Expected hevc codec in {}",
            probe_str
        );
    }

    #[test]
    fn test_suffix_collision_resolution() {
        let tmp = tempdir().unwrap();
        let input = tmp.path().join("clip.mp4");
        create_test_video(&input);

        let mut cfg = VideoConfig::default();
        cfg.video_dir = tmp.path().join("out");
        cfg.work_dir = tmp.path().join("work");
        cfg.preset = "ultrafast".to_string();
        cfg.crf = 28;
        cfg.overwrite = false;

        fs::create_dir_all(&cfg.video_dir).unwrap();
        let existing = cfg.video_dir.join("clip.mp4");
        fs::write(&existing, b"dummy content").unwrap();

        let opts = VideoConvertOptions {
            password: None,
            dry_run: false,
            keep_work_dir_on_error: false,
        };

        let res = convert_one(&input, &cfg, &opts);
        assert_eq!(res.status, "success");
        let out = res.output.unwrap();
        assert_ne!(out, existing);
        assert!(out.to_string_lossy().contains("clip (1).mp4"));
        assert!(out.exists());
        assert!(existing.exists());
    }
}
