use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // Only compile Java classes when building for Android
    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("android") {
        return;
    }

    println!("cargo:rerun-if-changed=java/");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let java_dir = Path::new("java");

    // Find javac
    let javac = find_javac().expect("javac not found. Please install JDK and set JAVA_HOME");

    // Compile Java sources
    let sources = vec![
        // Cronet stubs
        java_dir.join("stubs/org/chromium/net/UrlRequest.java"),
        java_dir.join("stubs/org/chromium/net/UrlResponseInfo.java"),
        java_dir.join("stubs/org/chromium/net/CronetException.java"),
        java_dir.join("stubs/org/chromium/net/UploadDataProvider.java"),
        java_dir.join("stubs/org/chromium/net/UploadDataSink.java"),
        // Java NIO stubs
        java_dir.join("stubs/java/nio/ByteBuffer.java"),
        // Android stubs
        java_dir.join("stubs/android/content/Context.java"),
        java_dir.join("stubs/android/os/Build.java"),
        java_dir.join("stubs/android/app/Notification.java"),
        java_dir.join("stubs/android/app/NotificationChannel.java"),
        java_dir.join("stubs/android/app/NotificationManager.java"),
        java_dir.join("stubs/android/R.java"),
        // AndroidX stubs
        java_dir.join("stubs/androidx/annotation/NonNull.java"),
        java_dir.join("stubs/androidx/work/Data.java"),
        java_dir.join("stubs/androidx/work/WorkerParameters.java"),
        java_dir.join("stubs/androidx/work/ListenableWorker.java"),
        java_dir.join("stubs/androidx/work/Worker.java"),
        java_dir.join("stubs/androidx/work/WorkerFactory.java"),
        java_dir.join("stubs/androidx/work/Configuration.java"),
        java_dir.join("stubs/androidx/work/WorkInfo.java"),
        java_dir.join("stubs/androidx/work/ForegroundInfo.java"),
        java_dir.join("stubs/androidx/core/app/NotificationCompat.java"),
        // Guava stubs (for WorkManager)
        java_dir.join("stubs/com/google/common/util/concurrent/ListenableFuture.java"),
        // Our callback implementation
        java_dir.join("se/brendan/frakt/RustUrlRequestCallback.java"),
        // WorkManager download components
        java_dir.join("se/brendan/frakt/DownloadWorker.java"),
        java_dir.join("se/brendan/frakt/DownloadProgressCallback.java"),
        java_dir.join("se/brendan/frakt/DexWorkerFactory.java"),
        java_dir.join("se/brendan/frakt/BackgroundDownloader.java"),
        // Upload progress tracking
        java_dir.join("se/brendan/frakt/ProgressTrackingUploadDataProvider.java"),
    ];

    let status = Command::new(&javac)
        .arg("-d")
        .arg(&out_dir)
        .arg("-source")
        .arg("8")
        .arg("-target")
        .arg("8")
        .arg("-sourcepath")
        .arg(java_dir.join("stubs"))
        .arg("-sourcepath")
        .arg(java_dir)
        .args(&sources)
        .status()
        .expect("Failed to run javac");

    if !status.success() {
        panic!("javac failed to compile Java sources");
    }

    println!(
        "cargo:warning=Compiled Java callback class to {}",
        out_dir.display()
    );

    // Convert .class files to .dex format for Android
    convert_to_dex(&out_dir);
}

fn convert_to_dex(class_dir: &Path) {
    // Find d8 tool from Android SDK
    let d8 = find_d8().expect(
        "d8 not found. Please install Android SDK and set ANDROID_HOME or ANDROID_SDK_ROOT",
    );

    let dex_output = class_dir.join("classes.dex");

    // Collect all .class files recursively
    let mut class_files = Vec::new();
    fn collect_class_files(dir: &Path, files: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect_class_files(&path, files);
                } else if path.extension().and_then(|s| s.to_str()) == Some("class") {
                    files.push(path);
                }
            }
        }
    }
    collect_class_files(&class_dir, &mut class_files);

    // Run d8 to convert all class files to DEX
    let mut d8_command = Command::new(&d8);
    d8_command
        .arg("--output")
        .arg(class_dir)
        .arg("--min-api")
        .arg("21"); // Minimum API level

    for class_file in &class_files {
        d8_command.arg(class_file);
    }

    let status = d8_command.status().expect("Failed to run d8");

    if !status.success() {
        panic!("d8 failed to convert class files to DEX");
    }

    println!("cargo:warning=Converted to DEX: {}", dex_output.display());
}

fn find_d8() -> Option<PathBuf> {
    // Try ANDROID_HOME first
    if let Ok(android_home) = env::var("ANDROID_HOME") {
        let d8 = PathBuf::from(android_home).join("build-tools");
        if d8.exists() {
            // Find the latest build-tools version
            if let Ok(entries) = std::fs::read_dir(&d8) {
                let mut versions: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .collect();
                versions.sort_by(|a, b| b.path().cmp(&a.path()));

                if let Some(latest) = versions.first() {
                    let d8_path = latest.path().join("d8");
                    if d8_path.exists() {
                        return Some(d8_path);
                    }
                }
            }
        }
    }

    // Try ANDROID_SDK_ROOT
    if let Ok(sdk_root) = env::var("ANDROID_SDK_ROOT") {
        let d8 = PathBuf::from(sdk_root).join("build-tools");
        if d8.exists() {
            if let Ok(entries) = std::fs::read_dir(&d8) {
                let mut versions: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .collect();
                versions.sort_by(|a, b| b.path().cmp(&a.path()));

                if let Some(latest) = versions.first() {
                    let d8_path = latest.path().join("d8");
                    if d8_path.exists() {
                        return Some(d8_path);
                    }
                }
            }
        }
    }

    None
}

fn find_javac() -> Option<PathBuf> {
    // Try JAVA_HOME first
    if let Ok(java_home) = env::var("JAVA_HOME") {
        let javac = PathBuf::from(java_home).join("bin").join("javac");
        if javac.exists() {
            return Some(javac);
        }
    }

    // Try PATH
    if let Ok(output) = Command::new("which").arg("javac").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }

    // Try common locations on macOS
    let macos_java = Path::new("/usr/libexec/java_home");
    if macos_java.exists() {
        if let Ok(output) = Command::new(macos_java).output() {
            if output.status.success() {
                let java_home = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let javac = PathBuf::from(java_home).join("bin").join("javac");
                if javac.exists() {
                    return Some(javac);
                }
            }
        }
    }

    None
}
