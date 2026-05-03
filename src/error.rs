use thiserror::Error;

#[allow(dead_code)]
#[derive(Debug, Error)]
#[must_use = "errors must be handled or explicitly ignored with `let _ =`"]
pub enum RespError {
    #[error("incomplete frame")]
    Incomplete,

    #[error("unknown type byte: 0x{byte:02x}")]
    InvalidTypeByte { byte: u8 },

    #[error("invalid integer")]
    InvalidInteger,

    #[error("invalid double")]
    InvalidDouble,

    #[error("invalid length")]
    InvalidLength,

    #[error("missing CRLF terminator")]
    MissingCrlf,

    #[error("verbatim string encoding separator missing or too short")]
    InvalidVerbatim,

    #[error("invalid big number")]
    InvalidBigNumber,

    #[error("nesting depth limit exceeded")]
    DepthLimitExceeded,

    #[error("frame exceeds size limit of {limit} bytes")]
    FrameTooLarge { limit: usize },

    #[error("I/O error")]
    Io {
        #[source]
        source: std::io::Error,
    },
}

impl RespError {
    pub(crate) fn invalid_type(byte: u8) -> Self {
        Self::InvalidTypeByte { byte }
    }

    pub(crate) fn too_large(limit: usize) -> Self {
        Self::FrameTooLarge { limit }
    }
}

impl From<std::io::Error> for RespError {
    fn from(source: std::io::Error) -> Self {
        Self::Io { source }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn io_error_converts_to_resp_error() {
        let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed");
        let resp_err: RespError = io_err.into();
        assert!(matches!(resp_err, RespError::Io { .. }));
        assert!(resp_err.to_string().contains("I/O error"));
    }
}
