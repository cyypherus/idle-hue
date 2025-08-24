use clap::Parser;
use dotenv::dotenv;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use version_api_models::{VERSION_SERVER_DEV, VERSION_SERVER_PROD};

#[derive(Parser)]
#[command(name = "bundler")]
#[command(about = "Bundle and optionally sign/upload idle-hue applications")]
struct Args {
    /// Skip uploading to version server
    #[arg(long)]
    skip_upload: bool,
    /// Upload to production server
    #[arg(long)]
    upload_prod: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Load .env file
    dotenv().unwrap();

    let project_root = std::env::current_dir()?;
    println!("Project root: {}", project_root.display());

    // Get version from idle-hue package
    let version = get_app_version(&project_root)?;
    println!("App version: {version}");

    // Build all targets
    build_all_targets(&project_root)?;

    // Create zip files in target directory
    let mut zip_paths = create_zip_files(&project_root)?;

    // Sign and notarize macOS apps if signing credentials are available
    if let (Ok(_), Ok(_), Ok(_)) = (
        env::var("APPLE_TEAM_ID"),
        env::var("APPLE_ID"),
        env::var("APPLE_APP_SPECIFIC_PASSWORD"),
    ) {
        println!("Signing credentials found, processing macOS apps...");
        sign_and_notarize_macos_apps(&project_root, &mut zip_paths)?;
    } else {
        println!("Skipping code signing - Apple credentials not set in .env");
    }

    // Upload using CLI if environment variables are set and not skipped
    if args.skip_upload {
        println!("Skipping upload - --skip-upload flag provided");
        println!("Created zip files:");
        for (platform, path) in &zip_paths {
            println!("  {}: {}", platform, path.display());
        }
    } else if let Ok(api_key) = env::var("VERSION_SERVER_API_KEY") {
        println!("Uploading to version server...");
        upload_to_server(
            &project_root,
            &version,
            &zip_paths,
            if args.upload_prod {
                VERSION_SERVER_PROD
            } else {
                VERSION_SERVER_DEV
            },
            &api_key,
        )?;
    } else {
        println!("Skipping upload - VERSION_SERVER_API_KEY not set");
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

fn sign_and_notarize_macos_apps(
    project_root: &std::path::Path,
    zip_paths: &mut [(String, PathBuf)],
) -> Result<(), Box<dyn std::error::Error>> {
    let team_id = env::var("APPLE_TEAM_ID")?;
    let apple_id = env::var("APPLE_ID")?;
    let app_password = env::var("APPLE_APP_SPECIFIC_PASSWORD")?;

    // Get signing identity
    let identity_output = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()?;

    if !identity_output.status.success() {
        return Err("Failed to find code signing identities".into());
    }

    let identity_str = String::from_utf8_lossy(&identity_output.stdout);
    let identity = identity_str
        .lines()
        .find(|line| line.contains("Developer ID Application"))
        .and_then(|line| {
            // Extract text between quotes
            let start = line.find('"')?;
            let end = line.rfind('"')?;
            if start < end {
                Some(&line[start + 1..end])
            } else {
                None
            }
        })
        .ok_or("No Developer ID Application certificate found")?;

    println!("Using signing identity: {identity}");

    let target_dir = project_root.join("target");

    // Process ARM64 macOS bundle
    let arm_bundle_path = target_dir.join("release/bundle/osx/idle-hue.app");
    if arm_bundle_path.exists() {
        println!("Signing ARM64 macOS app...");
        sign_and_notarize_app(
            &arm_bundle_path,
            identity,
            &team_id,
            &apple_id,
            &app_password,
        )?;

        // Re-create zip with signed app
        let arm_zip_path = target_dir.join("idle-hue-macos-arm.zip");
        if arm_zip_path.exists() {
            fs::remove_file(&arm_zip_path)?;
        }

        let zip_status = Command::new("zip")
            .args(["-r", "idle-hue-macos-arm.zip", "idle-hue.app"])
            .current_dir(arm_bundle_path.parent().unwrap())
            .status()?;

        if !zip_status.success() {
            return Err("Failed to create signed ARM64 zip".into());
        }

        let zip_src = arm_bundle_path
            .parent()
            .unwrap()
            .join("idle-hue-macos-arm.zip");
        if zip_src.exists() {
            fs::rename(&zip_src, &arm_zip_path)?;
            // Update zip_paths with signed version
            if let Some(entry) = zip_paths
                .iter_mut()
                .find(|(platform, _)| platform == "macos-arm")
            {
                entry.1 = arm_zip_path;
            }
        }
    }

    // Process Intel macOS bundle
    let intel_bundle_path = target_dir.join("x86_64-apple-darwin/release/bundle/osx/idle-hue.app");
    if intel_bundle_path.exists() {
        println!("Signing Intel macOS app...");
        sign_and_notarize_app(
            &intel_bundle_path,
            identity,
            &team_id,
            &apple_id,
            &app_password,
        )?;

        // Re-create zip with signed app
        let intel_zip_path = target_dir.join("idle-hue-macos-intel.zip");
        if intel_zip_path.exists() {
            fs::remove_file(&intel_zip_path)?;
        }

        let zip_status = Command::new("zip")
            .args(["-r", "idle-hue-macos-intel.zip", "idle-hue.app"])
            .current_dir(intel_bundle_path.parent().unwrap())
            .status()?;

        if !zip_status.success() {
            return Err("Failed to create signed Intel zip".into());
        }

        let zip_src = intel_bundle_path
            .parent()
            .unwrap()
            .join("idle-hue-macos-intel.zip");
        if zip_src.exists() {
            fs::rename(&zip_src, &intel_zip_path)?;
            // Update zip_paths with signed version
            if let Some(entry) = zip_paths
                .iter_mut()
                .find(|(platform, _)| platform == "macos-intel")
            {
                entry.1 = intel_zip_path;
            }
        }
    }

    println!("macOS app signing and notarization completed!");
    Ok(())
}

fn sign_and_notarize_app(
    app_path: &std::path::Path,
    identity: &str,
    team_id: &str,
    apple_id: &str,
    app_password: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Sign the app
    println!("Code signing: {}", app_path.display());
    let sign_status = Command::new("codesign")
        .args([
            "--timestamp",
            "--options",
            "runtime",
            "--sign",
            identity,
            &app_path.to_string_lossy(),
        ])
        .status()?;

    if !sign_status.success() {
        return Err(format!("Failed to code sign {}", app_path.display()).into());
    }

    // Create temporary zip for notarization
    let temp_zip = app_path.with_extension("temp.zip");
    let zip_status = Command::new("zip")
        .args([
            "-r",
            &temp_zip.to_string_lossy(),
            &app_path.file_name().unwrap().to_string_lossy(),
        ])
        .current_dir(app_path.parent().unwrap())
        .status()?;

    if !zip_status.success() {
        return Err("Failed to create temporary zip for notarization".into());
    }

    // Submit for notarization
    println!("Submitting for notarization...");
    let notary_status = Command::new("xcrun")
        .args([
            "notarytool",
            "submit",
            "--wait",
            "--no-progress",
            "-f",
            "json",
            "--team-id",
            team_id,
            "--apple-id",
            apple_id,
            "--password",
            app_password,
            &temp_zip.to_string_lossy(),
        ])
        .status()?;

    // Clean up temp zip
    if temp_zip.exists() {
        fs::remove_file(&temp_zip)?;
    }

    if !notary_status.success() {
        return Err("Notarization failed".into());
    }

    // Staple the notarization
    println!("Stapling notarization...");
    let staple_status = Command::new("xcrun")
        .args(["stapler", "staple", &app_path.to_string_lossy()])
        .status()?;

    if !staple_status.success() {
        println!("Warning: Failed to staple notarization (this is okay for some apps)");
    }

    println!("Successfully signed and notarized: {}", app_path.display());
    Ok(())
}
