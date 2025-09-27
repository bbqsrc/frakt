//! Request body types

use bytes::Bytes;
use std::borrow::Cow;

/// Request body types
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
    #[cfg(feature = "multipart")]
    Multipart {
        /// Multipart parts
        parts: Vec<MultipartPart>,
    },

    /// JSON data
    #[cfg(feature = "json")]
    Json {
        /// JSON value
        value: serde_json::Value,
    },
}

/// A part of multipart form data
#[cfg(feature = "multipart")]
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
    /// Create an empty body
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Create a body from bytes
    pub fn bytes(content: impl Into<Bytes>, content_type: impl Into<String>) -> Self {
        Self::Bytes {
            content: content.into(),
            content_type: content_type.into(),
        }
    }

    /// Create a body from text
    pub fn text(content: impl Into<String>) -> Self {
        Self::Bytes {
            content: content.into().into(),
            content_type: "text/plain; charset=utf-8".to_string(),
        }
    }

    /// Create a form body
    pub fn form(fields: Vec<(impl Into<Cow<'static, str>>, impl Into<Cow<'static, str>>)>) -> Self {
        Self::Form {
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// Create a JSON body
    #[cfg(feature = "json")]
    pub fn json(value: impl serde::Serialize) -> Result<Self, crate::Error> {
        Ok(Self::Json {
            value: serde_json::to_value(value)?,
        })
    }

    /// Create a multipart body
    #[cfg(feature = "multipart")]
    pub fn multipart(parts: Vec<MultipartPart>) -> Self {
        Self::Multipart { parts }
    }

    /// Create a body from a file
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

#[cfg(feature = "json")]
impl From<serde_json::Value> for Body {
    fn from(value: serde_json::Value) -> Self {
        Self::Json { value }
    }
}

#[cfg(feature = "multipart")]
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
