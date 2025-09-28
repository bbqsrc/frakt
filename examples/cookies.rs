//! Example demonstrating cookie management with httpbin

use rsurlsession::{Client, Cookie, CookieJar, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Create a custom cookie jar
    let cookie_jar = CookieJar::new();

    // Add a custom cookie
    let custom_cookie = Cookie::new("test_cookie", "test_value")
        .domain("httpbin.org")
        .path("/")
        .secure(true);

    cookie_jar.add_cookie(custom_cookie)?;

    // Create client with the cookie jar
    let client = Client::builder().cookie_jar(cookie_jar).build()?;

    // 1. Set cookies via httpbin
    println!("Setting cookies via httpbin...");
    let response = client
        .get("https://httpbin.org/cookies/set/session_id/abc123")?
        .send()
        .await?;

    println!("Set cookies response status: {}", response.status());

    // 2. Get all cookies
    if let Some(jar) = client.cookie_jar() {
        let cookies = jar.all_cookies();
        println!("\nAll cookies in jar:");
        for cookie in &cookies {
            println!(
                "  {}={} (domain: {}, path: {})",
                cookie.name, cookie.value, cookie.domain, cookie.path
            );
        }

        // 3. Get cookies for specific URL
        let httpbin_cookies = jar.cookies_for_url("https://httpbin.org/")?;
        println!("\nCookies for httpbin.org:");
        for cookie in &httpbin_cookies {
            println!("  {}={}", cookie.name, cookie.value);
        }
    }

    // 4. Make request that should include cookies
    println!("\nMaking request to check cookies...");
    let response = client.get("https://httpbin.org/cookies")?.send().await?;

    let text = response.text().await?;
    println!("Cookies response: {}", text);

    // 5. Test with JSON endpoint that requires cookies
    println!("\nTesting JSON response with cookies...");
    let response = client.get("https://httpbin.org/json")?.send().await?;

    let json_text = response.text().await?;
    println!("JSON response: {}", json_text);

    // 6. Clear cookies
    if let Some(jar) = client.cookie_jar() {
        println!("\nClearing all cookies...");
        jar.clear();

        let remaining_cookies = jar.all_cookies();
        println!("Remaining cookies: {}", remaining_cookies.len());
    }

    Ok(())
}
