use std::env;
use std::path::PathBuf;

use mop_plugin_common::archive::ExtractLimits;
pub use mop_plugin_common::paths::{lexiclean, normalize};
use serde::{Deserialize, Serialize};

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_watch_dir() -> PathBuf {
    home_dir().join("manga")
}

fn default_output_dir() -> PathBuf {
    home_dir().join("manga-cbz")
}

fn default_unknown_dir() -> PathBuf {
    home_dir().join("manga-unknown")
}

fn default_work_dir() -> PathBuf {
    home_dir().join(".cache").join("manga2cbz")
}

/// Resolved runtime configuration for mop-plugin-manga.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MangaConfig {
    pub watch_dirs: Vec<PathBuf>,
    pub output_dir: PathBuf,
    pub unknown_dir: PathBuf,
    pub work_dir: PathBuf,
    pub workers: usize,
    pub webp_quality: i32,
    pub lossless: bool,
    pub keep_non_images: bool,
    pub remove_macos_metadata: bool,
    pub overwrite: bool,
    pub images_only: bool,
    pub dry_run: bool,
    pub manga_image_threshold: usize,
    pub delete_original: bool,
    pub series_subdir: bool,
    pub split_volumes: bool,
    pub dedupe_volumes: bool,
    pub max_nested_extract_depth: usize,
    pub max_input_size_gib: u64,
    pub max_extracted_size_gib: u64,
    pub max_file_count: usize,
    pub reject_symlinks: bool,
    pub scan_on_start: bool,
}

impl Default for MangaConfig {
    fn default() -> Self {
        MangaConfig {
            watch_dirs: vec![default_watch_dir()],
            output_dir: default_output_dir(),
            unknown_dir: default_unknown_dir(),
            work_dir: default_work_dir(),
            workers: 2,
            webp_quality: 92,
            lossless: false,
            keep_non_images: true,
            remove_macos_metadata: true,
            overwrite: false,
            images_only: false,
            dry_run: false,
            manga_image_threshold: 5,
            delete_original: false,
            series_subdir: true,
            split_volumes: true,
            dedupe_volumes: true,
            max_nested_extract_depth: 3,
            max_input_size_gib: 20,
            max_extracted_size_gib: 100,
            max_file_count: 100_000,
            reject_symlinks: true,
            scan_on_start: true,
        }
    }
}

impl MangaConfig {
    pub fn as_extract_limits(&self) -> ExtractLimits {
        ExtractLimits {
            max_extracted_size_gib: self.max_extracted_size_gib,
            max_file_count: self.max_file_count,
            reject_symlinks: self.reject_symlinks,
            remove_macos_metadata: self.remove_macos_metadata,
        }
    }
}

impl From<&MangaConfig> for ExtractLimits {
    fn from(cfg: &MangaConfig) -> Self {
        cfg.as_extract_limits()
    }
}

impl MangaConfig {
    pub fn validate_layout(&self) -> Result<(), String> {
        if self.watch_dirs.is_empty() {
            return Err("At least one watch_dir must be configured".to_string());
        }
        if self.webp_quality < 1 || self.webp_quality > 100 {
            return Err(format!(
                "webp_quality must be between 1 and 100, got {}",
                self.webp_quality
            ));
        }
        if self.workers == 0 {
            return Err("workers must be at least 1".to_string());
        }

        // Mutual non-overlap between watch_dirs and [output_dir, unknown_dir, work_dir]
        for out in [&self.output_dir, &self.unknown_dir, &self.work_dir] {
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

        // Mutual non-overlap between output_dir and unknown_dir
        let out_norm = normalize(&self.output_dir);
        let unk_norm = normalize(&self.unknown_dir);
        if out_norm.starts_with(&unk_norm) || unk_norm.starts_with(&out_norm) {
            return Err(format!(
                "output_dir ({}) and unknown_dir ({}) must not overlap",
                out_norm.display(),
                unk_norm.display()
            ));
        }

        Ok(())
    }

    /// JSON Schema definition for config.schema RPC
    pub fn json_schema() -> serde_json::Value {
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "MangaConfig",
            "type": "object",
            "properties": {
                "watch_dirs": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Directories monitored for incoming manga archives"
                },
                "output_dir": {
                    "type": "string",
                    "description": "Destination directory for converted CBZ archives"
                },
                "unknown_dir": {
                    "type": "string",
                    "description": "Destination directory for unrecognized files"
                },
                "work_dir": {
                    "type": "string",
                    "description": "Temporary extraction and staging directory"
                },
                "workers": {
                    "type": "integer",
                    "minimum": 1,
                    "default": 2,
                    "description": "Number of concurrent conversion threads"
                },
                "webp_quality": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "default": 92,
                    "description": "Lossy WebP compression quality (1-100)"
                },
                "lossless": {
                    "type": "boolean",
                    "default": false,
                    "description": "Encode images using lossless WebP compression"
                },
                "images_only": {
                    "type": "boolean",
                    "default": false,
                    "description": "Drop non-image files from output CBZ"
                },
                "keep_non_images": {
                    "type": "boolean",
                    "default": true,
                    "description": "Keep non-image files inside the output CBZ"
                },
                "delete_original": {
                    "type": "boolean",
                    "default": false,
                    "description": "Delete source archive after successful conversion"
                },
                "manga_image_threshold": {
                    "type": "integer",
                    "default": 5,
                    "description": "Minimum image count to classify archive as manga"
                },
                "overwrite": {
                    "type": "boolean",
                    "default": false,
                    "description": "Overwrite destination CBZ if it already exists"
                },
                "scan_on_start": {
                    "type": "boolean",
                    "default": true,
                    "description": "Scan watch_dirs on startup for existing files"
                },
                "reject_symlinks": {
                    "type": "boolean",
                    "default": true,
                    "description": "Reject archives containing symbolic links"
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
        let mut cfg = MangaConfig::default();
        cfg.watch_dirs = vec![PathBuf::from("/srv/manga")];
        cfg.output_dir = PathBuf::from("/srv/manga/cbz"); // nested inside watch!
        assert!(cfg.validate_layout().is_err());

        cfg.watch_dirs = vec![PathBuf::from("/srv/manga/incoming")];
        cfg.output_dir = PathBuf::from("/srv/manga"); // watch nested inside output!
        assert!(cfg.validate_layout().is_err());

        cfg.watch_dirs = vec![PathBuf::from("/srv/manga/incoming")];
        cfg.output_dir = PathBuf::from("/srv/manga/cbz"); // disjoint siblings
        assert!(cfg.validate_layout().is_ok());
    }
}
