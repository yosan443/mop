use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::error::ConvertError;

fn tmp_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(".mop-manga-tmp-{}-{n}.cbz", std::process::id())
}

/// Build a store-only (no recompression) CBZ from extracted files in `source_dir`
/// and atomically place it as `output_name` in `output_dir`.
pub fn build_cbz(
    source_dir: &Path,
    output_dir: &Path,
    output_name: &str,
    overwrite: bool,
) -> Result<PathBuf, ConvertError> {
    fs::create_dir_all(output_dir)?;
    let final_path = output_dir.join(output_name);
    if final_path.exists() && !overwrite {
        return Err(ConvertError::OutputExists);
    }

    let temp_path = output_dir.join(tmp_name());
    let result = (|| -> Result<(), ConvertError> {
        let file = File::create(&temp_path)?;
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

        let mut entries: Vec<PathBuf> = WalkDir::new(source_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .collect();
        entries.sort();

        for path in entries {
            let rel = path
                .strip_prefix(source_dir)
                .map_err(|_| ConvertError::Convert("internal: entry outside source dir".into()))?;
            let name = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");

            zip.start_file(name, opts)
                .map_err(|e| ConvertError::Convert(e.to_string()))?;
            let mut f = File::open(&path)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            zip.write_all(&buf)
                .map_err(|e| ConvertError::Convert(e.to_string()))?;
        }

        zip.finish()
            .map_err(|e| ConvertError::Convert(e.to_string()))?;
        Ok(())
    })();

    if let Err(e) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(e);
    }

    match fs::rename(&temp_path, &final_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            fs::copy(&temp_path, &final_path)?;
            fs::remove_file(&temp_path)?;
        }
        Err(e) => return Err(e.into()),
    }

    Ok(final_path)
}
