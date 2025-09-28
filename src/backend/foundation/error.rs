//! Foundation error handling

use crate::Error;

impl Error {
    /// Convert NSError to our Error type (Foundation backend specific)
    pub(crate) fn from_ns_error(error: &objc2_foundation::NSError) -> Self {
        use objc2_foundation::NSURLErrorDomain;

        let domain = error.domain();
        let code = error.code();
        let message = unsafe {
            objc2::rc::autoreleasepool(|pool| error.localizedDescription().to_str(pool).to_string())
        };

        if unsafe { domain.isEqualToString(&NSURLErrorDomain) } {
            match code {
                -1001 => Error::Timeout,
                -999 => Error::Cancelled,
                -1200..=-1000 => Error::Tls { message },
                _ => Error::Network {
                    code: code.try_into().unwrap(),
                    message,
                },
            }
        } else {
            Error::Internal(format!(
                "Domain: {}, Code: {}, Message: {}",
                unsafe { objc2::rc::autoreleasepool(|pool| domain.to_str(pool).to_string()) },
                code,
                message
            ))
        }
    }
}
