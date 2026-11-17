use std::fmt;

pub type RenderResult<T> = Result<T, RenderError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidArgument,
    ImageDecode,
    ImageTooLarge,
    FontParse,
    PngEncode,
    JpegEncode,
    Internal,
}

impl ErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "INVALID_ARGUMENT",
            Self::ImageDecode => "IMAGE_DECODE",
            Self::ImageTooLarge => "IMAGE_TOO_LARGE",
            Self::FontParse => "FONT_PARSE",
            Self::PngEncode => "PNG_ENCODE",
            Self::JpegEncode => "JPEG_ENCODE",
            Self::Internal => "INTERNAL",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderError {
    code: ErrorCode,
    message: String,
}

impl RenderError {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }

    #[must_use]
    pub fn image(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ImageDecode, message)
    }

    #[must_use]
    pub fn image_too_large(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ImageTooLarge, message)
    }

    #[must_use]
    pub fn font(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::FontParse, message)
    }

    #[must_use]
    pub fn encode(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::PngEncode, message)
    }

    #[must_use]
    pub fn jpeg(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::JpegEncode, message)
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    #[must_use]
    pub fn into_napi(self) -> napi::Error {
        let status = match self.code {
            ErrorCode::InvalidArgument | ErrorCode::ImageTooLarge => napi::Status::InvalidArg,
            ErrorCode::ImageDecode
            | ErrorCode::FontParse
            | ErrorCode::PngEncode
            | ErrorCode::JpegEncode
            | ErrorCode::Internal => napi::Status::GenericFailure,
        };
        napi::Error::new(status, self.to_string())
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for RenderError {}

impl From<RenderError> for napi::Error {
    fn from(value: RenderError) -> Self {
        value.into_napi()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_stable_error_codes() {
        assert_eq!(ErrorCode::InvalidArgument.as_str(), "INVALID_ARGUMENT");
        assert_eq!(ErrorCode::ImageTooLarge.as_str(), "IMAGE_TOO_LARGE");
        assert_eq!(ErrorCode::JpegEncode.as_str(), "JPEG_ENCODE");
        assert_eq!(RenderError::jpeg("test").to_string(), "[JPEG_ENCODE] test");
    }
}
