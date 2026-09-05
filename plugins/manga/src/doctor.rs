use mop_plugin_sdk::{DoctorCheck, DoctorResult};
use std::process::Command;

use crate::config::MangaConfig;

pub fn doctor(cfg: &MangaConfig) -> DoctorResult {
    let mut checks = Vec::new();

    // 1. libarchive check
    checks.push(DoctorCheck {
        name: "libarchive".to_string(),
        status: "ok".to_string(),
        message: format!("version {}", libarchive2::version_details()),
    });

    // 2. libvips check
    match libvips::VipsApp::new("mop-plugin-manga", false) {
        Ok(app) => {
            let ver = app.version_string().unwrap_or("unknown");
            checks.push(DoctorCheck {
                name: "libvips".to_string(),
                status: "ok".to_string(),
                message: format!("version {ver}"),
            });
        }
        Err(e) => {
            checks.push(DoctorCheck {
                name: "libvips".to_string(),
                status: "error".to_string(),
                message: format!("failed to initialize libvips: {e}"),
            });
        }
    }

    // 3. ffmpeg check (optional in M5)
    match Command::new("ffmpeg").arg("-version").output() {
        Ok(out) if out.status.success() => {
            let ver_line = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("ffmpeg")
                .to_string();
            checks.push(DoctorCheck {
                name: "ffmpeg".to_string(),
                status: "ok".to_string(),
                message: ver_line,
            });
        }
        _ => {
            checks.push(DoctorCheck {
                name: "ffmpeg".to_string(),
                status: "warn".to_string(),
                message:
                    "ffmpeg not found (optional for mop.manga; video conversion handled by mop.video)"
                        .to_string(),
            });
        }
    }

    // 4. layout check
    match cfg.validate_layout() {
        Ok(()) => {
            checks.push(DoctorCheck {
                name: "layout".to_string(),
                status: "ok".to_string(),
                message: "watch and output directories are valid and disjoint".to_string(),
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
