use std::path::Path;

use anyhow::{Context, Result};
use libarchive2::FileType;
use serde::Serialize;

use crate::archive;

#[derive(Debug, Clone, Serialize)]
pub struct InspectResult {
    pub path: String,
    pub total_entries: usize,
    pub encrypted_entries: usize,
    pub any_encrypted: bool,
    pub entries: Vec<InspectEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectEntry {
    pub name: String,
    pub entry_type: String,
    pub size: i64,
    pub encrypted: bool,
}

pub fn inspect(path: &Path, password: Option<&str>) -> Result<InspectResult> {
    let mut ar = archive::open(path, password)
        .with_context(|| format!("cannot open {} as an archive", path.display()))?;

    let mut total_entries = 0usize;
    let mut encrypted_entries = 0usize;
    let mut any_encrypted = false;
    let mut entries = Vec::new();

    loop {
        let entry = match ar.next_entry() {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(e) => {
                tracing::warn!("error reading entries: {e}");
                break;
            }
        };
        let name = entry.pathname().unwrap_or_default();
        let ft = entry.file_type();
        let size = entry.size();
        let enc = entry.is_encrypted();
        if enc {
            any_encrypted = true;
            encrypted_entries += 1;
        }
        entries.push(InspectEntry {
            name: if name.is_empty() {
                "<unnamed>".to_string()
            } else {
                name
            },
            entry_type: type_str(ft).to_string(),
            size,
            encrypted: enc,
        });
        total_entries += 1;
    }

    Ok(InspectResult {
        path: path.display().to_string(),
        total_entries,
        encrypted_entries,
        any_encrypted,
        entries,
    })
}

fn type_str(ft: FileType) -> &'static str {
    match ft {
        FileType::RegularFile => "file",
        FileType::Directory => "dir",
        FileType::SymbolicLink => "symlink",
        FileType::BlockDevice => "block",
        FileType::CharacterDevice => "char",
        FileType::Fifo => "fifo",
        FileType::Socket => "socket",
        FileType::Unknown => "unknown",
    }
}
