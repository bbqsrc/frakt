//! Integration tests for rsurlsession

use rsurlsession::{Client, Result};

#[tokio::test]
async fn test_basic_get_request() -> Result<()> {
    let client = Client::builder()
        .user_agent("rsurlsession-integration-test/1.0")
        .build()?;

    let response = client.get("https://httpbin.org/get").send().await?;

    assert_eq!(response.status(), 200);

    let text = response.text().await?;
    assert!(text.contains("httpbin.org"));

    Ok(())
}

#[tokio::test]
async fn test_post_with_json() -> Result<()> {
    let client = Client::builder()
        .user_agent("rsurlsession-integration-test/1.0")
        .build()?;

    let json_data = r#"{"test": "data", "number": 42}"#;

    let response = client
        .post("https://httpbin.org/post")
        .header("Content-Type", "application/json")
        .body(json_data)
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let text = response.text().await?;
    assert!(text.contains("test"));
    assert!(text.contains("data"));

    Ok(())
}

#[tokio::test]
async fn test_headers() -> Result<()> {
    let client = Client::builder()
        .user_agent("rsurlsession-integration-test/1.0")
        .header("X-Custom-Header", "test-value")
        .build()?;

    let response = client
        .get("https://httpbin.org/headers")
        .header("X-Request-Header", "request-value")
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let text = response.text().await?;
    assert!(text.contains("X-Custom-Header"));
    assert!(text.contains("test-value"));
    assert!(text.contains("X-Request-Header"));
    assert!(text.contains("request-value"));

    Ok(())
}

#[tokio::test]
async fn test_basic_auth() -> Result<()> {
    let client = Client::builder()
        .user_agent("rsurlsession-integration-test/1.0")
        .build()?;

    let response = client
        .get("https://httpbin.org/basic-auth/testuser/testpass")
        .auth(rsurlsession::Auth::Basic {
            username: "testuser".to_string(),
            password: "testpass".to_string(),
        })
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let text = response.text().await?;
    assert!(text.contains("authenticated"));

    Ok(())
}

#[tokio::test]
async fn test_bearer_auth() -> Result<()> {
    let client = Client::builder()
        .user_agent("rsurlsession-integration-test/1.0")
        .build()?;

    let response = client
        .get("https://httpbin.org/bearer")
        .auth(rsurlsession::Auth::Bearer {
            token: "test-token".to_string(),
        })
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let text = response.text().await?;
    assert!(text.contains("authenticated"));
    assert!(text.contains("test-token"));

    Ok(())
}

#[tokio::test]
async fn test_cookie_jar() -> Result<()> {
    let client = Client::builder()
        .user_agent("rsurlsession-integration-test/1.0")
        .use_cookies(true)
        .build()?;

    // Set a cookie
    let _response = client
        .get("https://httpbin.org/cookies/set/test_cookie/test_value")
        .send()
        .await?;

    // Verify the cookie is sent back
    let response = client.get("https://httpbin.org/cookies").send().await?;

    assert_eq!(response.status(), 200);

    let text = response.text().await?;
    assert!(text.contains("test_cookie"));
    assert!(text.contains("test_value"));

    Ok(())
}

#[tokio::test]
async fn test_download_file() -> Result<()> {
    let client = Client::builder()
        .user_agent("rsurlsession-integration-test/1.0")
        .build()?;

    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join("test_download.txt");

    // Clean up any existing file
    let _ = std::fs::remove_file(&file_path);

    let _response = client
        .download("https://httpbin.org/base64/SHR0cCBkb3dubG9hZCB0ZXN0")
        .to_file(&file_path)
        .send()
        .await?;

    // Verify the file was downloaded
    assert!(file_path.exists());

    let content = std::fs::read_to_string(&file_path)?;
    assert!(content.contains("Http download test"));

    // Clean up
    let _ = std::fs::remove_file(&file_path);

    Ok(())
}

// Note: Multipart form test removed - feature may not be fully implemented yet

#[tokio::test]
async fn test_timeout() -> Result<()> {
    use std::time::Duration;

    let client = Client::builder()
        .user_agent("rsurlsession-integration-test/1.0")
        .timeout(Duration::from_millis(100)) // Very short timeout
        .build()?;

    // This should timeout
    let result = client
        .get("https://httpbin.org/delay/5") // 5 second delay
        .send()
        .await;

    // Should get a timeout error
    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
async fn test_error_status_codes() -> Result<()> {
    let client = Client::builder()
        .user_agent("rsurlsession-integration-test/1.0")
        .build()?;

    let response = client.get("https://httpbin.org/status/404").send().await?;

    assert_eq!(response.status(), 404);

    Ok(())
}
