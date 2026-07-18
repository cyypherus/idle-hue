use app_update::{AppUpdater, UpdateConfig, UpdateOutcome, UpdateStatus as AppUpdateStatus};
#[cfg(not(feature = "prod"))]
use app_update_client::VERSION_SERVER_DEV;
#[cfg(feature = "prod")]
use app_update_client::VERSION_SERVER_PROD;
use app_update_client::{VersionServerAppClient, VersionServerClient};
use semver::Version;
use std::env;
use std::future::Future;

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
    updater: AppUpdater<VersionServerAppClient>,
}

impl AutoUpdater {
    pub fn new() -> Self {
        let current_version =
            Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 1, 0));

        #[cfg(feature = "prod")]
        let base_url = VERSION_SERVER_PROD;

        #[cfg(not(feature = "prod"))]
        let base_url = VERSION_SERVER_DEV;

        let config = UpdateConfig::new(current_version)
            .expect("idle-hue updates are only supported on macOS and Windows");
        let api = VersionServerClient::new(base_url).for_app(APP_NAME);

        Self {
            updater: AppUpdater::new(config, api),
        }
    }

    pub async fn check_and_install_updates_with_callback<F, Fut>(&self, status_callback: Option<F>)
    where
        F: Fn(UpdateStatus) -> Fut + Send + Sync + Clone,
        Fut: Future<Output = ()> + Send,
    {
        let result = self
            .updater
            .update_with_status({
                let status_callback = status_callback.clone();
                move |status| {
                    let status_callback = status_callback.clone();
                    async move {
                        if let Some(callback) = status_callback {
                            let status = match status {
                                AppUpdateStatus::Checking => UpdateStatus::Checking,
                                AppUpdateStatus::UpToDate { version } => {
                                    UpdateStatus::UpToDate { version }
                                }
                                AppUpdateStatus::Downloading { version } => {
                                    UpdateStatus::Downloading { version }
                                }
                                AppUpdateStatus::Installing { version } => {
                                    UpdateStatus::Installing { version }
                                }
                                AppUpdateStatus::Updated { version } => {
                                    UpdateStatus::Updated { version }
                                }
                            };
                            callback(status).await;
                        }
                    }
                }
            })
            .await;

        match result {
            Ok(UpdateOutcome::UpToDate { .. }) => {
                if let Some(callback) = status_callback {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    callback(UpdateStatus::Idle).await;
                }
            }
            Ok(UpdateOutcome::Updated { .. }) => {}
            Err(error) => {
                if let Some(callback) = status_callback {
                    callback(UpdateStatus::Error(error.to_string())).await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    callback(UpdateStatus::Idle).await;
                }
            }
        }
    }
}
