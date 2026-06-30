#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum FingerprintError {
    #[error("decode error: {message}")]
    DecodeError { message: String },

    #[error("unsupported format: {message}")]
    UnsupportedFormat { message: String },

    #[error("invalid input: {message}")]
    InvalidInput { message: String },

    #[error("io error: {message}")]
    IoError { message: String },
}

impl FingerprintError {
    pub fn decode(message: impl Into<String>) -> Self {
        Self::DecodeError {
            message: message.into(),
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::UnsupportedFormat {
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
        }
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::IoError {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::DecodeError { message }
            | Self::UnsupportedFormat { message }
            | Self::InvalidInput { message }
            | Self::IoError { message } => message,
        }
    }
}
