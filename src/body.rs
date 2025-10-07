//! Request body types

use bytes::Bytes;
use std::borrow::Cow;

/// Request body content for HTTP requests.
///
/// This enum represents different types of request bodies that can be sent with HTTP requests.
/// It supports common content types including text, binary data, form data, JSON, and multipart forms.
///
/// # Examples
///
/// Creating different body types:
/// ```rust,no_run
/// use frakt::Body;
///
/// // Text body
/// let body = Body::text("Hello, World!");
///
/// // Binary body
/// let data = vec![1, 2, 3, 4];
/// let body = Body::bytes(data, "application/octet-stream");
///
/// // Form data
/// let body = Body::form(vec![
///     ("username", "john"),
///     ("password", "secret"),
/// ]);
/// ```
///
/// Using convenient `From` implementations:
/// ```rust,no_run
/// use frakt::{Client, Body};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
///
/// // `String` automatically converts to [`Body`]
/// let response = client
///     .post("https://api.example.com/data")?
///     .body("Hello, World!")
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub enum Body {
    /// Empty body
    Empty,

    /// Raw bytes with content type
    Bytes {
        /// The content
        content: Bytes,
        /// Content type
        content_type: String,
    },

    /// Form-encoded data
    Form {
        /// Form fields
        fields: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    },

    /// Multipart form data
    Multipart {
        /// Multipart parts
        parts: Vec<MultipartPart>,
    },

    /// JSON data
    Json {
        /// JSON value
        value: serde_json::Value,
    },
}

/// A single part of multipart form data.
///
/// This struct represents one field in a multipart/form-data request body.
/// It can contain either text data or file uploads with optional content type and filename.
///
/// # Examples
///
/// ```rust,no_run
/// use frakt::MultipartPart;
///
/// // Text field
/// let text_part = MultipartPart::text("description", "A sample file");
///
/// // File field
/// let file_data = vec![1, 2, 3, 4];
/// let file_part = MultipartPart::file(
///     "upload",
///     file_data,
///     "data.bin",
///     Some("application/octet-stream".to_string())
/// );
/// ```
#[derive(Debug, Clone)]
pub struct MultipartPart {
    /// Field name
    pub name: String,
    /// Content
    pub content: Bytes,
    /// Content type
    pub content_type: Option<String>,
    /// Filename
    pub filename: Option<String>,
}

impl Body {
    /// Create an empty body.
    ///
    /// This creates a body with no content, typically used for GET, HEAD, and DELETE requests.
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Create a body from raw bytes with a specific content type.
    ///
    /// # Arguments
    ///
    /// * `content` - The raw bytes to use as body content
    /// * `content_type` - The MIME type of the content
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use frakt::Body;
    ///
    /// let data = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG header
    /// let body = Body::bytes(data, "image/jpeg");
    /// ```
    pub fn bytes(content: impl Into<Bytes>, content_type: impl Into<String>) -> Self {
        Self::Bytes {
            content: content.into(),
            content_type: content_type.into(),
        }
    }

    /// Create a body from plain text.
    ///
    /// This sets the content type to `text/plain; charset=utf-8`.
    ///
    /// # Arguments
    ///
    /// * `content` - The text content to use as body
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use frakt::Body;
    ///
    /// let body = Body::text("Hello, World!");
    /// ```
    pub fn text(content: impl Into<String>) -> Self {
        Self::Bytes {
            content: content.into().into(),
            content_type: "text/plain; charset=utf-8".to_string(),
        }
    }

    /// Create a form-urlencoded body from field/value pairs.
    ///
    /// This creates a body with content type `application/x-www-form-urlencoded`
    /// and URL-encodes the provided field/value pairs.
    ///
    /// # Arguments
    ///
    /// * `fields` - A vector of (field_name, field_value) tuples
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use frakt::Body;
    ///
    /// let body = Body::form(vec![
    ///     ("username", "john_doe"),
    ///     ("password", "secret123"),
    ///     ("remember_me", "true"),
    /// ]);
    /// ```
    pub fn form(fields: Vec<(impl Into<Cow<'static, str>>, impl Into<Cow<'static, str>>)>) -> Self {
        Self::Form {
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// Create a JSON body from a serializable value.
    ///
    /// This serializes the provided value to JSON and sets the content type to
    /// `application/json`. This feature requires the "json" feature flag.
    ///
    /// # Arguments
    ///
    /// * `value` - Any value that implements `serde::Serialize`
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be serialized to JSON.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use frakt::Body;
    /// use serde_json::json;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let body = Body::json(json!({
    ///     "name": "John Doe",
    ///     "age": 30,
    ///     "active": true
    /// }))?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn json(value: impl serde::Serialize) -> Result<Self, crate::Error> {
        Ok(Self::Json {
            value: serde_json::to_value(value).map_err(|e| crate::Error::Json(e.to_string()))?,
        })
    }

    /// Create a multipart form-data body.
    ///
    /// This creates a body with content type `multipart/form-data` containing
    /// the provided parts. This feature requires the "multipart" feature flag.
    ///
    /// # Arguments
    ///
    /// * `parts` - A vector of multipart parts to include
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use frakt::{Body, MultipartPart};
    ///
    /// let parts = vec![
    ///     MultipartPart::text("description", "Profile image"),
    ///     MultipartPart::file("image", vec![1, 2, 3, 4], "avatar.jpg", Some("image/jpeg".to_string())),
    /// ];
    /// let body = Body::multipart(parts);
    /// ```
    pub fn multipart(parts: Vec<MultipartPart>) -> Self {
        Self::Multipart { parts }
    }

    /// Create a body by reading from a file.
    ///
    /// This method reads the entire file into memory and creates a bytes body.
    /// If no content type is provided, it defaults to `application/octet-stream`.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to read
    /// * `content_type` - Optional MIME type for the file content
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use frakt::Body;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let body = Body::from_file("image.jpg", Some("image/jpeg".to_string())).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_file<P: AsRef<std::path::Path>>(
        path: P,
        content_type: Option<String>,
    ) -> Result<Self, crate::Error> {
        let content = tokio::fs::read(path).await?;
        let content_type = content_type.unwrap_or_else(|| "application/octet-stream".to_string());

        Ok(Self::Bytes {
            content: content.into(),
            content_type,
        })
    }
}

// Convenience From implementations
impl From<String> for Body {
    fn from(content: String) -> Self {
        Self::text(content)
    }
}

impl From<&str> for Body {
    fn from(content: &str) -> Self {
        Self::text(content)
    }
}

impl From<Vec<u8>> for Body {
    fn from(content: Vec<u8>) -> Self {
        Self::bytes(content, "application/octet-stream")
    }
}

impl From<&[u8]> for Body {
    fn from(content: &[u8]) -> Self {
        Self::bytes(content.to_vec(), "application/octet-stream")
    }
}

impl From<Bytes> for Body {
    fn from(content: Bytes) -> Self {
        Self::bytes(content, "application/octet-stream")
    }
}

impl From<serde_json::Value> for Body {
    fn from(value: serde_json::Value) -> Self {
        Self::Json { value }
    }
}

impl MultipartPart {
    /// Create a text part
    pub fn text(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            content: content.into().into(),
            content_type: Some("text/plain; charset=utf-8".to_string()),
            filename: None,
        }
    }

    /// Create a file part
    pub fn file(
        name: impl Into<String>,
        content: impl Into<Bytes>,
        filename: impl Into<String>,
        content_type: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
            content_type,
            filename: Some(filename.into()),
        }
    }

    /// Create a file part from a file path
    pub async fn from_file<P: AsRef<std::path::Path>>(
        name: impl Into<String>,
        path: P,
        content_type: Option<String>,
    ) -> Result<Self, crate::Error> {
        let content = tokio::fs::read(&path).await?;
        let filename = path
            .as_ref()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_string();

        Ok(Self {
            name: name.into(),
            content: content.into(),
            content_type,
            filename: Some(filename),
        })
    }
}
