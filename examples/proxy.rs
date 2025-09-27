//! Example demonstrating proxy configuration with httpbin

use rsurlsession::{Client, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Example 1: Basic HTTP proxy
    println!("1. Testing HTTP proxy configuration...");
    let client = Client::builder()
        .http_proxy("proxy.example.com", 8080)
        .proxy_auth("username", "password")
        .build()?;

    // This would normally use the proxy, but since proxy.example.com doesn't exist,
    // we'll get a connection error - which proves the proxy config is working
    match client.get("https://httpbin.org/ip").send().await {
        Ok(response) => {
            println!("Response (unexpected success): {}", response.status());
            let text = response.text().await?;
            println!("Response body: {}", text);
        }
        Err(e) => {
            println!("Expected proxy connection error: {}", e);
        }
    }

    // Example 2: HTTPS proxy
    println!("\n2. Testing HTTPS proxy configuration...");
    let https_client = Client::builder()
        .https_proxy("secure-proxy.example.com", 8443)
        .proxy_auth("admin", "secret")
        .build()?;

    match https_client.get("https://httpbin.org/headers").send().await {
        Ok(response) => {
            println!("Response (unexpected success): {}", response.status());
        }
        Err(e) => {
            println!("Expected HTTPS proxy connection error: {}", e);
        }
    }

    // Example 3: SOCKS proxy
    println!("\n3. Testing SOCKS proxy configuration...");
    let socks_client = Client::builder()
        .socks_proxy("socks-proxy.example.com", 1080)
        .proxy_auth("socks_user", "socks_pass")
        .build()?;

    match socks_client
        .get("https://httpbin.org/user-agent")
        .send()
        .await
    {
        Ok(response) => {
            println!("Response (unexpected success): {}", response.status());
        }
        Err(e) => {
            println!("Expected SOCKS proxy connection error: {}", e);
        }
    }

    // Example 4: Multiple proxy types
    println!("\n4. Testing multiple proxy configuration...");
    let multi_client = Client::builder()
        .http_proxy("http-proxy.example.com", 8080)
        .https_proxy("https-proxy.example.com", 8443)
        .socks_proxy("socks-proxy.example.com", 1080)
        .proxy_auth("multi_user", "multi_pass")
        .build()?;

    match multi_client.get("https://httpbin.org/get").send().await {
        Ok(response) => {
            println!("Response (unexpected success): {}", response.status());
        }
        Err(e) => {
            println!("Expected multi-proxy connection error: {}", e);
        }
    }

    // Example 5: No proxy (direct connection) - this should work
    println!("\n5. Testing direct connection (no proxy)...");
    let direct_client = Client::builder()
        .user_agent("rsurlsession-proxy-example/1.0")
        .build()?;

    match direct_client.get("https://httpbin.org/get").send().await {
        Ok(response) => {
            println!(
                "Direct connection successful! Status: {}",
                response.status()
            );
            let text = response.text().await?;
            println!("Response shows no proxy was used:");

            // Parse the JSON to show relevant parts
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(headers) = json.get("headers") {
                    println!("User-Agent: {:?}", headers.get("User-Agent"));
                    println!("Host: {:?}", headers.get("Host"));
                }
                if let Some(origin) = json.get("origin") {
                    println!("Origin IP: {}", origin);
                }
            }
        }
        Err(e) => {
            println!("Direct connection failed: {}", e);
        }
    }

    println!("\nProxy configuration examples completed!");
    println!("Note: The proxy examples above use non-existent proxy servers,");
    println!("so they demonstrate configuration but will fail to connect.");
    println!("In real usage, replace with actual proxy server details.");

    Ok(())
}
