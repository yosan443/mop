//! Minimal ComicInfo.xml reading/writing. The XML here is a tiny fixed-shape
//! document (no attributes, no namespaces), so a small tag extractor is
//! sufficient; no XML crate needed.

use std::fs;
use std::path::Path;

use crate::error::ConvertError;

/// Values read from a ComicInfo.xml.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // fields are kept for future title resolution
pub struct ComicInfo {
    pub series: Option<String>,
    pub volume: Option<u32>,
    pub number: Option<u32>,
    pub title: Option<String>,
}

impl ComicInfo {
    /// The effective volume number: `<Volume>` first, then `<Number>`.
    pub fn volume_number(&self) -> Option<u32> {
        self.volume.or(self.number)
    }
}

/// Extract the text content of a simple `<tag>text</tag>` pair (case-insensitive).
fn tag_value(xml: &str, tag: &str) -> Option<String> {
    let lower = xml.to_lowercase();
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = lower.find(&open)?;
    let after = start + open.len();
    let end = lower[after..].find(&close)? + after;
    let value = xml[after..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Parse ComicInfo.xml text.
pub fn parse(xml: &str) -> ComicInfo {
    ComicInfo {
        series: tag_value(xml, "series"),
        volume: tag_value(xml, "volume").and_then(|v| v.parse().ok()),
        number: tag_value(xml, "number").and_then(|v| v.parse().ok()),
        title: tag_value(xml, "title"),
    }
}

/// Read ComicInfo.xml from a directory (any case), if present.
pub fn read_from_dir(dir: &Path) -> Option<ComicInfo> {
    let mut name = None;
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let n = entry.file_name().to_string_lossy().into_owned();
        if n.eq_ignore_ascii_case("comicinfo.xml") && entry.file_type().ok()?.is_file() {
            name = Some(entry.path());
            break;
        }
    }
    let path = name?;
    let xml = fs::read_to_string(path).ok()?;
    Some(parse(&xml))
}

/// Escape a string for inclusion in XML text content.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Ensure `staging/ComicInfo.xml` carries at least Series/Volume/Title.
///
/// - An existing ComicInfo.xml with non-empty Series and Volume is kept as-is.
/// - Missing/empty Series, Volume and Title are filled in from the arguments.
/// - When no ComicInfo.xml exists, one is generated.
pub fn ensure(
    staging: &Path,
    series: &str,
    volume: Option<u32>,
    title: &str,
) -> Result<(), ConvertError> {
    let path = find_comicinfo(staging);
    let mut xml = match &path {
        Some(p) => fs::read_to_string(p).unwrap_or_default(),
        None => String::new(),
    };

    if xml.trim().is_empty() {
        xml = format!(
            "<?xml version=\"1.0\"?>\n<ComicInfo>\n  <Series>{}</Series>\n",
            escape(series)
        );
        if let Some(v) = volume {
            xml.push_str(&format!("  <Volume>{v}</Volume>\n"));
        }
        xml.push_str(&format!("  <Title>{}</Title>\n", escape(title)));
        xml.push_str("</ComicInfo>\n");
    } else {
        xml = fill_tag(&xml, "Series", series);
        if let Some(v) = volume {
            xml = fill_tag(&xml, "Volume", &v.to_string());
        }
        xml = fill_tag(&xml, "Title", title);
    }

    let target = match path {
        Some(p) => p,
        None => staging.join("ComicInfo.xml"),
    };
    fs::write(&target, xml)?;
    Ok(())
}

/// Find `ComicInfo.xml` (any case) in `dir`.
fn find_comicinfo(dir: &Path) -> Option<std::path::PathBuf> {
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        if entry.file_type().ok()?.is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("comicinfo.xml")
        {
            return Some(entry.path());
        }
    }
    None
}

/// Copy ComicInfo.xml from `src_dir` into `staging` when the staging dir does
/// not already have one. Returns whether a file was copied.
pub fn copy_if_present(src_dir: &Path, staging: &Path) -> Result<bool, ConvertError> {
    if find_comicinfo(staging).is_some() {
        return Ok(false);
    }
    match find_comicinfo(src_dir) {
        Some(src) => {
            fs::copy(&src, staging.join("ComicInfo.xml"))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Replace or insert a single tag in the existing XML text.
fn fill_tag(xml: &str, tag: &str, value: &str) -> String {
    let lower = xml.to_lowercase();
    let tag_lower = tag.to_lowercase();
    let open = format!("<{tag_lower}>");
    let close = format!("</{tag_lower}>");

    if let Some(start) = lower.find(&open) {
        if let Some(end_rel) = lower[start + open.len()..].find(&close) {
            let end = start + open.len() + end_rel;
            let escaped = escape(value);
            if xml[start + open.len()..end].trim().is_empty() {
                return format!("{}{}{}", &xml[..start + open.len()], escaped, &xml[end..]);
            }
            return xml.to_string();
        }
    }

    // Insert before </ComicInfo> (case-insensitive).
    if let Some(close_pos) = lower.find("</comicinfo>") {
        return format!(
            "{}\n  <{}>{}</{}>{}",
            &xml[..close_pos],
            tag,
            escape(value),
            tag,
            &xml[close_pos..]
        );
    }
    xml.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_tags_case_insensitively() {
        let xml = r#"<?xml version="1.0"?>
<ComicInfo>
  <Series>アオのハコ</Series>
  <Volume>3</Volume>
  <Title>アオのハコ v03</Title>
</ComicInfo>"#;
        let ci = parse(xml);
        assert_eq!(ci.series.as_deref(), Some("アオのハコ"));
        assert_eq!(ci.volume, Some(3));
        assert_eq!(ci.volume_number(), Some(3));
        assert_eq!(ci.title.as_deref(), Some("アオのハコ v03"));
    }

    #[test]
    fn number_falls_back_to_volume() {
        let ci = parse("<ComicInfo><Number>7</Number></ComicInfo>");
        assert_eq!(ci.volume_number(), Some(7));
    }

    #[test]
    fn generates_missing_comicinfo() {
        let tmp = tempfile::TempDir::new().unwrap();
        ensure(tmp.path(), "アオのハコ", Some(1), "アオのハコ v01").unwrap();
        let xml = fs::read_to_string(tmp.path().join("ComicInfo.xml")).unwrap();
        assert!(xml.contains("<Series>アオのハコ</Series>"));
        assert!(xml.contains("<Volume>1</Volume>"));
        assert!(xml.contains("<Title>アオのハコ v01</Title>"));
    }

    #[test]
    fn fills_empty_series_and_keeps_existing() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(
            tmp.path().join("ComicInfo.xml"),
            "<ComicInfo>\n  <Series></Series>\n  <Volume>2</Volume>\n</ComicInfo>",
        )
        .unwrap();
        ensure(tmp.path(), "アオのハコ", None, "アオのハコ v02").unwrap();
        let xml = fs::read_to_string(tmp.path().join("ComicInfo.xml")).unwrap();
        assert!(xml.contains("<Series>アオのハコ</Series>"));
        assert!(xml.contains("<Volume>2</Volume>"));
        assert!(xml.contains("<Title>アオのハコ v02</Title>"));
    }

    #[test]
    fn keeps_complete_existing_comicinfo_unchanged() {
        let tmp = tempfile::TempDir::new().unwrap();
        let original = "<ComicInfo>\n  <Series>既存</Series>\n  <Volume>5</Volume>\n</ComicInfo>";
        fs::write(tmp.path().join("ComicInfo.xml"), original).unwrap();
        ensure(tmp.path(), "別の名前", Some(9), "title").unwrap();
        let xml = fs::read_to_string(tmp.path().join("ComicInfo.xml")).unwrap();
        // Existing Series/Volume are kept; the missing Title is filled in.
        assert!(xml.contains("<Series>既存</Series>"));
        assert!(xml.contains("<Volume>5</Volume>"));
        assert!(xml.contains("<Title>title</Title>"));
    }
}
