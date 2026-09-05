use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use mop_plugin_common::archive;
use mop_plugin_common::classify::is_video_file;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct VideoInspectResult {
    pub path: String,
    pub is_archive: bool,
    pub format: String,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub resolution: Option<String>,
    pub size_bytes: u64,
    pub details: Vec<String>,
}

pub fn inspect(path: &Path, password: Option<&str>) -> Result<VideoInspectResult> {
    let meta = fs::metadata(path).context("read input metadata")?;
    let size_bytes = meta.len();

    if is_video_file(path) {
        // Probe with ffprobe / ffmpeg
        let mut res = VideoInspectResult {
            path: path.display().to_string(),
            is_archive: false,
            format: path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("video")
                .to_string(),
            video_codec: None,
            audio_codec: None,
            resolution: None,
            size_bytes,
            details: Vec::new(),
        };

        // Try ffprobe first
        if let Ok(out) = Command::new("ffprobe")
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("stream=codec_type,codec_name,width,height")
            .arg("-of")
            .arg("json")
            .arg(path)
            .output()
        {
            if out.status.success() {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                    if let Some(streams) = json.get("streams").and_then(|s| s.as_array()) {
                        for s in streams {
                            let c_type = s.get("codec_type").and_then(|t| t.as_str());
                            let c_name = s.get("codec_name").and_then(|n| n.as_str());
                            if c_type == Some("video") && res.video_codec.is_none() {
                                res.video_codec = c_name.map(|s| s.to_string());
                                let w = s.get("width").and_then(|v| v.as_u64());
                                let h = s.get("height").and_then(|v| v.as_u64());
                                if let (Some(width), Some(height)) = (w, h) {
                                    res.resolution = Some(format!("{width}x{height}"));
                                }
                            } else if c_type == Some("audio") && res.audio_codec.is_none() {
                                res.audio_codec = c_name.map(|s| s.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Fallback or supplementary: ffmpeg -i
        if res.video_codec.is_none() {
            if let Ok(out) = Command::new("ffmpeg").arg("-i").arg(path).output() {
                let err_str = String::from_utf8_lossy(&out.stderr);
                for line in err_str.lines() {
                    if (line.contains("Video:") && res.video_codec.is_none())
                        || (line.contains("Audio:") && res.audio_codec.is_none())
                    {
                        res.details.push(line.trim().to_string());
                    }
                }
            }
        }

        return Ok(res);
    }

    // Archive inspection
    let mut ar = archive::open(path, password)
        .with_context(|| format!("cannot open {} as an archive", path.display()))?;

    let mut details = Vec::new();
    let mut count = 0;
    while let Ok(Some(entry)) = ar.next_entry() {
        let name = entry.pathname().unwrap_or_default();
        if is_video_file(Path::new(&name)) {
            details.push(format!("video: {name} ({} bytes)", entry.size()));
        }
        count += 1;
    }

    Ok(VideoInspectResult {
        path: path.display().to_string(),
        is_archive: true,
        format: "archive".to_string(),
        video_codec: None,
        audio_codec: None,
        resolution: None,
        size_bytes,
        details: vec![format!(
            "Total entries: {count}, video entries: {}",
            details.len()
        )],
    })
}
