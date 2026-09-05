use std::fs;
use std::io::Write;
use std::path::Path;

use libarchive2::FileType;
use libarchive2::ReadArchive;

use crate::error::ConvertError;
use crate::paths::safe_join;

const STREAM_BUF_SIZE: usize = 1024 * 1024; // 1 MiB

pub fn open(path: &Path, passphrase: Option<&str>) -> Result<ReadArchive<'static>, ConvertError> {
    let res = match passphrase {
        Some(pw) => ReadArchive::open_with_passphrase(path, pw),
        None => ReadArchive::open(path),
    };
    res.map_err(|e| ConvertError::Open(format!("{}: {e}", path.display())))
}

#[derive(Debug, Clone)]
pub struct ExtractLimits {
    pub max_extracted_size_gib: u64,
    pub max_file_count: usize,
    pub reject_symlinks: bool,
    pub remove_macos_metadata: bool,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_extracted_size_gib: 20,
            max_file_count: 5000,
            reject_symlinks: true,
            remove_macos_metadata: true,
        }
    }
}

#[derive(Debug)]
pub struct ExtractResult {
    pub file_count: usize,
}

#[derive(Debug)]
pub struct ExtractTracker {
    pub total_bytes: u64,
    pub file_count: usize,
    pub max_bytes: u64,
    pub max_files: usize,
}

impl ExtractTracker {
    pub fn new(limits: &ExtractLimits) -> Self {
        ExtractTracker {
            total_bytes: 0,
            file_count: 0,
            max_bytes: limits.max_extracted_size_gib.saturating_mul(1 << 30),
            max_files: limits.max_file_count,
        }
    }
}

pub fn is_macos_metadata(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    lower == "__macosx"
        || lower.starts_with("__macosx/")
        || base == ".ds_store"
        || base == ".appledouble"
        || base.starts_with("._")
}

pub fn extract(
    archive: &mut ReadArchive<'static>,
    dest: &Path,
    limits: &ExtractLimits,
    has_password: bool,
    tracker: &mut ExtractTracker,
) -> Result<ExtractResult, ConvertError> {
    let mut file_count: usize = 0;
    let mut encrypted_seen = false;
    let mut buf = vec![0u8; STREAM_BUF_SIZE];

    loop {
        let entry = match archive.next_entry() {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(e) => return Err(ConvertError::Convert(format!("reading entry header: {e}"))),
        };

        let name = entry.pathname().unwrap_or_default();

        if limits.remove_macos_metadata && is_macos_metadata(&name) {
            continue;
        }

        match entry.file_type() {
            FileType::Directory => continue,
            FileType::RegularFile => {}
            FileType::SymbolicLink => {
                if limits.reject_symlinks {
                    return Err(ConvertError::UnsafePath(format!(
                        "symlink in archive: {name:?}"
                    )));
                }
                continue;
            }
            other => {
                return Err(ConvertError::UnsupportedFileType(format!(
                    "{name:?}: {other:?}"
                )));
            }
        }

        if entry.is_encrypted() {
            encrypted_seen = true;
            if !has_password {
                return Err(ConvertError::PasswordRequired);
            }
        }

        if tracker.file_count >= tracker.max_files {
            return Err(ConvertError::LimitExceeded(format!(
                "entry count would exceed {}",
                tracker.max_files
            )));
        }

        let out_path = safe_join(dest, &name)?;
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = match fs::File::create(&out_path) {
            Ok(f) => f,
            Err(e) => return Err(ConvertError::Io(e)),
        };

        let write_result = (|| -> Result<(), ConvertError> {
            loop {
                let n = match archive.read_data(&mut buf) {
                    Ok(n) => n,
                    Err(e) => {
                        let msg = e.to_string();
                        if encrypted_seen && has_password {
                            return Err(ConvertError::PasswordInvalid(msg));
                        }
                        return Err(ConvertError::Convert(format!(
                            "reading data for {name:?}: {msg}"
                        )));
                    }
                };
                if n == 0 {
                    break;
                }
                tracker.total_bytes = tracker.total_bytes.saturating_add(n as u64);
                if tracker.total_bytes > tracker.max_bytes {
                    return Err(ConvertError::LimitExceeded(format!(
                        "extracted size exceeded limit ({} bytes)",
                        tracker.total_bytes
                    )));
                }
                file.write_all(&buf[..n])?;
            }
            Ok(())
        })();

        if let Err(e) = write_result {
            let _ = fs::remove_file(&out_path);
            return Err(e);
        }

        file_count += 1;
        tracker.file_count += 1;
    }

    Ok(ExtractResult { file_count })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_zip(path: &Path, files: &[(&str, Vec<u8>)]) {
        use zip::write::SimpleFileOptions;
        use zip::{CompressionMethod, ZipWriter};
        let f = fs::File::create(path).unwrap();
        let mut zip = ZipWriter::new(f);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, data) in files {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn stream_extracts_multichunk_file_byte_exact() {
        let data: Vec<u8> = (0..=255u8).cycle().take(2 * 1024 * 1024).collect();
        let tmp = tempfile::TempDir::new().unwrap();
        let zip_path = tmp.path().join("test.cbz");
        build_zip(&zip_path, &[("a.bin", data.clone())]);

        let mut archive = open(&zip_path, None).unwrap();
        let dest = tmp.path().join("raw");
        fs::create_dir_all(&dest).unwrap();

        let limits = ExtractLimits::default();
        let mut tracker = ExtractTracker::new(&limits);
        let res = extract(&mut archive, &dest, &limits, false, &mut tracker).unwrap();
        assert_eq!(res.file_count, 1);
        assert_eq!(fs::read(dest.join("a.bin")).unwrap(), data);
    }

    #[test]
    fn extract_limit_exceeded_file_count() {
        let tmp = tempfile::TempDir::new().unwrap();
        let zip_path = tmp.path().join("many_files.zip");
        build_zip(
            &zip_path,
            &[
                ("1.txt", b"a".to_vec()),
                ("2.txt", b"b".to_vec()),
                ("3.txt", b"c".to_vec()),
            ],
        );

        let mut archive = open(&zip_path, None).unwrap();
        let dest = tmp.path().join("raw");
        fs::create_dir_all(&dest).unwrap();

        let limits = ExtractLimits {
            max_file_count: 2,
            ..Default::default()
        };
        let mut tracker = ExtractTracker::new(&limits);
        let res = extract(&mut archive, &dest, &limits, false, &mut tracker);
        assert!(matches!(res, Err(ConvertError::LimitExceeded(_))));
    }

    #[test]
    fn extract_limit_exceeded_bytes() {
        let data: Vec<u8> = vec![b'x'; 1024 * 1024]; // 1 MiB
        let tmp = tempfile::TempDir::new().unwrap();
        let zip_path = tmp.path().join("big_file.zip");
        build_zip(&zip_path, &[("big.bin", data)]);

        let mut archive = open(&zip_path, None).unwrap();
        let dest = tmp.path().join("raw");
        fs::create_dir_all(&dest).unwrap();

        let tracker = ExtractTracker {
            total_bytes: 0,
            file_count: 0,
            max_bytes: 500 * 1024, // 500 KiB limit
            max_files: 1000,
        };
        let mut t = tracker;
        let limits = ExtractLimits::default();
        let res = extract(&mut archive, &dest, &limits, false, &mut t);
        assert!(matches!(res, Err(ConvertError::LimitExceeded(_))));
    }
}
