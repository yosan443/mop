use mop_plugin_sdk::{DoctorCheck, DoctorResult};
use std::process::Command;
use std::sync::OnceLock;

use crate::config::MangaConfig;

static VIPS_APP: OnceLock<Result<&'static libvips::VipsApp, String>> = OnceLock::new();

/// Set the globally held VipsApp instance created at startup.
pub fn set_vips_app(app_res: Result<&'static libvips::VipsApp, String>) {
    let _ = VIPS_APP.set(app_res);
}

/// Retrieve the globally held VipsApp instance or initialize it once if not yet set.
pub fn get_vips_app() -> Result<&'static libvips::VipsApp, &'static str> {
    let res = VIPS_APP.get_or_init(|| match libvips::VipsApp::new("mop-plugin-manga", false) {
        Ok(app) => {
            let leaked = Box::leak(Box::new(app));
            Ok(leaked)
        }
        Err(e) => Err(e.to_string()),
    });
    match res {
        Ok(app) => Ok(*app),
        Err(e) => Err(e.as_str()),
    }
}

pub fn doctor(cfg: &MangaConfig) -> DoctorResult {
    let mut checks = Vec::new();

    // 1. libarchive check
    checks.push(DoctorCheck {
        name: "libarchive".to_string(),
        status: "ok".to_string(),
        message: format!("version {}", libarchive2::version_details()),
    });

    // 2. libvips check
    match get_vips_app() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doctor_checks() {
        let cfg = MangaConfig::default();
        let res = doctor(&cfg);
        assert_eq!(res.checks.len(), 4);
        let libvips_check = res.checks.iter().find(|c| c.name == "libvips").unwrap();
        assert_eq!(libvips_check.status, "ok");
        assert!(libvips_check.message.contains("version"));

        // Calling doctor again must reuse the held instance without error or vips_shutdown
        let res2 = doctor(&cfg);
        let libvips_check2 = res2.checks.iter().find(|c| c.name == "libvips").unwrap();
        assert_eq!(libvips_check2.status, "ok");
    }
}
