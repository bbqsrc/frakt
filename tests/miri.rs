//! Miri tests for unsafe code in frakt
//!
//! This file contains tests that run only under miri to check for undefined behavior
//! in the unsafe code paths that don't require network access or the full Objective-C runtime.

#![cfg(miri)]

use frakt::Body;

#[test]
fn test_method_as_str() {
    // Test Method enum string conversion using http::Method
    use http::Method;

    assert_eq!(Method::GET.as_str(), "GET");
    assert_eq!(Method::POST.as_str(), "POST");
    assert_eq!(Method::PUT.as_str(), "PUT");
    assert_eq!(Method::DELETE.as_str(), "DELETE");
    assert_eq!(Method::PATCH.as_str(), "PATCH");
    assert_eq!(Method::HEAD.as_str(), "HEAD");
}

#[test]
fn test_body_text_creation() {
    // Test Body::text creation which uses string to bytes conversion
    let body = Body::text("Hello, World!");

    match body {
        Body::Bytes {
            content,
            content_type,
        } => {
            assert_eq!(content.as_ref(), b"Hello, World!");
            assert_eq!(content_type, "text/plain; charset=utf-8");
        }
        _ => panic!("Expected Body::Bytes variant"),
    }
}

#[test]
fn test_body_bytes_creation() {
    // Test Body::bytes creation with raw data
    let data = vec![0x48, 0x65, 0x6c, 0x6c, 0x6f]; // "Hello" in ASCII
    let body = Body::bytes(data.clone(), "application/octet-stream");

    match body {
        Body::Bytes {
            content,
            content_type,
        } => {
            assert_eq!(content.as_ref(), &data);
            assert_eq!(content_type, "application/octet-stream");
        }
        _ => panic!("Expected Body::Bytes variant"),
    }
}

#[test]
fn test_body_form_creation() {
    // Test form data creation and encoding
    let fields = vec![
        ("username", "john_doe"),
        ("password", "secret123"),
        ("remember", "true"),
    ];
    let body = Body::form(fields);

    match body {
        Body::Form { fields } => {
            assert_eq!(fields.len(), 3);
            assert_eq!(fields[0].0, "username");
            assert_eq!(fields[0].1, "john_doe");
            assert_eq!(fields[1].0, "password");
            assert_eq!(fields[1].1, "secret123");
            assert_eq!(fields[2].0, "remember");
            assert_eq!(fields[2].1, "true");
        }
        _ => panic!("Expected Body::Form variant"),
    }
}

#[test]
fn test_body_json_creation() {
    // Test JSON body creation
    use serde_json::json;

    let json_value = json!({
        "name": "John Doe",
        "age": 30,
        "active": true
    });

    let body = Body::json(json_value.clone()).expect("Failed to create JSON body");

    match body {
        Body::Json { value } => {
            assert_eq!(value["name"], "John Doe");
            assert_eq!(value["age"], 30);
            assert_eq!(value["active"], true);
        }
        _ => panic!("Expected Body::Json variant"),
    }
}

#[test]
fn test_multipart_part_creation() {
    // Test multipart form part creation
    use frakt::MultipartPart;

    let part = MultipartPart::file(
        "file",
        vec![1, 2, 3, 4, 5],
        "test.bin",
        Some("application/octet-stream".to_string()),
    );

    assert_eq!(part.name, "file");
    assert_eq!(part.content.as_ref(), &[1, 2, 3, 4, 5]);
    assert_eq!(
        part.content_type.as_ref().unwrap(),
        "application/octet-stream"
    );
    assert_eq!(part.filename.as_ref().unwrap(), "test.bin");
}

#[test]
fn test_string_conversions() {
    // Test string to bytes conversions in Body
    let long_string = "Long string: ".repeat(1000);
    let test_strings = [
        "Hello, World!",
        "Unicode: 世界 🌍 Здравствуй мир",
        "",
        long_string.as_str(),
    ];

    for test_str in test_strings {
        let body = Body::text(test_str);
        match body {
            Body::Bytes {
                content,
                content_type,
            } => {
                assert_eq!(content_type, "text/plain; charset=utf-8");
                let roundtrip = String::from_utf8(content.to_vec()).expect("Invalid UTF-8");
                assert_eq!(roundtrip, test_str);
            }
            _ => panic!("Expected Body::Bytes variant"),
        }
    }
}

#[test]
fn test_large_data_handling() {
    // Test handling of larger data buffers to catch potential overflow issues
    let large_data = vec![0x42; 1024 * 1024]; // 1MB of data
    let body = Body::bytes(large_data.clone(), "application/octet-stream");

    match body {
        Body::Bytes { content, .. } => {
            assert_eq!(content.len(), 1024 * 1024);
            assert_eq!(content[0], 0x42);
            assert_eq!(content[1024 * 1024 - 1], 0x42);
        }
        _ => panic!("Expected Body::Bytes variant"),
    }
}

#[test]
fn test_edge_cases() {
    // Test empty and edge case values for Body
    let empty_body = Body::empty();
    assert!(matches!(empty_body, Body::Empty));

    let empty_text_body = Body::text("");
    match empty_text_body {
        Body::Bytes { content, .. } => assert_eq!(content.len(), 0),
        _ => panic!("Expected Body::Bytes variant"),
    }

    let empty_bytes_body = Body::bytes(Vec::new(), "application/octet-stream");
    match empty_bytes_body {
        Body::Bytes { content, .. } => assert_eq!(content.len(), 0),
        _ => panic!("Expected Body::Bytes variant"),
    }
}

#[test]
fn test_from_conversions() {
    // Test From trait implementations for Body
    let string_body: Body = "Hello World".into();
    assert!(matches!(string_body, Body::Bytes { .. }));

    let string_owned: Body = String::from("Hello World").into();
    assert!(matches!(string_owned, Body::Bytes { .. }));

    let vec_body: Body = vec![1, 2, 3, 4].into();
    assert!(matches!(vec_body, Body::Bytes { .. }));

    let slice_body: Body = [1, 2, 3, 4].as_slice().into();
    assert!(matches!(slice_body, Body::Bytes { .. }));
}
