use std::ffi::CStr;
use std::fs;
use std::path::Path;

use libvips::{ops, VipsImage};
use tracing::warn;

use crate::error::ConvertError;

/// WebP spec limit: 14 bits per dimension. libvips fails to save images
/// larger than this with a bare "WebpsaveError", so we downscale first.
const WEBP_MAX_DIMENSION: i32 = 16383;

/// Fetch the last libvips error from the thread-local error buffer.
/// Rationale for unsafe: Calling C FFI `vips_error_buffer()` from libvips C library to inspect error details.
fn vips_error_message() -> String {
    unsafe {
        let ptr = libvips::bindings::vips_error_buffer();
        if ptr.is_null() {
            "unknown libvips error".to_string()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

/// Transcode a single still image to WebP, applying EXIF auto-rotation.
pub fn transcode_to_webp(
    src: &Path,
    dst: &Path,
    quality: i32,
    lossless: bool,
) -> Result<u32, ConvertError> {
    let src_str = src.to_string_lossy();
    let image = VipsImage::new_from_file(&src_str)
        .map_err(|e| ConvertError::Convert(format!("decode {}: {e}", src.display())))?;

    let pages = image.get_n_pages();
    if pages > 1 {
        return Err(ConvertError::AnimatedNotSupported(format!(
            "{}: {} pages (animated)",
            src.display(),
            pages
        )));
    }

    let rotated = ops::autorot(&image).map_err(|e| {
        ConvertError::Convert(format!(
            "autorot {}: {e} ({})",
            src.display(),
            vips_error_message()
        ))
    })?;

    let (w, h) = (rotated.get_width(), rotated.get_height());
    let encoded = if w > WEBP_MAX_DIMENSION || h > WEBP_MAX_DIMENSION {
        let max_dim = w.max(h);
        let scale = WEBP_MAX_DIMENSION as f64 / max_dim as f64;
        warn!(
            "image {} exceeds WebP dimension limit ({}x{} > {}), downscaling to {}x{}",
            src.display(),
            w,
            h,
            WEBP_MAX_DIMENSION,
            (w as f64 * scale) as i32,
            (h as f64 * scale) as i32
        );
        ops::resize(&rotated, scale).map_err(|e| {
            ConvertError::Convert(format!(
                "resize {}: {e} ({})",
                src.display(),
                vips_error_message()
            ))
        })?
    } else {
        rotated
    };

    let save_opts = format!(".webp[Q={quality},lossless={lossless}]");
    let bytes = encoded.image_write_to_buffer(&save_opts).map_err(|e| {
        ConvertError::Convert(format!(
            "webpsave {}: {e} ({})",
            src.display(),
            vips_error_message()
        ))
    })?;
    fs::write(dst, &bytes)?;

    Ok(pages as u32)
}

/// Image extensions that are transcoded to WebP.
pub fn is_image_ext(ext: Option<&str>) -> bool {
    matches!(
        ext.map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "avif" | "heic" | "webp")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webp_max_dimension_is_16383() {
        assert_eq!(WEBP_MAX_DIMENSION, 16383);
    }
}
