use mop_plugin_sdk::{DoctorCheck, DoctorResult};
use std::process::Command;

use crate::config::VideoConfig;

pub fn check_ffmpeg() -> DoctorCheck {
    let out = match Command::new("ffmpeg").arg("-version").output() {
        Ok(o) if o.status.success() => o,
        _ => {
            return DoctorCheck {
                name: "ffmpeg".to_string(),
                status: "error".to_string(),
                message: "ffmpeg command not found in PATH".to_string(),
            };
        }
    };

    let ver_line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("ffmpeg")
        .to_string();

    let enc_out = match Command::new("ffmpeg").arg("-encoders").output() {
        Ok(o) if o.status.success() => o,
        _ => {
            return DoctorCheck {
                name: "ffmpeg".to_string(),
                status: "error".to_string(),
                message: "failed to query ffmpeg encoders".to_string(),
            };
        }
    };

    let enc_str = String::from_utf8_lossy(&enc_out.stdout);
    if enc_str.contains("libx265") {
        DoctorCheck {
            name: "ffmpeg".to_string(),
            status: "ok".to_string(),
            message: format!("{ver_line} (libx265 enabled)"),
        }
    } else {
        DoctorCheck {
            name: "ffmpeg".to_string(),
            status: "error".to_string(),
            message: format!("{ver_line} (libx265 encoder missing)"),
        }
    }
}

pub fn doctor(cfg: &VideoConfig) -> DoctorResult {
    let mut checks = Vec::new();

    // 1. ffmpeg + libx265 check (mandatory)
    checks.push(check_ffmpeg());

    // 2. layout check
    match cfg.validate_layout() {
        Ok(()) => {
            checks.push(DoctorCheck {
                name: "layout".to_string(),
                status: "ok".to_string(),
                message: "watch and video directories are valid and disjoint".to_string(),
            });
        }
        Err(e) => {
            checks.push(DoctorCheck {
                name: "layout".to_string(),
                status: "error".to_string(),
                message: e,
            });
        }
    }

    DoctorResult { checks }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffmpeg_and_libx265_detection() {
        let check = check_ffmpeg();
        assert_eq!(check.name, "ffmpeg");
        assert_eq!(
            check.status, "ok",
            "ffmpeg with libx265 must be available in the test environment: {}",
            check.message
        );
        assert!(check.message.contains("libx265"));
    }

    #[test]
    fn test_doctor_layout_validation() {
        let cfg = VideoConfig::default();
        let res = doctor(&cfg);
        assert_eq!(res.checks.len(), 2);
        let layout_check = res.checks.iter().find(|c| c.name == "layout").unwrap();
        assert_eq!(layout_check.status, "ok");
    }
}
