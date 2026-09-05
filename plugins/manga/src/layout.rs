//! Volume-unit detection over an extracted tree, and duplicate-volume
//! merging. Deterministic rules only: no AI, no external data.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tracing::info;
use walkdir::WalkDir;

use crate::comicinfo;
use crate::config::MangaConfig;
use crate::error::ConvertError;
use crate::image;
use crate::title::{self, Confidence, VolumeKey};

/// Where a volume unit's pages live on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // nested archives become directories; Archive kept for future use
pub enum VolumeSource {
    Directory(PathBuf),
    Archive(PathBuf),
}

/// One detected volume unit.
#[derive(Debug, Clone)]
#[allow(dead_code)] // `source` mirrors `dir`; kept for future archive sources
pub struct VolumeUnit {
    pub source: VolumeSource,
    /// Directory that contains the pages (staging root for this volume).
    pub dir: PathBuf,
    /// Folder or archive stem name this unit was detected from.
    pub raw_name: String,
    pub page_count: u32,
    pub byte_size: u64,
    pub volume_key: VolumeKey,
    pub confidence: Confidence,
}

/// Directory names that hold pages one level below a volume folder
/// (`Vol 01/images/...`, `Vol 01/本編/...`); such directories are never
/// volumes themselves.
fn is_image_dir_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "images" | "image" | "pages" | "page" | "scans" | "scan" | "本編"
    )
}

/// Per-directory statistics gathered in one walk.
#[derive(Default)]
struct DirStats {
    /// Image files directly inside this directory.
    direct_images: usize,
    /// Direct subdirectories.
    subdirs: Vec<PathBuf>,
    /// Recursive image count (direct + all descendants).
    total_images: usize,
    /// Recursive byte size of files.
    total_bytes: u64,
}

/// Detect volume units in an already-extracted tree under `root`.
///
/// `outer_stem` names the root unit (flat input) and is used for its
/// `raw_name`. Directories are considered volumes when they hold at least
/// `manga_image_threshold` images directly (or one level below in an
/// image-only subdirectory) and none of their subdirectories already is a
/// volume (wrapper folders).
pub fn find_volumes(
    root: &Path,
    cfg: &MangaConfig,
    outer_stem: &str,
) -> Result<Vec<VolumeUnit>, ConvertError> {
    let threshold = cfg.manga_image_threshold;

    // Pass 1: collect per-directory stats.
    let mut stats: HashMap<PathBuf, DirStats> = HashMap::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|e| ConvertError::Convert(format!("walk error: {e}")))?;
        let ft = entry.file_type();
        if ft.is_dir() {
            let path = entry.path().to_path_buf();
            stats.entry(path.clone()).or_default();
            if let Some(parent) = path.parent() {
                stats
                    .entry(parent.to_path_buf())
                    .or_default()
                    .subdirs
                    .push(path);
            }
        } else if ft.is_file() {
            let path = entry.path().to_path_buf();
            let parent = path.parent().unwrap_or(root).to_path_buf();
            let s = stats.entry(parent).or_default();
            if image::is_image_ext(path.extension().and_then(|e| e.to_str())) {
                s.direct_images += 1;
            }
            s.total_bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }

    // Pass 2: bottom-up (deepest first) recursive totals.
    let mut dirs: Vec<PathBuf> = stats.keys().cloned().collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for dir in &dirs {
        let (direct, subdirs) = {
            let s = &stats[dir];
            (s.direct_images, s.subdirs.clone())
        };
        let mut total_images = direct;
        let mut total_bytes = stats[dir].total_bytes;
        for sub in &subdirs {
            let cs = &stats[sub];
            total_images += cs.total_images;
            total_bytes += cs.total_bytes;
        }
        let s = stats.get_mut(dir).expect("dir present");
        s.total_images = total_images;
        s.total_bytes = total_bytes;
    }

    // Pass 3: bottom-up volume detection.
    let mut units: Vec<VolumeUnit> = Vec::new();
    let mut unit_dirs: HashSet<PathBuf> = HashSet::new();
    for dir in &dirs {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if is_image_dir_name(&name) {
            continue;
        }
        let s = &stats[dir];
        // Images directly in this dir plus image-only subdirs one level down.
        let mut candidate = s.direct_images;
        for sub in &s.subdirs {
            if is_image_dir_name(
                &sub.file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default(),
            ) {
                candidate += stats[sub].total_images;
            }
        }
        let child_is_unit = s.subdirs.iter().any(|c| unit_dirs.contains(c));
        if candidate < threshold || child_is_unit {
            continue;
        }

        let raw_name = if dir == root {
            outer_stem.to_string()
        } else {
            name
        };
        let volume_key = resolve_key(dir, &raw_name);
        let confidence = match volume_key {
            VolumeKey::Unknown => Confidence::Low,
            _ => Confidence::High,
        };
        unit_dirs.insert(dir.clone());
        units.push(VolumeUnit {
            source: VolumeSource::Directory(dir.clone()),
            dir: dir.clone(),
            page_count: s.total_images as u32,
            byte_size: s.total_bytes,
            raw_name,
            volume_key,
            confidence,
        });
    }

    units.sort_by_key(unit_order);
    Ok(units)
}

/// Volume key for a unit: ComicInfo.xml `<Volume>`/`<Number>` wins, otherwise
/// the folder/archive name is parsed.
fn resolve_key(dir: &Path, raw_name: &str) -> VolumeKey {
    if let Some(ci) = comicinfo::read_from_dir(dir) {
        if let Some(n) = ci.volume_number() {
            return VolumeKey::Number(n);
        }
    }
    title::parse_volume(raw_name)
}

/// Deterministic processing order: numbers, then parts, extras, unknowns.
fn unit_order(u: &VolumeUnit) -> (u8, u32, String) {
    match &u.volume_key {
        VolumeKey::Number(n) => (0, *n, String::new()),
        VolumeKey::Part(p) => (1, p.number(), String::new()),
        VolumeKey::Extra { label } => (2, 0, label.clone()),
        VolumeKey::Unknown => (3, 0, u.raw_name.clone()),
    }
}

/// Penalty for duplicate-looking names (scan copies, alt rips).
fn dup_penalty(u: &VolumeUnit) -> usize {
    let lower = u.raw_name.to_lowercase();
    ["コピー", "copy", "scan", "alt", "仮"]
        .iter()
        .any(|k| lower.contains(k)) as usize
}

/// Pick the better of two units: more pages, then more bytes, then less
/// duplicate-looking name.
fn better(a: &VolumeUnit, b: &VolumeUnit) -> std::cmp::Ordering {
    b.page_count
        .cmp(&a.page_count)
        .then(b.byte_size.cmp(&a.byte_size))
        .then(dup_penalty(a).cmp(&dup_penalty(b)))
        .then(a.raw_name.cmp(&b.raw_name))
}

/// Merge units that share a volume key. The winner keeps the most pages (then
/// bytes, then the cleanest name); losers are logged as `status=deduped`.
///
/// Exception: when page counts differ by more than 20% and more than 10
/// pages, the units are treated as distinct releases and all are kept (with
/// low confidence, so they go to `_review`).
pub fn dedupe(mut units: Vec<VolumeUnit>, cfg: &MangaConfig) -> Vec<VolumeUnit> {
    if !cfg.dedupe_volumes {
        return units;
    }

    let mut groups: HashMap<VolumeKey, Vec<VolumeUnit>> = HashMap::new();
    for u in units.drain(..) {
        groups.entry(u.volume_key.clone()).or_default().push(u);
    }

    let mut out = Vec::new();
    for (key, mut group) in groups {
        if group.len() <= 1 {
            out.extend(group);
            continue;
        }

        let max_pages = group.iter().map(|u| u.page_count).max().unwrap_or(0);
        let min_pages = group.iter().map(|u| u.page_count).min().unwrap_or(0);
        let diff = max_pages.saturating_sub(min_pages);
        // Distinct releases (e.g. normal + special edition) are kept apart
        // only when neither candidate looks like a duplicate copy.
        let all_clean = group.iter().all(|u| dup_penalty(u) == 0);
        if all_clean && diff > 10 && diff > max_pages / 5 {
            for u in &mut group {
                u.confidence = Confidence::Low;
            }
            out.extend(group);
            continue;
        }

        group.sort_by(better);
        let winner = group.remove(0);
        for loser in group {
            info!(
                "status=deduped key={:?} volume={} pages={} kept={} pages={}",
                key, loser.raw_name, loser.page_count, winner.raw_name, winner.page_count
            );
        }
        out.push(winner);
    }

    out.sort_by_key(unit_order);
    out
}

/// Longest common prefix of all volume raw names (series-name fallback).
pub fn common_series(units: &[VolumeUnit]) -> String {
    title::longest_common_prefix(units.iter().map(|u| u.raw_name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn png_file() -> Vec<u8> {
        vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
    }

    fn cfg_threshold(n: usize) -> MangaConfig {
        MangaConfig {
            manga_image_threshold: n,
            ..MangaConfig::default()
        }
    }

    fn write_images(dir: &Path, count: usize) {
        fs::create_dir_all(dir).unwrap();
        for i in 0..count {
            fs::write(dir.join(format!("p{i:03}.png")), png_file()).unwrap();
        }
    }

    #[test]
    fn flat_root_is_single_unit() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_images(tmp.path(), 12);
        let units = find_volumes(tmp.path(), &cfg_threshold(10), "アオのハコ").unwrap();
        assert_eq!(units.len(), 1);
        let u = &units[0];
        assert_eq!(u.raw_name, "アオのハコ");
        assert_eq!(u.page_count, 12);
        assert_eq!(u.volume_key, VolumeKey::Unknown);
    }

    #[test]
    fn volume_folders_are_detected() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_images(&tmp.path().join("Vol 01"), 12);
        write_images(&tmp.path().join("Vol 02"), 14);
        write_images(&tmp.path().join("Vol 03"), 11);
        let units = find_volumes(tmp.path(), &cfg_threshold(10), "series").unwrap();
        assert_eq!(units.len(), 3);
        assert_eq!(units[0].volume_key, VolumeKey::Number(1));
        assert_eq!(units[1].volume_key, VolumeKey::Number(2));
        assert_eq!(units[2].volume_key, VolumeKey::Number(3));
        assert_eq!(units[0].page_count, 12);
    }

    #[test]
    fn nested_archive_dirs_are_volumes() {
        // Simulates extracted nested .cbz files: each archive became a dir.
        let tmp = tempfile::TempDir::new().unwrap();
        write_images(&tmp.path().join("v01"), 12);
        write_images(&tmp.path().join("v02"), 12);
        let units = find_volumes(tmp.path(), &cfg_threshold(10), "outer").unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].volume_key, VolumeKey::Number(1));
        assert_eq!(units[1].volume_key, VolumeKey::Number(2));
    }

    #[test]
    fn single_wrapper_is_unwrapped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wrap = tmp.path().join("wrapper");
        write_images(&wrap.join("Vol 01"), 12);
        write_images(&wrap.join("Vol 02"), 12);
        let units = find_volumes(tmp.path(), &cfg_threshold(10), "outer").unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].raw_name, "Vol 01");
    }

    #[test]
    fn image_subdir_counts_toward_parent() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_images(&tmp.path().join("Vol 01").join("images"), 12);
        let units = find_volumes(tmp.path(), &cfg_threshold(10), "outer").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].volume_key, VolumeKey::Number(1));
        assert_eq!(units[0].page_count, 12);
    }

    #[test]
    fn macos_metadata_and_cover_only_folders_ignored() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_images(&tmp.path().join("Vol 01"), 12);
        fs::create_dir_all(tmp.path().join("__MACOSX")).unwrap();
        fs::write(tmp.path().join("__MACOSX").join("._x"), b"x").unwrap();
        fs::write(tmp.path().join(".DS_Store"), b"x").unwrap();
        fs::write(tmp.path().join("cover.jpg"), b"x").unwrap();
        let units = find_volumes(tmp.path(), &cfg_threshold(10), "outer").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].volume_key, VolumeKey::Number(1));
    }

    #[test]
    fn below_threshold_is_not_a_volume() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_images(&tmp.path().join("Vol 01"), 5);
        let units = find_volumes(tmp.path(), &cfg_threshold(10), "outer").unwrap();
        assert!(units.is_empty());
    }

    #[test]
    fn dedupe_keeps_most_pages() {
        let cfg = cfg_threshold(1);
        let tmp = tempfile::TempDir::new().unwrap();
        write_images(&tmp.path().join("Vol 01"), 30);
        write_images(&tmp.path().join("Vol 01 copy"), 12);
        let units = find_volumes(tmp.path(), &cfg, "outer").unwrap();
        assert_eq!(units.len(), 2);
        let deduped = dedupe(units, &cfg);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].page_count, 30);
    }

    #[test]
    fn dedupe_keeps_both_when_pages_differ_greatly() {
        let cfg = cfg_threshold(1);
        let tmp = tempfile::TempDir::new().unwrap();
        write_images(&tmp.path().join("Vol 01"), 40);
        write_images(&tmp.path().join("Vol 01 special"), 100);
        let units = find_volumes(tmp.path(), &cfg, "outer").unwrap();
        assert_eq!(units.len(), 2);
        let deduped = dedupe(units, &cfg);
        assert_eq!(deduped.len(), 2, "distinct releases must both be kept");
        assert!(deduped.iter().all(|u| u.confidence == Confidence::Low));
    }

    #[test]
    fn dedupe_disabled_keeps_all() {
        let mut cfg = cfg_threshold(1);
        cfg.dedupe_volumes = false;
        let tmp = tempfile::TempDir::new().unwrap();
        write_images(&tmp.path().join("Vol 01"), 30);
        write_images(&tmp.path().join("Vol 01 copy"), 12);
        let units = find_volumes(tmp.path(), &cfg, "outer").unwrap();
        let deduped = dedupe(units, &cfg);
        assert_eq!(deduped.len(), 2);
    }
}
