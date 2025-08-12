use anyhow::{Context, Result};
use reqwest::Client;
use semver::Version;
use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use tokio::fs;
use tokio::io::AsyncWriteExt;

const GITHUB_API_URL: &str = "https://api.github.com";
const REPO_OWNER: &str = "cyypherus";
const REPO_NAME: &str = "idle-hue";

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

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: String,
    assets: Vec<GitHubAsset>,
    prerelease: bool,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Clone)]
pub struct AutoUpdater {
    current_version: Version,
    client: Client,
}

impl AutoUpdater {
    pub fn new() -> Self {
        let current_version =
            Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 1, 0));

        Self {
            current_version,
            client: Client::new(),
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
            callback(UpdateStatus::Checking).await;
        }
        let release = self
            .get_release_by_version(&version)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Release not found for version {}", version))?;

        let asset = self.find_asset_for_platform(&release.assets)?;

        let temp_dir = TempDir::new()?;
        let download_path = temp_dir.path().join(&asset.name);

        if let Some(callback) = status_callback {
            callback(UpdateStatus::Downloading {
                version: version.clone(),
            })
            .await;
        }

        self.download_file(&asset.browser_download_url, &download_path)
            .await?;

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

    async fn get_latest_release(&self) -> Result<GitHubRelease> {
        let url = format!(
            "{}/repos/{}/{}/releases/latest",
            GITHUB_API_URL, REPO_OWNER, REPO_NAME
        );

        let response = self
            .client
            .get(&url)
            .header(
                "User-Agent",
                format!("{}/{}", REPO_NAME, self.current_version),
            )
            .send()
            .await?;

        let release: GitHubRelease = response.json().await?;

        Ok(release)
    }

    async fn get_release_by_version(&self, version: &Version) -> Result<Option<GitHubRelease>> {
        let tag = format!("{}", version);
        let url = format!(
            "{}/repos/{}/{}/releases/tags/{}",
            GITHUB_API_URL, REPO_OWNER, REPO_NAME, tag
        );

        let response = self
            .client
            .get(&url)
            .header(
                "User-Agent",
                format!("{}/{}", REPO_NAME, self.current_version),
            )
            .send()
            .await?;

        if response.status() == 404 {
            return Ok(None);
        }

        let release: GitHubRelease = response.json().await?;
        Ok(Some(release))
    }

    fn parse_version(&self, tag: &str) -> Result<Version> {
        let version_str = tag.strip_prefix('v').unwrap_or(tag);
        Version::parse(version_str).context("Failed to parse version")
    }

    fn find_asset_for_platform<'a>(&self, assets: &'a [GitHubAsset]) -> Result<&'a GitHubAsset> {
        let platform_suffix = self.get_platform_suffix();

        assets
            .iter()
            .find(|asset| asset.name.contains(&platform_suffix))
            .ok_or_else(|| anyhow::anyhow!("No asset found for platform: {}", platform_suffix))
    }

    fn get_platform_suffix(&self) -> String {
        match env::consts::OS {
            "windows" => "windows-x86_64-gnu.zip".to_string(),
            "macos" => {
                if cfg!(target_arch = "aarch64") {
                    "macos-arm.zip".to_string()
                } else {
                    "macos-intel.zip".to_string()
                }
            }
            os => format!("{}.zip", os),
        }
    }

    async fn download_file(&self, url: &str, path: &Path) -> Result<()> {
        let response = self.client.get(url).send().await?;
        let bytes = response.bytes().await?;

        let mut file = fs::File::create(path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;

        Ok(())
    }

    async fn install_update(&self, zip_path: &Path) -> Result<()> {
        #[cfg(target_os = "windows")]
        return self.install_windows(zip_path).await;

        #[cfg(target_os = "macos")]
        return self.install_macos(zip_path).await;

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
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
            .args(&["-av", "--delete"])
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
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        std::fs::create_dir_all(p)?;
                    }
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

    pub fn restart_application() -> Result<()> {
        let current_exe = env::current_exe()?;

        #[cfg(target_os = "windows")]
        {
            std::process::Command::new(&current_exe).spawn()?;
        }

        #[cfg(target_os = "macos")]
        {
            let app_bundle = Self::find_app_bundle(&current_exe)?;
            std::process::Command::new("open")
                .arg(&app_bundle)
                .spawn()?;
        }

        #[cfg(target_os = "linux")]
        {
            std::process::Command::new(&current_exe).spawn()?;
        }

        std::process::exit(0);
    }

    pub async fn check_and_install_updates_with_callback<F, Fut>(&self, status_callback: Option<F>)
    where
        F: Fn(UpdateStatus) -> Fut + Send + Sync + Clone,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let release = match self.get_latest_release().await {
            Err(e) => {
                if let Some(ref callback) = status_callback {
                    callback(UpdateStatus::Error(e.to_string())).await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    callback(UpdateStatus::Idle).await;
                }
                return;
            }
            Ok(release) => release,
        };
        let latest = match self.parse_version(&release.tag_name) {
            Err(e) => {
                if let Some(ref callback) = status_callback {
                    callback(UpdateStatus::Error(e.to_string())).await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    callback(UpdateStatus::Idle).await;
                }
                return;
            }
            Ok(latest) => latest,
        };

        let true = latest > self.current_version else {
            if let Some(ref callback) = status_callback {
                callback(UpdateStatus::UpToDate { version: latest }).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                callback(UpdateStatus::Idle).await;
            }
            return;
        };

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
