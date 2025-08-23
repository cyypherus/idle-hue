use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    println!("Project root: {}", project_root.display());

    // Get version from idle-hue package
    let version = get_app_version(&project_root)?;
    println!("App version: {version}");

    // Build all targets
    build_all_targets(&project_root)?;

    // Create zip files in target directory
    let zip_paths = create_zip_files(&project_root)?;

    // Upload using CLI if environment variables are set
    if let (Ok(server_url), Ok(api_key)) = (
        env::var("VERSION_SERVER_URL"),
        env::var("VERSION_SERVER_API_KEY"),
    ) {
        println!("Uploading to version server...");
        upload_to_server(&project_root, &version, &zip_paths, &server_url, &api_key)?;
    } else {
        println!("Skipping upload - VERSION_SERVER_URL or VERSION_SERVER_API_KEY not set");
        println!("Created zip files:");
        for (platform, path) in &zip_paths {
            println!("  {}: {}", platform, path.display());
        }
    }

    println!("Bundle process completed successfully!");
    Ok(())
}

fn get_app_version(project_root: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let cargo_toml_path = project_root.join("idle-hue/Cargo.toml");
    let cargo_toml_content = fs::read_to_string(&cargo_toml_path)?;

    for line in cargo_toml_content.lines() {
        if line.trim().starts_with("version") && line.contains("=") {
            let version = line
                .split("=")
                .nth(1)
                .ok_or("Invalid version line")?
                .trim()
                .trim_matches('"')
                .to_string();
            return Ok(version);
        }
    }

    Err("Version not found in Cargo.toml".into())
}

fn build_all_targets(project_root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("Building for Apple Silicon (ARM64)...");
    let arm_status = Command::new("cargo")
        .args([
            "bundle",
            "--release",
            "--bin",
            "idle-hue",
            "--package",
            "idle-hue",
            "--features",
            "prod",
        ])
        .current_dir(project_root)
        .status()?;

    if !arm_status.success() {
        return Err("Failed to build ARM64 bundle".into());
    }

    println!("Building for Intel (x86_64)...");
    let intel_status = Command::new("cargo")
        .args([
            "bundle",
            "--release",
            "--target",
            "x86_64-apple-darwin",
            "--bin",
            "idle-hue",
            "--package",
            "idle-hue",
            "--features",
            "prod",
        ])
        .current_dir(project_root)
        .status()?;

    if !intel_status.success() {
        return Err("Failed to build Intel bundle".into());
    }

    println!("Building for Windows (x86_64)...");
    let windows_status = Command::new("cargo")
        .args([
            "build",
            "--target",
            "x86_64-pc-windows-gnu",
            "--release",
            "--bin",
            "idle-hue",
            "--package",
            "idle-hue",
            "--features",
            "prod",
        ])
        .current_dir(project_root)
        .status()?;

    if !windows_status.success() {
        return Err("Failed to build Windows executable".into());
    }

    Ok(())
}

fn create_zip_files(
    project_root: &std::path::Path,
) -> Result<Vec<(String, PathBuf)>, Box<dyn std::error::Error>> {
    let target_dir = project_root.join("target");
    let mut zip_paths = Vec::new();

    // ARM64 macOS bundle
    let arm_bundle_path = target_dir.join("release/bundle/osx/idle-hue.app");
    let arm_zip_path = target_dir.join("idle-hue-macos-arm.zip");

    if !arm_bundle_path.exists() {
        return Err(format!("ARM bundle not found at {arm_bundle_path:?}").into());
    }

    println!("Creating ARM64 zip...");
    let arm_zip_status = Command::new("zip")
        .args(["-r", "idle-hue-macos-arm.zip", "idle-hue.app"])
        .current_dir(arm_bundle_path.parent().unwrap())
        .status()?;

    if !arm_zip_status.success() {
        return Err("Failed to create ARM64 zip".into());
    }

    // Move to target directory
    let arm_zip_src = arm_bundle_path
        .parent()
        .unwrap()
        .join("idle-hue-macos-arm.zip");
    if arm_zip_src.exists() {
        fs::rename(&arm_zip_src, &arm_zip_path)?;
        zip_paths.push(("macos-arm".to_string(), arm_zip_path));
    }

    // Intel macOS bundle
    let intel_bundle_path = target_dir.join("x86_64-apple-darwin/release/bundle/osx/idle-hue.app");
    let intel_zip_path = target_dir.join("idle-hue-macos-intel.zip");

    if !intel_bundle_path.exists() {
        return Err(format!("Intel bundle not found at {intel_bundle_path:?}").into());
    }

    println!("Creating Intel zip...");
    let intel_zip_status = Command::new("zip")
        .args(["-r", "idle-hue-macos-intel.zip", "idle-hue.app"])
        .current_dir(intel_bundle_path.parent().unwrap())
        .status()?;

    if !intel_zip_status.success() {
        return Err("Failed to create Intel zip".into());
    }

    // Move to target directory
    let intel_zip_src = intel_bundle_path
        .parent()
        .unwrap()
        .join("idle-hue-macos-intel.zip");
    if intel_zip_src.exists() {
        fs::rename(&intel_zip_src, &intel_zip_path)?;
        zip_paths.push(("macos-intel".to_string(), intel_zip_path));
    }

    // Windows executable
    let windows_exe_path = target_dir.join("x86_64-pc-windows-gnu/release/idle-hue.exe");
    let windows_zip_path = target_dir.join("idle-hue-windows-x86_64-gnu.zip");

    if !windows_exe_path.exists() {
        return Err(format!("Windows executable not found at {windows_exe_path:?}").into());
    }

    println!("Creating Windows zip...");
    let windows_zip_status = Command::new("zip")
        .args(["-j", "idle-hue-windows-x86_64-gnu.zip", "idle-hue.exe"])
        .current_dir(windows_exe_path.parent().unwrap())
        .status()?;

    if !windows_zip_status.success() {
        return Err("Failed to create Windows zip".into());
    }

    // Move to target directory
    let windows_zip_src = windows_exe_path
        .parent()
        .unwrap()
        .join("idle-hue-windows-x86_64-gnu.zip");
    if windows_zip_src.exists() {
        fs::rename(&windows_zip_src, &windows_zip_path)?;
        zip_paths.push(("windows-x86_64-gnu".to_string(), windows_zip_path));
    }

    Ok(zip_paths)
}

fn upload_to_server(
    project_root: &std::path::Path,
    version: &str,
    zip_paths: &[(String, PathBuf)],
    server_url: &str,
    api_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Build the CLI first
    println!("Building CLI...");
    let cli_build_status = Command::new("cargo")
        .args(["build", "--release", "--package", "cli"])
        .current_dir(project_root)
        .status()?;

    if !cli_build_status.success() {
        return Err("Failed to build CLI".into());
    }

    let cli_path = project_root.join("target/release/cli");
    if !cli_path.exists() {
        return Err("CLI executable not found".into());
    }

    // Construct upload command arguments
    let mut args = vec![
        "--url".to_string(),
        server_url.to_string(),
        "--api-key".to_string(),
        api_key.to_string(),
        "upload".to_string(),
        "idle-hue".to_string(),
        version.to_string(),
    ];

    // Add file arguments
    for (platform, path) in zip_paths {
        if path.exists() {
            args.push(format!("{platform}={}", path.display()));
        } else {
            eprintln!("Warning: Zip file not found: {}", path.display());
        }
    }

    println!(
        "Uploading with command: {} {}",
        cli_path.display(),
        args.join(" ")
    );

    let upload_status = Command::new(&cli_path)
        .args(&args)
        .current_dir(project_root)
        .status()?;

    if !upload_status.success() {
        return Err("Failed to upload to version server".into());
    }

    println!("Successfully uploaded version {version} to server");
    Ok(())
}
