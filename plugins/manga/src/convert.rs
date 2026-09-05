use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::archive;
use crate::cbz;
use crate::comicinfo;
use crate::config::MangaConfig;
use crate::error::{ConvertError, Reason};
use crate::image;
use crate::layout::{self, VolumeUnit};
use crate::paths;
use crate::title::{self, VolumeKey};

/// Per-invocation options for a single conversion job.
pub struct ConvertOptions {
    pub password: Option<String>,
    pub keep_work_dir_on_error: bool,
    pub dry_run: bool,
}

/// Outcome of a single conversion job, ready for status logging.
pub struct ConvertResult {
    pub status: &'static str,
    pub reason: Option<Reason>,
    pub output: Option<PathBuf>,
    pub outputs: Vec<PathBuf>,
    pub pages: Option<u32>,
    pub videos: Option<u32>,
    pub error: Option<String>,
}

/// Per-volume result inside one job.
pub enum VolumeOutcome {
    Built(PathBuf, u32),
    Skipped(PathBuf),
}

/// Log a greppable, journald-friendly status line.
pub fn log_status(result: &ConvertResult, input: &Path) {
    let mut line = format!("status={} input={}", result.status, input.display());
    if let Some(r) = result.reason {
        line.push_str(&format!(" reason={}", r.as_str()));
    }
    if let Some(out) = &result.output {
        line.push_str(&format!(" output={}", out.display()));
    }
    if !result.outputs.is_empty() {
        let joined = result
            .outputs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(",");
        line.push_str(&format!(" outputs=[{joined}]"));
    }
    if let Some(p) = result.pages {
        line.push_str(&format!(" pages={p}"));
    }
    if let Some(v) = result.videos {
        line.push_str(&format!(" videos={v}"));
    }
    if let Some(e) = &result.error {
        line.push_str(&format!(" error=\"{e}\""));
    }
    info!("{line}");
}

/// Convert a single archive to one or more WebP-based CBZs.
pub fn convert_one(input: &Path, cfg: &MangaConfig, opts: &ConvertOptions) -> ConvertResult {
    debug!("status=started input={}", input.display());

    let result =
        |status, reason: Option<Reason>, output, outputs: Vec<PathBuf>, pages, videos, error| {
            ConvertResult {
                status,
                reason,
                output,
                outputs,
                pages,
                videos,
                error,
            }
        };

    let outcome = run(input, cfg, opts);
    let res = match outcome {
        Err(ConvertError::OutputExists) => result(
            "skipped",
            Some(Reason::OutputExists),
            Some(output_path(input, cfg)),
            Vec::new(),
            None,
            None,
            None,
        ),
        Err(ConvertError::PasswordRequired) => result(
            "skipped",
            Some(Reason::PasswordRequired),
            None,
            Vec::new(),
            None,
            None,
            None,
        ),
        Err(e) => {
            let reason = e.reason();
            result(
                "failed",
                Some(reason),
                None,
                Vec::new(),
                None,
                None,
                Some(e.to_string()),
            )
        }
        Ok(outcomes) => {
            let built: Vec<PathBuf> = outcomes
                .iter()
                .filter_map(|o| match o {
                    VolumeOutcome::Built(p, _) => Some(p.clone()),
                    VolumeOutcome::Skipped(_) => None,
                })
                .collect();
            let skipped: Vec<PathBuf> = outcomes
                .iter()
                .filter_map(|o| match o {
                    VolumeOutcome::Skipped(p) => Some(p.clone()),
                    VolumeOutcome::Built(_, _) => None,
                })
                .collect();
            let total_pages: u32 = outcomes
                .iter()
                .filter_map(|o| match o {
                    VolumeOutcome::Built(_, p) => Some(*p),
                    VolumeOutcome::Skipped(_) => None,
                })
                .sum();
            if built.is_empty() {
                result(
                    "skipped",
                    Some(Reason::OutputExists),
                    None,
                    skipped,
                    None,
                    None,
                    None,
                )
            } else {
                result(
                    "success",
                    None,
                    built.first().cloned(),
                    built,
                    Some(total_pages),
                    None,
                    None,
                )
            }
        }
    };
    log_status(&res, input);
    res
}

fn output_path(input: &Path, cfg: &MangaConfig) -> PathBuf {
    cfg.output_dir.join(paths::output_stem(input))
}

fn run(
    input: &Path,
    cfg: &MangaConfig,
    opts: &ConvertOptions,
) -> Result<Vec<VolumeOutcome>, ConvertError> {
    // Validate the input is a regular file.
    let meta = fs::metadata(input)?;
    if !meta.is_file() {
        return Err(ConvertError::UnsupportedFileType(format!(
            "{} is not a regular file",
            input.display()
        )));
    }

    // Input size guard.
    if meta.len() > cfg.max_input_size_gib * (1 << 30) {
        return Err(ConvertError::LimitExceeded(format!(
            "input exceeds {} GiB",
            cfg.max_input_size_gib
        )));
    }

    let outer_stem = paths::output_base(input);

    if !cfg.split_volumes {
        return run_legacy(input, cfg, opts, &outer_stem);
    }

    let has_password = opts.password.is_some();

    if opts.dry_run {
        return dry_run(input, cfg, has_password, &outer_stem);
    }

    // Unique temp work dir under cfg.work_dir.
    fs::create_dir_all(&cfg.work_dir)?;
    let temp = tempfile::Builder::new()
        .prefix("job-")
        .tempdir_in(&cfg.work_dir)
        .map_err(ConvertError::Io)?;
    let work = temp.keep();

    let cleanup = |keep: bool| {
        if !keep {
            let _ = fs::remove_dir_all(&work);
        }
    };

    // Open archive (with passphrase if provided).
    let mut archive = match archive::open(input, opts.password.as_deref()) {
        Ok(a) => a,
        Err(e) => {
            cleanup(false);
            return Err(e);
        }
    };

    // Cumulative limits across outer + nested extraction.
    let limits = cfg.as_extract_limits();
    let mut tracker = archive::ExtractTracker::new(&limits);

    // Extract to <work>/raw preserving structure.
    let raw = work.join("raw");
    fs::create_dir_all(&raw)?;
    let extracted = match archive::extract(&mut archive, &raw, &limits, has_password, &mut tracker)
    {
        Ok(r) => r,
        Err(e) => {
            cleanup(opts.keep_work_dir_on_error);
            return Err(e);
        }
    };

    if extracted.file_count == 0 {
        cleanup(false);
        return Err(ConvertError::Convert(format!(
            "{}: no files extracted",
            input.display()
        )));
    }

    // Peel single-directory wrappers (repeatedly).
    let mut root = raw;
    unwrap_wrappers(&mut root)?;

    // Re-expand nested archives (depth-limited, cumulative limits).
    unpack_nested(&root, cfg, &mut tracker, cfg.max_nested_extract_depth)?;

    // Detect volume units.
    let units = layout::find_volumes(&root, cfg, &outer_stem)?;
    if units.is_empty() {
        cleanup(false);
        return Err(ConvertError::Convert(format!(
            "{}: no volume units found in extracted tree",
            input.display()
        )));
    }
    let units = layout::dedupe(units, cfg);

    // Series name and output roots.
    let series = resolve_series(&outer_stem, &units);
    let series_root = if cfg.series_subdir {
        cfg.output_dir.join(&series)
    } else {
        cfg.output_dir.clone()
    };
    let review_root = cfg
        .output_dir
        .join("_review")
        .join(paths::sanitize_filename(&outer_stem));

    // Build each volume; on failure remove everything this job wrote.
    let mut outcomes: Vec<VolumeOutcome> = Vec::new();
    let mut failed: Option<ConvertError> = None;
    for (i, unit) in units.iter().enumerate() {
        let staging = work.join(format!("staging-{i}"));
        fs::create_dir_all(&staging)?;

        let mut pages: u32 = 0;
        if let Err(e) = stage_files(&unit.dir, &staging, cfg, &mut pages) {
            failed = Some(e);
            break;
        }

        // Carry the outer ComicInfo.xml into the volume, then fill in
        // Series/Volume/Title.
        let _ = comicinfo::copy_if_present(&root, &staging)?;
        let file_name = title::volume_file_name(&series, &unit.volume_key, &unit.raw_name);
        let display_title = file_name.trim_end_matches(".cbz");
        let volume_number = match unit.volume_key {
            VolumeKey::Number(n) => Some(n),
            VolumeKey::Part(p) => Some(p.number()),
            _ => None,
        };
        if let Err(e) = comicinfo::ensure(&staging, &series, volume_number, display_title) {
            failed = Some(e);
            break;
        }

        let dest_dir = if low_confidence_unit(unit, &units) {
            review_root.clone()
        } else {
            series_root.clone()
        };
        match cbz::build_cbz(&staging, &dest_dir, &file_name, cfg.overwrite) {
            Ok(path) => {
                info!(
                    "status=volume input={} output={} pages={}",
                    input.display(),
                    path.display(),
                    pages
                );
                outcomes.push(VolumeOutcome::Built(path, pages));
            }
            Err(ConvertError::OutputExists) => {
                info!(
                    "status=skipped input={} output={} reason=output_exists",
                    input.display(),
                    dest_dir.join(&file_name).display()
                );
                outcomes.push(VolumeOutcome::Skipped(dest_dir.join(file_name)));
            }
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }

    if let Some(e) = failed {
        remove_written(&outcomes);
        cleanup(opts.keep_work_dir_on_error);
        return Err(e);
    }

    cleanup(false);
    Ok(outcomes)
}

/// A unit goes to `_review/<outer-stem>/` when it is low confidence and the
/// job has more than one volume (a single flat volume stays in the series
/// folder so plain inputs keep their normal output path).
fn low_confidence_unit(unit: &VolumeUnit, units: &[VolumeUnit]) -> bool {
    units.len() > 1 && unit.confidence == title::Confidence::Low
}

fn remove_written(outcomes: &[VolumeOutcome]) {
    for o in outcomes {
        if let VolumeOutcome::Built(p, _) = o {
            let _ = fs::remove_file(p);
        }
    }
}

/// Strip single-directory wrappers: while the root contains no files and
/// exactly one subdirectory, descend into it.
fn unwrap_wrappers(root: &mut PathBuf) -> Result<(), ConvertError> {
    loop {
        let mut files = 0usize;
        let mut dirs = Vec::new();
        for entry in fs::read_dir(&*root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if archive::is_macos_metadata(&name) {
                continue;
            }
            let ft = entry.file_type()?;
            if ft.is_dir() {
                dirs.push(entry.path());
            } else if ft.is_file() {
                files += 1;
            }
        }
        if files == 0 && dirs.len() == 1 {
            *root = dirs.pop().expect("one dir");
        } else {
            return Ok(());
        }
    }
}

/// Re-expand archives inside the extracted tree, depth-limited. Each nested
/// archive is replaced by a directory of the same stem. Cumulative limits are
/// shared through `tracker`. Corrupt/non-archive files are skipped with a
/// warning; safety errors (limits, unsafe paths, passwords) abort the job.
fn unpack_nested(
    root: &Path,
    cfg: &MangaConfig,
    tracker: &mut archive::ExtractTracker,
    depth: usize,
) -> Result<(), ConvertError> {
    if depth == 0 {
        return Ok(());
    }
    let archives: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && paths::has_archive_extension(e.path()))
        .map(|e| e.into_path())
        .collect();

    for ar in archives {
        let mut handle = match archive::open(&ar, None) {
            Ok(a) => a,
            Err(e) => {
                warn!("skipping nested archive {}: {e}", ar.display());
                continue;
            }
        };
        let parent = ar.parent().unwrap_or(root).to_path_buf();
        let stem = paths::output_base(&ar);
        let dest = paths::unique_dest(&parent.join(stem));
        fs::create_dir_all(&dest)?;
        let limits = cfg.as_extract_limits();
        match archive::extract(&mut handle, &dest, &limits, false, tracker) {
            Ok(_) => {
                debug!(
                    "unpacked nested archive {} -> {}",
                    ar.display(),
                    dest.display()
                );
                if let Err(e) = fs::remove_file(&ar) {
                    warn!("failed to remove nested archive {}: {e}", ar.display());
                }
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&dest);
                match e {
                    ConvertError::LimitExceeded(_)
                    | ConvertError::UnsafePath(_)
                    | ConvertError::PasswordRequired => return Err(e),
                    other => warn!("skipping nested archive {}: {other}", ar.display()),
                }
            }
        }
    }

    unpack_nested(root, cfg, tracker, depth - 1)
}

/// Series name: outer input stem with its volume token stripped; fall back to
/// the longest common prefix of the detected volume names; finally the raw
/// outer stem.
fn resolve_series(outer_stem: &str, units: &[VolumeUnit]) -> String {
    let candidate = title::strip_volume_token(outer_stem);
    if !candidate.trim().is_empty() {
        return paths::sanitize_filename(&candidate);
    }
    let common = layout::common_series(units);
    let candidate2 = title::strip_volume_token(&common);
    if !candidate2.trim().is_empty() {
        return paths::sanitize_filename(&candidate2);
    }
    paths::sanitize_filename(outer_stem)
}

/// Legacy path: 1 input -> 1 CBZ at `output_dir/<stem>.cbz` (flat staging).
fn run_legacy(
    input: &Path,
    cfg: &MangaConfig,
    opts: &ConvertOptions,
    outer_stem: &str,
) -> Result<Vec<VolumeOutcome>, ConvertError> {
    let final_path = output_path(input, cfg);

    // Early output-exists check (cheap; avoids wasted CPU).
    if final_path.exists() && !cfg.overwrite {
        return Err(ConvertError::OutputExists);
    }

    let has_password = opts.password.is_some();

    if opts.dry_run {
        return dry_run(input, cfg, has_password, outer_stem);
    }

    fs::create_dir_all(&cfg.work_dir)?;
    let temp = tempfile::Builder::new()
        .prefix("job-")
        .tempdir_in(&cfg.work_dir)
        .map_err(ConvertError::Io)?;
    let work = temp.keep();

    let cleanup = |keep: bool| {
        if !keep {
            let _ = fs::remove_dir_all(&work);
        }
    };

    let mut archive = match archive::open(input, opts.password.as_deref()) {
        Ok(a) => a,
        Err(e) => {
            cleanup(false);
            return Err(e);
        }
    };

    let raw = work.join("raw");
    fs::create_dir_all(&raw)?;
    let limits = cfg.as_extract_limits();
    let mut tracker = archive::ExtractTracker::new(&limits);
    let extracted = match archive::extract(&mut archive, &raw, &limits, has_password, &mut tracker)
    {
        Ok(r) => r,
        Err(e) => {
            cleanup(opts.keep_work_dir_on_error);
            return Err(e);
        }
    };

    if extracted.file_count == 0 {
        cleanup(false);
        return Err(ConvertError::Convert(format!(
            "{}: no files extracted",
            input.display()
        )));
    }

    let staging = work.join("staging");
    fs::create_dir_all(&staging)?;

    let mut pages: u32 = 0;
    if let Err(e) = stage_files(&raw, &staging, cfg, &mut pages) {
        cleanup(opts.keep_work_dir_on_error);
        return Err(e);
    }

    let out_name = paths::output_stem(input);
    let built = cbz::build_cbz(&staging, &cfg.output_dir, &out_name, cfg.overwrite);
    cleanup(false);
    let final_path = built?;

    Ok(vec![VolumeOutcome::Built(final_path, pages)])
}

/// Walk the raw extracted tree and produce the staged tree that becomes the
/// CBZ. Returns an error if any image fails to transcode.
fn stage_files(
    raw: &Path,
    staging: &Path,
    cfg: &MangaConfig,
    pages: &mut u32,
) -> Result<(), ConvertError> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(raw)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    files.sort();

    for path in files {
        let rel = path
            .strip_prefix(raw)
            .map_err(|_| ConvertError::Convert("internal: path outside raw tree".into()))?;
        let ext = rel.extension().map(|e| e.to_string_lossy().into_owned());
        let is_comicinfo = rel
            .file_name()
            .map(|f| f.to_string_lossy().eq_ignore_ascii_case("comicinfo.xml"))
            .unwrap_or(false);

        let keep = is_comicinfo || cfg.keep_non_images;
        // Every image (including already-WebP input) is re-encoded via libvips
        // at the configured quality, so webp_quality always applies.
        let dest_rel = if image::is_image_ext(ext.as_deref()) {
            rel.with_extension("webp")
        } else if keep {
            rel.to_path_buf()
        } else {
            // images_only: drop non-image, non-ComicInfo files.
            continue;
        };

        let src_path = &path;
        let dest_path = staging.join(&dest_rel);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if image::is_image_ext(ext.as_deref()) {
            image::transcode_to_webp(src_path, &dest_path, cfg.webp_quality, cfg.lossless)?;
            *pages += 1;
        } else {
            fs::copy(src_path, &dest_path)?;
        }
    }
    Ok(())
}

/// Validate an archive and report what would be done, writing nothing.
fn dry_run(
    input: &Path,
    cfg: &MangaConfig,
    has_password: bool,
    outer_stem: &str,
) -> Result<Vec<VolumeOutcome>, ConvertError> {
    let mut archive = archive::open(input, None)?;
    let mut count = 0usize;
    let mut encrypted = false;
    let mut image_count = 0usize;
    loop {
        let entry = match archive.next_entry() {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(e) => return Err(ConvertError::Convert(format!("reading entry: {e}"))),
        };
        let name = entry.pathname().unwrap_or_default();
        if cfg.remove_macos_metadata && name == "__MACOSX" || name.starts_with("__MACOSX/") {
            continue;
        }
        if entry.is_encrypted() {
            encrypted = true;
        }
        if image::is_image_ext(Path::new(&name).extension().and_then(|e| e.to_str())) {
            image_count += 1;
        }
        count += 1;
    }

    if encrypted && !has_password {
        return Err(ConvertError::PasswordRequired);
    }

    let out = if cfg.split_volumes && cfg.series_subdir {
        let series = resolve_series_for_dry_run(outer_stem);
        cfg.output_dir.join(&series).join(format!("{series}.cbz"))
    } else {
        cfg.output_dir.join(paths::output_stem(input))
    };
    info!(
        "dry_run input={} entries={} images={} output={}",
        input.display(),
        count,
        image_count,
        out.display()
    );
    Ok(vec![VolumeOutcome::Built(out, image_count as u32)])
}

fn resolve_series_for_dry_run(outer_stem: &str) -> String {
    let candidate = title::strip_volume_token(outer_stem);
    if !candidate.trim().is_empty() {
        return paths::sanitize_filename(&candidate);
    }
    paths::sanitize_filename(outer_stem)
}
