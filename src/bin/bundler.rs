use std::fs;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;

    println!("Project root: {}", project_root.display());

    println!("Getting git commit hash...");
    let git_hash = match Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
    {
        Ok(output) if output.status.success() => String::from_utf8(output.stdout)
            .unwrap_or_else(|_| "unknown".to_string())
            .trim()
            .to_string(),
        _ => "unknown".to_string(),
    };

    let version_file_path = project_root.join("src").join("version.txt");
    fs::write(&version_file_path, format!("\"{}\"", git_hash))?;
    println!(
        "Written git hash '{}' to {}",
        git_hash,
        version_file_path.display()
    );

    println!("Building for Apple Silicon (ARM64)...");
    let arm_status = Command::new("cargo")
        .args(["bundle", "--release", "--bin", "idle-hue"])
        .current_dir(&project_root)
        .status()?;

    if !arm_status.success() {
        eprintln!("Failed to build ARM64 bundle");
        std::process::exit(1);
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
        ])
        .current_dir(&project_root)
        .status()?;

    if !intel_status.success() {
        eprintln!("Failed to build Intel bundle");
        std::process::exit(1);
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
        ])
        .current_dir(&project_root)
        .status()?;

    if !windows_status.success() {
        eprintln!("Failed to build Windows executable");
        std::process::exit(1);
    }

    let arm_bundle_path = project_root.join("target/release/bundle/osx/idle-hue.app");
    let intel_bundle_path =
        project_root.join("target/x86_64-apple-darwin/release/bundle/osx/idle-hue.app");
    let windows_exe_path = project_root.join("target/x86_64-pc-windows-gnu/release/idle-hue.exe");

    if !arm_bundle_path.exists() {
        eprintln!("ARM bundle not found at {:?}", arm_bundle_path);
        std::process::exit(1);
    }

    if !intel_bundle_path.exists() {
        eprintln!("Intel bundle not found at {:?}", intel_bundle_path);
        std::process::exit(1);
    }

    if !windows_exe_path.exists() {
        eprintln!("Windows executable not found at {:?}", windows_exe_path);
        std::process::exit(1);
    }

    println!("Creating ARM64 zip...");
    let arm_zip_status = Command::new("zip")
        .args(["-r", "idle-hue-macos-arm.zip", "idle-hue.app"])
        .current_dir(arm_bundle_path.parent().unwrap())
        .status()?;

    if !arm_zip_status.success() {
        eprintln!("Failed to create ARM64 zip");
        std::process::exit(1);
    }

    println!("Creating Intel zip...");
    let intel_zip_status = Command::new("zip")
        .args(["-r", "idle-hue-macos-intel.zip", "idle-hue.app"])
        .current_dir(intel_bundle_path.parent().unwrap())
        .status()?;

    if !intel_zip_status.success() {
        eprintln!("Failed to create Intel zip");
        std::process::exit(1);
    }

    println!("Creating Windows zip...");
    let windows_zip_status = Command::new("zip")
        .args(&["-j", "idle-hue-windows-x86_64-gnu.zip", "idle-hue.exe"])
        .current_dir(windows_exe_path.parent().unwrap())
        .status()?;

    if !windows_zip_status.success() {
        eprintln!("Failed to create Windows zip");
        std::process::exit(1);
    }

    let arm_zip_src = arm_bundle_path
        .parent()
        .unwrap()
        .join("idle-hue-macos-arm.zip");
    let intel_zip_src = intel_bundle_path
        .parent()
        .unwrap()
        .join("idle-hue-macos-intel.zip");
    let windows_zip_src = windows_exe_path
        .parent()
        .unwrap()
        .join("idle-hue-windows-x86_64-gnu.zip");
    let arm_zip_dest = project_root.join("idle-hue-macos-arm.zip");
    let intel_zip_dest = project_root.join("idle-hue-macos-intel.zip");
    let windows_zip_dest = project_root.join("idle-hue-windows-x86_64-gnu.zip");

    if arm_zip_src.exists() {
        fs::rename(&arm_zip_src, &arm_zip_dest)?;
        println!("Created: {}", arm_zip_dest.display());
    }

    if intel_zip_src.exists() {
        fs::rename(&intel_zip_src, &intel_zip_dest)?;
        println!("Created: {}", intel_zip_dest.display());
    }

    if windows_zip_src.exists() {
        fs::rename(&windows_zip_src, &windows_zip_dest)?;
        println!("Created: {}", windows_zip_dest.display());
    }

    println!("Bundle process completed successfully!");
    Ok(())
}
