//! Authentication support for HTTP requests

use std::fmt;

/// Authentication methods supported by HTTP requests.
///
/// This enum provides common authentication schemes that can be used with HTTP requests.
/// Each variant automatically generates the appropriate `Authorization` header value.
///
/// # Examples
///
/// ```rust,no_run
/// use rsurlsession::{Client, Auth};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
///
/// // Basic authentication
/// let response = client
///     .get("https://httpbin.org/basic-auth/user/pass")?
///     .auth(Auth::basic("user", "pass"))
///     .send()
///     .await?;
///
/// // Bearer token
/// let response = client
///     .get("https://api.example.com/protected")?
///     .auth(Auth::bearer("your-jwt-token"))
///     .send()
///     .await?;
///
/// // Custom authentication
/// let response = client
///     .get("https://api.example.com/data")?
///     .auth(Auth::custom("ApiKey", "your-api-key"))
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub enum Auth {
    /// HTTP Basic authentication with username and password.
    ///
    /// This creates an `Authorization: Basic <base64(username:password)>` header.
    Basic {
        /// Username for basic authentication
        username: String,
        /// Password for basic authentication
        password: String,
    },
    /// Bearer token authentication (OAuth, JWT, etc.).
    ///
    /// This creates an `Authorization: Bearer <token>` header.
    Bearer {
        /// Bearer token
        token: String,
    },
    /// Custom authorization header with a custom scheme.
    ///
    /// This creates an `Authorization: <scheme> <credentials>` header.
    Custom {
        /// Authentication scheme (e.g., "ApiKey", "Digest")
        scheme: String,
        /// Credentials for the scheme
        credentials: String,
    },
}

impl Auth {
    /// Create HTTP Basic authentication.
    ///
    /// This method creates Basic authentication using the provided username and password.
    /// The credentials will be base64-encoded when the header value is generated.
    ///
    /// # Arguments
    ///
    /// * `username` - The username for authentication
    /// * `password` - The password for authentication
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Auth;
    ///
    /// let auth = Auth::basic("john_doe", "secret123");
    /// ```
    pub fn basic(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self::Basic {
            username: username.into(),
            password: password.into(),
        }
    }

    /// Create Bearer token authentication.
    ///
    /// This method creates Bearer token authentication using the provided token.
    /// This is commonly used for OAuth 2.0, JWT tokens, and API keys.
    ///
    /// # Arguments
    ///
    /// * `token` - The bearer token
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Auth;
    ///
    /// let auth = Auth::bearer("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...");
    /// ```
    pub fn bearer(token: impl Into<String>) -> Self {
        Self::Bearer {
            token: token.into(),
        }
    }

    /// Create custom authentication with a custom scheme.
    ///
    /// This method allows you to create authentication using any custom scheme
    /// and credentials. The Authorization header will be formatted as `<scheme> <credentials>`.
    ///
    /// # Arguments
    ///
    /// * `scheme` - The authentication scheme (e.g., "ApiKey", "Digest", "HMAC")
    /// * `credentials` - The credentials for the scheme
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Auth;
    ///
    /// let auth = Auth::custom("ApiKey", "your-api-key-here");
    /// let auth = Auth::custom("Digest", "username=\"john\", realm=\"api\"");
    /// ```
    pub fn custom(scheme: impl Into<String>, credentials: impl Into<String>) -> Self {
        Self::Custom {
            scheme: scheme.into(),
            credentials: credentials.into(),
        }
    }

    /// Convert authentication to Authorization header value.
    ///
    /// This method generates the complete value for the `Authorization` HTTP header
    /// based on the authentication type. The resulting string can be used directly
    /// as the header value.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Auth;
    ///
    /// let basic_auth = Auth::basic("user", "pass");
    /// assert_eq!(basic_auth.to_header_value(), "Basic dXNlcjpwYXNz");
    ///
    /// let bearer_auth = Auth::bearer("token123");
    /// assert_eq!(bearer_auth.to_header_value(), "Bearer token123");
    ///
    /// let custom_auth = Auth::custom("ApiKey", "secret");
    /// assert_eq!(custom_auth.to_header_value(), "ApiKey secret");
    /// ```
    pub fn to_header_value(&self) -> String {
        match self {
            Auth::Basic { username, password } => {
                let credentials = format!("{}:{}", username, password);
                let encoded = base64_encode(credentials.as_bytes());
                format!("Basic {}", encoded)
            }
            Auth::Bearer { token } => {
                format!("Bearer {}", token)
            }
            Auth::Custom {
                scheme,
                credentials,
            } => {
                format!("{} {}", scheme, credentials)
            }
        }
    }
}

impl fmt::Display for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Auth::Basic { username, .. } => {
                write!(f, "Basic authentication for user: {}", username)
            }
            Auth::Bearer { .. } => write!(f, "Bearer token authentication"),
            Auth::Custom { scheme, .. } => write!(f, "Custom {} authentication", scheme),
        }
    }
}

/// Simple base64 encoding implementation
/// This avoids adding another dependency and is sufficient for Basic auth
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();

    let mut i = 0;
    while i < input.len() {
        let b1 = input[i];
        let b2 = if i + 1 < input.len() { input[i + 1] } else { 0 };
        let b3 = if i + 2 < input.len() { input[i + 2] } else { 0 };

        let bitmap = ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);

        result.push(ALPHABET[((bitmap >> 18) & 63) as usize] as char);
        result.push(ALPHABET[((bitmap >> 12) & 63) as usize] as char);

        if i + 1 < input.len() {
            result.push(ALPHABET[((bitmap >> 6) & 63) as usize] as char);
        } else {
            result.push('=');
        }

        if i + 2 < input.len() {
            result.push(ALPHABET[(bitmap & 63) as usize] as char);
        } else {
            result.push('=');
        }

        i += 3;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_auth() {
        let auth = Auth::basic("user", "pass");
        assert_eq!(auth.to_header_value(), "Basic dXNlcjpwYXNz");
    }

    #[test]
    fn test_bearer_auth() {
        let auth = Auth::bearer("token123");
        assert_eq!(auth.to_header_value(), "Bearer token123");
    }

    #[test]
    fn test_custom_auth() {
        let auth = Auth::custom("ApiKey", "secret123");
        assert_eq!(auth.to_header_value(), "ApiKey secret123");
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64_encode(b"test"), "dGVzdA==");
    }
}
