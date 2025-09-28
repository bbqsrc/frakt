//! Windows-specific error handling and mapping

use crate::Error;
use windows::core;

/// Convert Windows HRESULT errors to library errors
pub fn map_windows_error(error: core::Error) -> Error {
    let hresult = error.code().0 as i64;

    match hresult {
        // Network-related errors
        0x80072EE7 => Error::Network {
            code: hresult,
            message: "The server name or address could not be resolved".to_string(),
        },
        0x80072EFD => Error::Network {
            code: hresult,
            message: "The connection with the server was terminated abnormally".to_string(),
        },
        0x80072EE2 => Error::Timeout,
        0x80072F05 => Error::Network {
            code: hresult,
            message: "The URL is invalid".to_string(),
        },
        0x80072F06 => Error::Network {
            code: hresult,
            message: "The URL scheme is invalid".to_string(),
        },
        0x80072F0C => Error::Network {
            code: hresult,
            message: "The server returned an invalid response".to_string(),
        },
        0x80072F0D => Error::Network {
            code: hresult,
            message: "The request has timed out".to_string(),
        },
        0x80072F17 => Error::Network {
            code: hresult,
            message: "The operation was cancelled".to_string(),
        },
        0x80072F19 => Error::Network {
            code: hresult,
            message: "The requested resource could not be found".to_string(),
        },
        0x80072F76 => Error::Network {
            code: hresult,
            message: "The HTTP redirect request failed".to_string(),
        },
        0x80072F7D => Error::Network {
            code: hresult,
            message: "The application is moving from a non-SSL to an SSL connection".to_string(),
        },
        0x80072F8F => Error::Network {
            code: hresult,
            message: "The certificate authority is invalid or incorrect".to_string(),
        },
        0x80072F90 => Error::Network {
            code: hresult,
            message: "The SSL certificate contains errors".to_string(),
        },

        // WebSocket-specific errors
        0x80072EF4 => Error::Network {
            code: hresult,
            message: "WebSocket protocol error".to_string(),
        },
        0x80072EF5 => Error::Network {
            code: hresult,
            message: "WebSocket data type error".to_string(),
        },

        // Generic errors
        0x80004005 => Error::Internal("Unspecified error".to_string()),
        0x80070005 => Error::Internal("Access denied".to_string()),
        0x8007000E => Error::Internal("Out of memory".to_string()),
        0x80070057 => Error::Internal("Invalid argument".to_string()),

        // Default case
        _ => Error::Internal(format!("Windows error: 0x{:08X} - {}", hresult, error.message())),
    }
}

/// Map Windows errors with additional context
pub fn map_windows_error_with_context(error: core::Error, context: &str) -> Error {
    let mut mapped_error = map_windows_error(error);

    // Add context to the error message
    if let Error::Internal(ref mut message) = mapped_error {
        *message = format!("{}: {}", context, message);
    } else if let Error::Network { ref mut message, .. } = mapped_error {
        *message = format!("{}: {}", context, message);
    }

    mapped_error
}