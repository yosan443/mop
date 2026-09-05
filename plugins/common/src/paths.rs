use std::path::{Component, Path, PathBuf};

use crate::error::ConvertError;

const ARCHIVE_EXTS: &[&str] = &[
    "zip", "cbz", "rar", "cbr", "7z", "cb7", "gz", "bz2", "xz", "zst", "zstd", "lzh", "lha", "cab",
    "iso", "tar", "tgz", "tbz2", "txz", "lz", "lzma", "lz4", "cpio", "ar", "xar", "warc",
];

fn ascii_lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

pub fn output_base(input: &Path) -> String {
    let file_name = input
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.to_string_lossy().into_owned());

    let lower = ascii_lower(&file_name);

    for ext in [
        "tar.gz", "tar.xz", "tar.zst", "tar.zstd", "tar.bz2", "tar.lzma", "tar.lz",
    ] {
        if let Some(rest) = lower.strip_suffix(ext) {
            let trimmed = rest.trim_end_matches('.');
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    if let Some(dot) = lower.rfind('.') {
        let ext = &lower[dot + 1..];
        if ARCHIVE_EXTS.contains(&ext) {
            let stem = &file_name[..dot];
            if !stem.is_empty() {
                return stem.to_string();
            }
        }
    }

    file_name
}

pub fn output_stem(input: &Path) -> String {
    format!("{}.cbz", output_base(input))
}

pub fn has_archive_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| ARCHIVE_EXTS.contains(&ascii_lower(e).as_str()))
        .unwrap_or(false)
}

pub fn sanitize_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let trimmed = out.trim_end_matches(['.', ' ']);
    trimmed.to_string()
}

pub fn lexiclean(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

pub fn normalize(p: &Path) -> PathBuf {
    if p.exists() {
        p.canonicalize().unwrap_or_else(|_| lexiclean(p))
    } else {
        lexiclean(p)
    }
}

pub fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, ConvertError> {
    if rel.starts_with('/') {
        return Err(ConvertError::UnsafePath(format!(
            "absolute path in archive: {rel:?}"
        )));
    }
    let rel_path = Path::new(rel);
    if rel_path
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(ConvertError::UnsafePath(format!(
            "parent traversal in archive: {rel:?}"
        )));
    }
    let joined = base.join(rel_path);
    let escaped = {
        let base_norm = lexiclean(base);
        let joined_norm = lexiclean(&joined);
        !joined_norm.starts_with(&base_norm)
    };
    if escaped {
        return Err(ConvertError::UnsafePath(format!(
            "path escapes temp dir: {rel:?}"
        )));
    }
    Ok(joined)
}

pub fn move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::copy(src, dst)?;
            std::fs::remove_file(src)?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub fn unique_dest(dest: &Path) -> PathBuf {
    if !dest.exists() {
        return dest.to_path_buf();
    }
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let name = dest
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| dest.to_string_lossy().into_owned());
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    for i in 1.. {
        let cand = parent.join(format!("{stem} ({i}){ext}"));
        if !cand.exists() {
            return cand;
        }
    }
    unreachable!("suffix loop cannot be exhausted")
}

pub fn suffix_path(p: &Path, n: usize) -> PathBuf {
    let parent = p.parent().unwrap_or_else(|| Path::new("."));
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name.rfind('.') {
        Some(i) if i > 0 => parent.join(format!("{} ({n}){}", &name[..i], &name[i..])),
        _ => parent.join(format!("{name} ({n})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_stem_single_ext() {
        assert_eq!(
            output_stem(Path::new("Example Volume 01.cbr")),
            "Example Volume 01.cbz"
        );
        assert_eq!(output_stem(Path::new("foo.zip")), "foo.cbz");
        assert_eq!(output_stem(Path::new("bar.CBZ")), "bar.cbz");
    }

    #[test]
    fn output_stem_multi_ext() {
        assert_eq!(output_stem(Path::new("foo.tar.gz")), "foo.cbz");
        assert_eq!(output_stem(Path::new("foo.tar.xz")), "foo.cbz");
    }

    #[test]
    fn output_stem_no_ext() {
        assert_eq!(output_stem(Path::new("README")), "README.cbz");
    }

    #[test]
    fn output_base_strips_archive_exts() {
        assert_eq!(output_base(Path::new("foo.tar.gz")), "foo");
        assert_eq!(
            output_base(Path::new("Example Volume 01.cbr")),
            "Example Volume 01"
        );
        assert_eq!(output_base(Path::new("README")), "README");
        assert_eq!(output_base(Path::new("ep01.mkv")), "ep01.mkv");
    }

    #[test]
    fn safe_join_ok() {
        let base = Path::new("/tmp/x");
        assert_eq!(
            safe_join(base, "a/b/c.png").unwrap(),
            base.join("a/b/c.png")
        );
    }

    #[test]
    fn safe_join_rejects_absolute() {
        assert!(safe_join(Path::new("/tmp/x"), "/etc/passwd").is_err());
    }

    #[test]
    fn safe_join_rejects_parent() {
        assert!(safe_join(Path::new("/tmp/x"), "../evil").is_err());
        assert!(safe_join(Path::new("/tmp/x"), "a/../../evil").is_err());
    }

    #[test]
    fn suffix_path_appends_counter() {
        assert_eq!(
            suffix_path(Path::new("/v/a.mp4"), 1),
            PathBuf::from("/v/a (1).mp4")
        );
        assert_eq!(
            suffix_path(Path::new("/v/notes"), 2),
            PathBuf::from("/v/notes (2)")
        );
    }
}
