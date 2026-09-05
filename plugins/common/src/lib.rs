pub mod archive;
pub mod classify;
pub mod error;
pub mod paths;

pub use archive::{extract, open, ExtractLimits, ExtractResult, ExtractTracker};
pub use classify::{classify, decide, is_image_ext, is_video_ext, is_video_file, Kind};
pub use error::{ConvertError, Reason};
pub use paths::{
    has_archive_extension, lexiclean, move_file, normalize, output_base, output_stem, safe_join,
    sanitize_filename, suffix_path, unique_dest,
};
