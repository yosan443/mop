use thiserror::Error;

/// Machine-greppable status `reason` codes emitted in status log lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    PasswordRequired,
    PasswordInvalid,
    OpenFailed,
    ConvertError,
    AnimatedNotSupported,
    UnsupportedFileType,
    UnsafePath,
    LimitExceeded,
    OutputExists,
}

impl Reason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Reason::PasswordRequired => "password_required",
            Reason::PasswordInvalid => "password_invalid",
            Reason::OpenFailed => "open_failed",
            Reason::ConvertError => "convert_error",
            Reason::AnimatedNotSupported => "animated_not_supported",
            Reason::UnsupportedFileType => "unsupported_file_type",
            Reason::UnsafePath => "unsafe_path",
            Reason::LimitExceeded => "limit_exceeded",
            Reason::OutputExists => "output_exists",
        }
    }
}

/// Errors raised while converting or inspecting an archive / media item.
#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("password required")]
    PasswordRequired,

    #[error("password invalid: {0}")]
    PasswordInvalid(String),

    #[error("could not open archive: {0}")]
    Open(String),

    #[error("unsupported entry type: {0}")]
    UnsupportedFileType(String),

    #[error("unsafe path in archive: {0}")]
    UnsafePath(String),

    #[error("archive limits exceeded: {0}")]
    LimitExceeded(String),

    #[error("animated image not supported: {0}")]
    AnimatedNotSupported(String),

    #[error("output already exists")]
    OutputExists,

    #[error("conversion failed: {0}")]
    Convert(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl ConvertError {
    pub fn reason(&self) -> Reason {
        match self {
            ConvertError::PasswordRequired => Reason::PasswordRequired,
            ConvertError::PasswordInvalid(_) => Reason::PasswordInvalid,
            ConvertError::Open(_) => Reason::OpenFailed,
            ConvertError::UnsupportedFileType(_) => Reason::UnsupportedFileType,
            ConvertError::UnsafePath(_) => Reason::UnsafePath,
            ConvertError::LimitExceeded(_) => Reason::LimitExceeded,
            ConvertError::AnimatedNotSupported(_) => Reason::AnimatedNotSupported,
            ConvertError::OutputExists => Reason::OutputExists,
            ConvertError::Convert(_) | ConvertError::Io(_) => Reason::ConvertError,
        }
    }
}
