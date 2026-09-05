//! Deterministic title/volume parsing. No AI, no external data: naming and
//! splitting use fixed rules and filesystem structure only.

use crate::paths;

/// Parts of a multi-part volume (上 / 中 / 下).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    First,
    Second,
    Third,
}

impl Part {
    pub fn number(self) -> u32 {
        match self {
            Part::First => 1,
            Part::Second => 2,
            Part::Third => 3,
        }
    }
}

/// Identity of a volume for grouping/dedup purposes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VolumeKey {
    Number(u32),
    Part(Part),
    Extra { label: String },
    Unknown,
}

/// Confidence of a volume classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Low,
}

/// Keywords that mark a volume as non-mainline (never merged with the main
/// series numbers).
const EXTRA_KEYWORDS: &[&str] = &["外伝", "特装", "特典", "画集", "おまけ", "番外"];

/// Suffixes that indicate the first/second/third part of a volume.
const PART_SUFFIXES: &[(&str, Part)] = &[
    ("上巻", Part::First),
    ("中巻", Part::Second),
    ("下巻", Part::Third),
    ("上", Part::First),
    ("中", Part::Second),
    ("下", Part::Third),
];

/// Anything at or above this number at the tail of a name is treated as a
/// year/episode/page count, not a volume number.
const MAX_VOLUME_NUMBER: u32 = 500;

/// Normalize full-width digits (and 〇) to ASCII digits.
pub fn normalize_digits(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '０'..='９' => out.push((c as u32 - '０' as u32 + b'0' as u32) as u8 as char),
            '〇' => out.push('0'),
            _ => out.push(c),
        }
    }
    out
}

/// Parse a volume key from a folder or archive name. Fixed priority:
/// 第n巻/第n話 > vol(n)/v(n) > 上/中/下 > trailing number < 500 > extra keyword > Unknown.
pub fn parse_volume(name: &str) -> VolumeKey {
    let normalized = normalize_digits(name);

    // 第\d+巻 / 第\d+話
    if let Some(rest) = normalized.strip_prefix('第') {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let after = &rest[digits.len()..];
            if after.starts_with('巻') || after.starts_with('話') {
                if let Ok(n) = digits.parse() {
                    return VolumeKey::Number(n);
                }
            }
        }
    }

    // vol / volume / v followed by digits.
    if let Some(n) = find_vol_number(&normalized) {
        return VolumeKey::Number(n);
    }

    // 上 / 中 / 下 (末尾、直前が空白・開き括弧・文字列先頭).
    if let Some(part) = find_part(&normalized) {
        return VolumeKey::Part(part);
    }

    // Trailing number inside parentheses or after whitespace (< 500).
    if let Some(n) = trailing_number(&normalized) {
        return VolumeKey::Number(n);
    }

    // Extra keywords: gaiden, special edition, artbook, bonus, spin-off.
    for kw in EXTRA_KEYWORDS {
        if normalized.contains(kw) {
            return VolumeKey::Extra {
                label: kw.to_string(),
            };
        }
    }

    VolumeKey::Unknown
}

/// Find `vol`/`volume`/`v` + number anywhere in the name (word-boundary aware
/// for the bare `v` form).
fn find_vol_number(normalized: &str) -> Option<u32> {
    let lower = normalized.to_lowercase();

    // "volume" / "vol" forms: `vol`, `vol.`, `volume` followed by optional
    // spaces and digits.
    for pat in ["volume", "vol"] {
        let mut idx = 0;
        while let Some(pos) = lower[idx..].find(pat) {
            let pos = idx + pos;
            let after = lower[pos + pat.len()..].trim_start();
            let after = after.strip_prefix('.').unwrap_or(after).trim_start();
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                if let Ok(n) = digits.parse() {
                    return Some(n);
                }
            }
            idx = pos + pat.len();
        }
    }

    // Bare `v` form: `v01`, `v1`, `V2`. The `v` must not be preceded by an
    // alphanumeric char (avoids "novel", "cv01").
    let bytes = lower.as_bytes();
    for (i, c) in lower.char_indices() {
        if c != 'v' {
            continue;
        }
        let prev_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if !prev_ok {
            continue;
        }
        let after = &lower[i + 1..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.len() >= 2 || (digits.len() == 1 && lower.len() <= i + 2) {
            if let Ok(n) = digits.parse() {
                return Some(n);
            }
        }
    }
    None
}

/// 上 / 中 / 下 at the very end (optionally inside parentheses), preceded by
/// whitespace, an opening bracket, `巻`, or the start of the string.
fn find_part(normalized: &str) -> Option<Part> {
    let trimmed = normalized.trim_end();
    let inner = trimmed
        .strip_suffix(')')
        .or_else(|| trimmed.strip_suffix('）'))
        .unwrap_or(trimmed);
    for (suffix, part) in PART_SUFFIXES {
        let Some(rest) = inner.strip_suffix(suffix) else {
            continue;
        };
        if rest.is_empty() {
            return Some(*part);
        }
        match rest.chars().next_back() {
            Some(c) if c.is_whitespace() || c == '(' || c == '（' || c == '巻' => {
                return Some(*part)
            }
            _ => {}
        }
    }
    None
}

/// Trailing integer after whitespace or inside parentheses, below
/// `MAX_VOLUME_NUMBER`.
fn trailing_number(normalized: &str) -> Option<u32> {
    let trimmed = normalized.trim_end();
    let mut inner = trimmed;
    if let Some(rest) = inner.strip_suffix(')').or_else(|| inner.strip_suffix('）')) {
        inner = rest.trim_end();
    }
    let after = inner
        .rsplit_once(' ')
        .map(|(_, n)| n)
        .or_else(|| inner.rsplit_once('　').map(|(_, n)| n))
        .unwrap_or(inner);
    let after = after
        .strip_prefix('(')
        .or_else(|| after.strip_prefix('（'))
        .unwrap_or(after);
    if after.is_empty() || !after.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    match after.parse::<u32>() {
        Ok(n) if n < MAX_VOLUME_NUMBER => Some(n),
        _ => None,
    }
}

/// Strip a trailing volume token from a series-ish name.
///
/// Examples: `アオのハコ 7` -> `アオのハコ`, `foo v01` -> `foo`,
/// `foo 第1巻` -> `foo`, `foo 外伝` -> `foo`.
pub fn strip_volume_token(name: &str) -> String {
    let normalized = normalize_digits(name);
    let trimmed = normalized.trim_end();

    // 第\d+巻 / 第\d+話 at the end.
    if let Some(pos) = trimmed.rfind('第') {
        let rest = &trimmed[pos + '第'.len_utf8()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let after = &rest[digits.len()..];
            if after.starts_with('巻') || after.starts_with('話') {
                return trimmed[..pos].trim_end().to_string();
            }
        }
    }

    // `vol`/`v` + number at the end.
    let lower = normalized.to_lowercase();
    for pat in ["volume", "vol"] {
        if let Some(pos) = lower.rfind(pat) {
            let after = lower[pos + pat.len()..].trim_start();
            let after = after.strip_prefix('.').unwrap_or(after).trim_start();
            let digits: usize = after.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && after.chars().nth(digits).is_none() {
                return trimmed[..pos].trim_end().to_string();
            }
        }
    }
    if let Some(pos) = lower.rfind('v') {
        let after = &lower[pos + 1..];
        let digits: usize = after.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && after.chars().nth(digits).is_none() {
            return trimmed[..pos].trim_end().to_string();
        }
    }

    // Trailing number < 500.
    if trailing_number(trimmed).is_some() {
        let after = trimmed
            .rsplit_once(' ')
            .map(|(_, t)| t)
            .or_else(|| trimmed.rsplit_once('　').map(|(_, t)| t))
            .unwrap_or(trimmed);
        let end = trimmed.len() - after.len();
        return trimmed[..end].trim_end().to_string();
    }

    // 上 / 中 / 下 at the end.
    if find_part(trimmed).is_some() {
        for (suffix, _) in PART_SUFFIXES {
            if let Some(rest) = trimmed.strip_suffix(suffix) {
                return rest.trim_end().to_string();
            }
        }
    }

    // Extra keyword at the end.
    for kw in EXTRA_KEYWORDS {
        if let Some(pos) = trimmed.rfind(kw) {
            if trimmed[pos + kw.len()..].trim().is_empty() {
                return trimmed[..pos].trim_end().to_string();
            }
        }
    }

    trimmed.to_string()
}

/// Longest common prefix of an iterator of strings.
pub fn longest_common_prefix<'a>(mut it: impl Iterator<Item = &'a str>) -> String {
    let first = match it.next() {
        Some(s) => s,
        None => return String::new(),
    };
    let mut prefix = first.to_string();
    for s in it {
        while !s.starts_with(&prefix) {
            prefix.pop();
            if prefix.is_empty() {
                return prefix;
            }
        }
    }
    prefix
}

/// Output filename for a volume: `{series} v{nn}.cbz`, `{series} extra-{label}.cbz`,
/// or the sanitized raw name for unknown volumes.
pub fn volume_file_name(series: &str, key: &VolumeKey, raw_name: &str) -> String {
    let series = paths::sanitize_filename(series);
    match key {
        VolumeKey::Number(n) => format!("{series} v{n:02}.cbz"),
        VolumeKey::Part(p) => format!("{series} v{:02}.cbz", p.number()),
        VolumeKey::Extra { label } => {
            format!("{series} extra-{}.cbz", paths::sanitize_filename(label))
        }
        VolumeKey::Unknown => format!("{}.cbz", paths::sanitize_filename(raw_name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullwidth_digits_normalized() {
        assert_eq!(normalize_digits("第１巻"), "第1巻");
        assert_eq!(normalize_digits("１２３"), "123");
        assert_eq!(normalize_digits("〇"), "0");
    }

    #[test]
    fn parse_number_forms() {
        assert_eq!(parse_volume("第01巻"), VolumeKey::Number(1));
        assert_eq!(parse_volume("第1巻"), VolumeKey::Number(1));
        assert_eq!(parse_volume("第12話"), VolumeKey::Number(12));
        assert_eq!(parse_volume("v01"), VolumeKey::Number(1));
        assert_eq!(parse_volume("Vol. 1"), VolumeKey::Number(1));
        assert_eq!(parse_volume("Volume 3"), VolumeKey::Number(3));
        assert_eq!(parse_volume("アオのハコ 7"), VolumeKey::Number(7));
        assert_eq!(parse_volume("アオのハコ (7)"), VolumeKey::Number(7));
        assert_eq!(parse_volume("vol2"), VolumeKey::Number(2));
    }

    #[test]
    fn parse_part_forms() {
        assert_eq!(parse_volume("上"), VolumeKey::Part(Part::First));
        assert_eq!(parse_volume("中"), VolumeKey::Part(Part::Second));
        assert_eq!(parse_volume("下"), VolumeKey::Part(Part::Third));
        assert_eq!(parse_volume("アオのハコ 上"), VolumeKey::Part(Part::First));
        assert_eq!(parse_volume("アオのハコ(下)"), VolumeKey::Part(Part::Third));
        assert_eq!(parse_volume("下巻"), VolumeKey::Part(Part::Third));
    }

    #[test]
    fn trailing_year_or_large_number_is_not_volume() {
        assert_eq!(parse_volume("Chapter 2024"), VolumeKey::Unknown);
        assert_eq!(parse_volume("アオのハコ 500"), VolumeKey::Unknown);
        assert_eq!(parse_volume("1000"), VolumeKey::Unknown);
    }

    #[test]
    fn extra_keywords() {
        assert_eq!(
            parse_volume("アオのハコ 外伝"),
            VolumeKey::Extra {
                label: "外伝".into()
            }
        );
        assert_eq!(
            parse_volume("画集"),
            VolumeKey::Extra {
                label: "画集".into()
            }
        );
        assert_eq!(
            parse_volume("アオのハコ おまけ"),
            VolumeKey::Extra {
                label: "おまけ".into()
            }
        );
    }

    #[test]
    fn unknown_falls_through() {
        assert_eq!(parse_volume("readme"), VolumeKey::Unknown);
        assert_eq!(parse_volume("スキャン差分"), VolumeKey::Unknown);
        assert_eq!(parse_volume(""), VolumeKey::Unknown);
    }

    #[test]
    fn series_from_name() {
        assert_eq!(strip_volume_token("アオのハコ 7"), "アオのハコ");
        assert_eq!(strip_volume_token("アオのハコ 第1巻"), "アオのハコ");
        assert_eq!(strip_volume_token("foo v01"), "foo");
        assert_eq!(strip_volume_token("foo Vol. 2"), "foo");
        assert_eq!(strip_volume_token("Aoi no Hako"), "Aoi no Hako");
        assert_eq!(strip_volume_token("アオのハコ 上"), "アオのハコ");
        assert_eq!(strip_volume_token("アオのハコ 外伝"), "アオのハコ");
    }

    #[test]
    fn longest_common_prefix_works() {
        assert_eq!(
            longest_common_prefix(["Vol 01", "Vol 02", "Vol 03"].iter().copied()),
            "Vol 0"
        );
        assert_eq!(longest_common_prefix(["a", "b"].iter().copied()), "");
        assert_eq!(longest_common_prefix(["same"].iter().copied()), "same");
    }

    #[test]
    fn volume_file_names() {
        let series = "アオのハコ";
        assert_eq!(
            volume_file_name(series, &VolumeKey::Number(1), "x"),
            "アオのハコ v01.cbz"
        );
        assert_eq!(
            volume_file_name(series, &VolumeKey::Number(100), "x"),
            "アオのハコ v100.cbz"
        );
        assert_eq!(
            volume_file_name(series, &VolumeKey::Part(Part::Second), "x"),
            "アオのハコ v02.cbz"
        );
        assert_eq!(
            volume_file_name(
                series,
                &VolumeKey::Extra {
                    label: "外伝".into()
                },
                "x"
            ),
            "アオのハコ extra-外伝.cbz"
        );
        assert_eq!(
            volume_file_name(series, &VolumeKey::Unknown, "スキャン 差分"),
            "スキャン 差分.cbz"
        );
        assert_eq!(
            volume_file_name("a/b", &VolumeKey::Number(1), "x"),
            "a b v01.cbz"
        );
    }
}
