use std::env;
use std::path::PathBuf;

pub use mop_plugin_common::paths::normalize;
use serde::{Deserialize, Serialize};

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_watch_dir() -> PathBuf {
    home_dir().join("video")
}

fn default_video_dir() -> PathBuf {
    home_dir().join("video-mp4")
}

fn default_work_dir() -> PathBuf {
    home_dir().join(".cache").join("mop-video")
}

/// Runtime configuration for mop-plugin-video.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoConfig {
    pub watch_dirs: Vec<PathBuf>,
    pub video_dir: PathBuf,
    pub work_dir: PathBuf,
    pub workers: usize,
    pub preset: String,
    pub crf: u32,
    pub delete_original: bool,
    pub overwrite: bool,
    pub scan_on_start: bool,
}

impl Default for VideoConfig {
    fn default() -> Self {
        VideoConfig {
            watch_dirs: vec![default_watch_dir()],
            video_dir: default_video_dir(),
            work_dir: default_work_dir(),
            workers: 1,
            preset: "medium".to_string(),
            crf: 23,
            delete_original: false,
            overwrite: false,
            scan_on_start: true,
        }
    }
}

impl VideoConfig {
    pub fn validate_layout(&self) -> Result<(), String> {
        if self.watch_dirs.is_empty() {
            return Err("At least one watch_dir must be configured".to_string());
        }
        if self.workers == 0 {
            return Err("workers must be at least 1".to_string());
        }

        // Mutual non-overlap between watch_dirs and [video_dir, work_dir]
        for out in [&self.video_dir, &self.work_dir] {
            let out_norm = normalize(out);
            for w in &self.watch_dirs {
                let w_norm = normalize(w);
                if out_norm.starts_with(&w_norm) {
                    return Err(format!(
                        "directory ({}) is inside watch dir ({}); refusing to avoid a reprocessing loop",
                        out_norm.display(),
                        w_norm.display()
                    ));
                }
                if w_norm.starts_with(&out_norm) {
                    return Err(format!(
                        "watch dir ({}) is inside directory ({}); refusing to avoid a reprocessing loop",
                        w_norm.display(),
                        out_norm.display()
                    ));
                }
            }
        }

        // Mutual non-overlap between video_dir and work_dir
        let vid_norm = normalize(&self.video_dir);
        let wrk_norm = normalize(&self.work_dir);
        if vid_norm.starts_with(&wrk_norm) || wrk_norm.starts_with(&vid_norm) {
            return Err(format!(
                "video_dir ({}) and work_dir ({}) must not overlap",
                vid_norm.display(),
                wrk_norm.display()
            ));
        }

        Ok(())
    }

    /// JSON Schema definition for config.schema RPC
    pub fn json_schema() -> serde_json::Value {
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "VideoConfig",
            "type": "object",
            "properties": {
                "watch_dirs": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Directories monitored for incoming video files and archives"
                },
                "video_dir": {
                    "type": "string",
                    "description": "Destination directory for transcoded HEVC MP4 videos"
                },
                "work_dir": {
                    "type": "string",
                    "description": "Temporary extraction and processing directory"
                },
                "workers": {
                    "type": "integer",
                    "minimum": 1,
                    "default": 1,
                    "description": "Number of concurrent transcode workers"
                },
                "preset": {
                    "type": "string",
                    "default": "medium",
                    "description": "x265 encoding preset (e.g. ultrafast, fast, medium, slow)"
                },
                "crf": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 51,
                    "default": 23,
                    "description": "Constant Rate Factor (0-51, lower means higher quality)"
                },
                "delete_original": {
                    "type": "boolean",
                    "default": false,
                    "description": "Delete source video after successful transcode"
                },
                "overwrite": {
                    "type": "boolean",
                    "default": false,
                    "description": "Overwrite destination video if it already exists"
                },
                "scan_on_start": {
                    "type": "boolean",
                    "default": true,
                    "description": "Scan watch_dirs on startup for existing videos"
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_layout_overlap_rejection() {
        let mut cfg = VideoConfig::default();
        cfg.watch_dirs = vec![PathBuf::from("/srv/videos")];
        cfg.video_dir = PathBuf::from("/srv/videos/mp4"); // nested inside watch!
        assert!(cfg.validate_layout().is_err());

        cfg.watch_dirs = vec![PathBuf::from("/srv/videos/incoming")];
        cfg.video_dir = PathBuf::from("/srv/videos"); // watch nested inside video_dir!
        assert!(cfg.validate_layout().is_err());

        cfg.watch_dirs = vec![PathBuf::from("/srv/videos/incoming")];
        cfg.video_dir = PathBuf::from("/srv/videos/transcoded"); // disjoint siblings
        assert!(cfg.validate_layout().is_ok());
    }
}
