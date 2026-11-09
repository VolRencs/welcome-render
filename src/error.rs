use std::fmt;

pub type RenderResult<T> = Result<T, RenderError>;

#[derive(Debug, Clone)]
pub struct RenderError {
    code: &'static str,
    message: String,
}

impl RenderError {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID_ARGUMENT", message)
    }

    #[must_use]
    pub fn image(message: impl Into<String>) -> Self {
        Self::new("IMAGE_DECODE", message)
    }

    #[must_use]
    pub fn image_too_large(message: impl Into<String>) -> Self {
        Self::new("IMAGE_TOO_LARGE", message)
    }

    #[must_use]
    pub fn font(message: impl Into<String>) -> Self {
        Self::new("FONT_PARSE", message)
    }

    #[must_use]
    pub fn encode(message: impl Into<String>) -> Self {
        Self::new("PNG_ENCODE", message)
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("INTERNAL", message)
    }

    #[must_use]
    pub fn into_napi(self) -> napi::Error {
        let status = match self.code {
            "INVALID_ARGUMENT" | "IMAGE_TOO_LARGE" => napi::Status::InvalidArg,
            _ => napi::Status::GenericFailure,
        };
        napi::Error::new(status, self.to_string())
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for RenderError {}

impl From<RenderError> for napi::Error {
    fn from(value: RenderError) -> Self {
        value.into_napi()
    }
}
