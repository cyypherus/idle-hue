use anyhow::Result;
use semver::Version;
use std::env;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use version_api_client::VersionServerClient;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const APP_NAME: &str = "idle-hue";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    Idle,
    Checking,

    UpToDate { version: Version },
    Downloading { version: Version },
    Installing { version: Version },
    Updated { version: Version },
    Error(String),
}

#[derive(Clone)]
pub struct AutoUpdater {
    current_version: Version,
    client: VersionServerClient,
}

impl AutoUpdater {
    pub fn new() -> Self {
        let current_version =
            Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 1, 0));

        #[cfg(feature = "prod")]
        let client = VersionServerClient::new(version_api_models::VERSION_SERVER_PROD);

        #[cfg(not(feature = "prod"))]
        let client = VersionServerClient::new(version_api_models::VERSION_SERVER_DEV);

        Self {
            current_version,
            client,
        }
    }

    pub async fn download_and_install_update_with_callback<F, Fut>(
        &self,
        version: Version,
        status_callback: &Option<F>,
    ) -> Result<()>
    where
        F: Fn(UpdateStatus) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = ()> + Send,
    {
        if let Some(callback) = status_callback {
            callback(UpdateStatus::Downloading {
                version: version.clone(),
            })
            .await;
        }

        let platform = self.get_platform_string();
        let version_str = version.to_string();

        let download_data = self
            .client
            .download_version(APP_NAME, &platform, &version_str)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to download version: {}", e))?;

        let temp_dir = TempDir::new()?;
        let download_path = temp_dir.path().join(format!("{APP_NAME}-{platform}.zip"));

        let mut file = fs::File::create(&download_path).await?;
        file.write_all(&download_data).await?;
        file.flush().await?;

        if let Some(callback) = status_callback {
            callback(UpdateStatus::Installing {
                version: version.clone(),
            })
            .await;
        }

        self.install_update(&download_path).await?;

        if let Some(callback) = status_callback {
            callback(UpdateStatus::Updated { version }).await;
        }

        log::info!("Update installed successfully");
        Ok(())
    }

    fn get_platform_string(&self) -> String {
        match env::consts::OS {
            "windows" => "windows-x86_64-gnu".to_string(),
            "macos" => {
                if cfg!(target_arch = "aarch64") {
                    "macos-arm".to_string()
                } else {
                    "macos-intel".to_string()
                }
            }
            _ => "linux-x86_64-gnu".to_string(),
        }
    }

    async fn install_update(&self, zip_path: &Path) -> Result<()> {
        #[cfg(target_os = "windows")]
        return self.install_windows(zip_path).await;

        #[cfg(target_os = "macos")]
        return self.install_macos(zip_path).await;

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        Err(anyhow::anyhow!("Unsupported OS: {}", env::consts::OS))
    }

    #[cfg(target_os = "windows")]
    async fn install_windows(&self, zip_path: &Path) -> Result<()> {
        let current_exe = env::current_exe()?;
        let install_dir = current_exe
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine install directory"))?;

        let temp_dir = TempDir::new()?;
        self.extract_zip(zip_path, temp_dir.path()).await?;

        let exe_name = current_exe
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine executable name"))?;

        let new_exe = temp_dir.path().join(exe_name);
        let backup_exe = install_dir.join(format!("{}.backup", exe_name.to_string_lossy()));
        let target_exe = install_dir.join(exe_name);

        fs::copy(&target_exe, &backup_exe).await?;
        fs::copy(&new_exe, &target_exe).await?;

        Ok(())
    }

    #[cfg(target_os = "macos")]
    async fn install_macos(&self, zip_path: &Path) -> Result<()> {
        let current_exe = env::current_exe()?;
        let app_bundle = Self::find_app_bundle(&current_exe)?;

        let temp_dir = TempDir::new()?;
        self.extract_zip(zip_path, temp_dir.path()).await?;

        let new_app_bundle = temp_dir.path().join("idle-hue.app");

        let output = tokio::process::Command::new("rsync")
            .args(["-av", "--delete"])
            .arg(&new_app_bundle)
            .arg(app_bundle.parent().unwrap())
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to install update: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    async fn extract_zip(&self, zip_path: &Path, extract_to: &Path) -> Result<()> {
        let zip_path = zip_path.to_path_buf();
        let extract_to = extract_to.to_path_buf();

        let file = std::fs::File::open(&zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = extract_to.join(file.name());

            if file.name().ends_with('/') {
                std::fs::create_dir_all(&outpath)?;
            } else {
                if let Some(p) = outpath.parent()
                    && !p.exists()
                {
                    std::fs::create_dir_all(p)?;
                }
                let mut outfile = std::fs::File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(mode) = file.unix_mode() {
                        let permissions = std::fs::Permissions::from_mode(mode);
                        std::fs::set_permissions(&outpath, permissions)?;
                    } else if file.name().contains("idle-hue") {
                        let permissions = std::fs::Permissions::from_mode(0o755);
                        std::fs::set_permissions(&outpath, permissions)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn find_app_bundle(exe_path: &Path) -> Result<PathBuf> {
        let mut current = exe_path;
        let mut levels = 0;

        while let Some(parent) = current.parent() {
            if levels >= 3 {
                break;
            }

            if parent.extension().and_then(|s| s.to_str()) == Some("app") {
                return Ok(parent.to_path_buf());
            }
            current = parent;
            levels += 1;
        }

        Err(anyhow::anyhow!("Could not find .app bundle"))
    }

    pub async fn restart_application() -> Result<()> {
        let current_exe = env::current_exe()?;

        #[cfg(target_os = "windows")]
        {
            std::process::Command::new(&current_exe)
                .creation_flags(0x00000008) // DETACHED_PROCESS
                .spawn()?;
        }

        #[cfg(target_os = "macos")]
        {
            let app_bundle = Self::find_app_bundle(&current_exe)?;
            std::process::Command::new("open")
                .arg("-n")
                .arg(&app_bundle)
                .spawn()?;
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        std::process::exit(0);
    }

    pub async fn check_and_install_updates_with_callback<F, Fut>(&self, status_callback: Option<F>)
    where
        F: Fn(UpdateStatus) -> Fut + Send + Sync + Clone,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let platform = self.get_platform_string();

        let latest_version = match self.client.get_latest_version(APP_NAME, &platform).await {
            Err(e) => {
                let error_msg = e.to_string();
                if let Some(ref callback) = status_callback {
                    callback(UpdateStatus::Error(error_msg)).await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    callback(UpdateStatus::Idle).await;
                }
                return;
            }
            Ok(Some(version_info)) => version_info,
            Ok(None) => {
                if let Some(ref callback) = status_callback {
                    callback(UpdateStatus::Error("No versions available".to_string())).await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    callback(UpdateStatus::Idle).await;
                }
                return;
            }
        };

        let latest = match Version::parse(&latest_version.version) {
            Err(e) => {
                if let Some(ref callback) = status_callback {
                    callback(UpdateStatus::Error(format!("Invalid version format: {e}"))).await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    callback(UpdateStatus::Idle).await;
                }
                return;
            }
            Ok(latest) => latest,
        };

        if latest <= self.current_version {
            if let Some(ref callback) = status_callback {
                callback(UpdateStatus::UpToDate { version: latest }).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                callback(UpdateStatus::Idle).await;
            }
            return;
        }

        match self
            .download_and_install_update_with_callback(latest.clone(), &status_callback)
            .await
        {
            Err(e) => {
                if let Some(callback) = status_callback {
                    callback(UpdateStatus::Error(e.to_string())).await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    callback(UpdateStatus::Idle).await;
                }
            }
            Ok(_) => {
                if let Some(callback) = status_callback {
                    callback(UpdateStatus::Updated { version: latest }).await;
                }
            }
        }
    }
}
