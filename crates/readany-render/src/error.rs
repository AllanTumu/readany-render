use crate::model::Unrendered;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderErrorCode {
    UnknownFormat,
    MalformedDocument,
    InvalidEncoding,
    NoFonts,
    LimitExceeded,
    Unsupported,
    StrictIncomplete,
    InvalidOptions,
    Rasterisation,
}

impl RenderErrorCode {
    pub const fn stable(self) -> &'static str {
        match self {
            Self::UnknownFormat => "RR-0101",
            Self::MalformedDocument => "RR-0102",
            Self::InvalidEncoding => "RR-0103",
            Self::NoFonts => "RR-0104",
            Self::LimitExceeded => "RR-0201",
            Self::Unsupported => "RR-0301",
            Self::StrictIncomplete => "RR-0302",
            Self::InvalidOptions => "RR-0401",
            Self::Rasterisation => "RR-0402",
        }
    }
}

#[derive(Debug, Error)]
#[error("{code}: {message}", code = .code.stable())]
pub struct RenderError {
    pub code: RenderErrorCode,
    pub message: String,
    pub unrendered: Vec<Unrendered>,
}

impl RenderError {
    pub(crate) fn new(code: RenderErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            unrendered: Vec::new(),
        }
    }

    pub(crate) fn malformed(message: impl Into<String>) -> Self {
        Self::new(RenderErrorCode::MalformedDocument, message)
    }

    pub(crate) fn limit(name: &'static str, actual: u64) -> Self {
        Self::new(
            RenderErrorCode::LimitExceeded,
            format!("{name} limit exceeded by input value {actual}; open a smaller document"),
        )
    }

    pub(crate) fn invalid_options(message: impl Into<String>) -> Self {
        Self::new(RenderErrorCode::InvalidOptions, message)
    }

    pub(crate) fn strict(unrendered: Vec<Unrendered>) -> Self {
        Self {
            code: RenderErrorCode::StrictIncomplete,
            message: "the document contains content this build cannot draw; disable strict mode to inspect the partial result".into(),
            unrendered,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_error_codes_are_never_renumbered() {
        let actual = [
            RenderErrorCode::UnknownFormat,
            RenderErrorCode::MalformedDocument,
            RenderErrorCode::InvalidEncoding,
            RenderErrorCode::NoFonts,
            RenderErrorCode::LimitExceeded,
            RenderErrorCode::Unsupported,
            RenderErrorCode::StrictIncomplete,
            RenderErrorCode::InvalidOptions,
            RenderErrorCode::Rasterisation,
        ]
        .map(RenderErrorCode::stable);
        assert_eq!(
            actual,
            [
                "RR-0101", "RR-0102", "RR-0103", "RR-0104", "RR-0201", "RR-0301", "RR-0302",
                "RR-0401", "RR-0402"
            ]
        );
    }
}
