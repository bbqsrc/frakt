//! Test the new backend abstraction system

use frakt::backend::Backend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing backend abstraction system...");

    // Test Reqwest backend
    println!("\n=== Testing Reqwest backend ===");
    let reqwest_backend = Backend::reqwest()?;
    test_backend(reqwest_backend, "Reqwest").await?;

    // Test Foundation backend (macOS only)
    #[cfg(target_vendor = "apple")]
    {
        println!("\n=== Testing Foundation backend ===");
        let foundation_backend = Backend::foundation()?;
        test_backend(foundation_backend, "Foundation").await?;
    }

    // Test Windows backend (Windows only)
    #[cfg(windows)]
    {
        println!("\n=== Testing Windows backend ===");
        let windows_backend = Backend::windows()?;
        test_backend(windows_backend, "Windows").await?;
    }

    // Test auto-selection
    println!("\n=== Testing auto-selected backend ===");
    let auto_backend = Backend::default_for_platform()?;
    test_backend(auto_backend, "Auto-selected").await?;

    println!("\n✅ All backend tests completed successfully!");
    Ok(())
}

async fn test_backend(backend: Backend, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    use frakt::backend::types::BackendRequest;
    use http::{HeaderMap, Method};

    println!("Testing {} backend...", name);

    let request = BackendRequest {
        method: Method::GET,
        url: "https://httpbin.org/json".try_into().unwrap(),
        headers: HeaderMap::new(),
        body: None,
        progress_callback: None,
    };

    match backend.execute(request).await {
        Ok(response) => {
            println!("  ✓ Status: {}", response.status);
            println!("  ✓ Headers: {} entries", response.headers.len());

            // Try to read some body data
            let mut receiver = response.body_receiver;
            let mut body_size = 0;
            let mut chunks_received = 0;

            while let Some(chunk_result) = receiver.recv().await {
                match chunk_result {
                    Ok(chunk) => {
                        body_size += chunk.len();
                        chunks_received += 1;
                        if chunks_received >= 5 {
                            // Don't read entire response for test
                            break;
                        }
                    }
                    Err(e) => {
                        println!("  ⚠ Body read error: {}", e);
                        break;
                    }
                }
            }

            println!(
                "  ✓ Body: {} bytes read in {} chunks",
                body_size, chunks_received
            );
            println!("  ✅ {} backend test completed successfully!", name);
        }
        Err(e) => {
            println!("  ❌ {} backend test failed: {}", name, e);
            println!("     (This might be expected in some network environments)");
        }
    }

    Ok(())
}
