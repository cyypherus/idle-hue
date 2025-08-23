use anyhow::Result;
use reqwest::{Client, Response, multipart};
use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;

pub use version_api_models::*;

#[derive(Error, Debug)]
pub enum VersionServerError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("Authentication failed")]
    Authentication,

    #[error("Version not found")]
    VersionNotFound,

    #[error("Platform not supported: {0}")]
    UnsupportedPlatform(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct VersionServerClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
}

impl VersionServerClient {
    pub fn new<S: Into<String>>(base_url: S) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: None,
        }
    }

    pub fn with_api_key<S: Into<String>>(mut self, api_key: S) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn handle_response<T>(&self, response: Response) -> Result<T, VersionServerError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let status = response.status();

        if status.is_success() {
            Ok(response.json().await?)
        } else {
            let error_text = response.text().await.unwrap_or_default();

            match status.as_u16() {
                401 => Err(VersionServerError::Authentication),
                404 => Err(VersionServerError::VersionNotFound),
                _ => {
                    let message = if let Ok(error_response) =
                        serde_json::from_str::<ErrorResponse>(&error_text)
                    {
                        error_response.error
                    } else {
                        error_text
                    };
                    Err(VersionServerError::Api {
                        status: status.as_u16(),
                        message,
                    })
                }
            }
        }
    }

    fn add_auth_header(&self, request_builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(api_key) = &self.api_key {
            request_builder.header("Authorization", format!("Bearer {api_key}"))
        } else {
            request_builder
        }
    }

    pub async fn list_versions<S: AsRef<str>>(
        &self,
        app_name: S,
    ) -> Result<Vec<VersionResponse>, VersionServerError> {
        let app_name = app_name.as_ref();
        let response = self
            .client
            .get(format!("{}/{}/versions", self.base_url, app_name))
            .send()
            .await?;

        match response.status().as_u16() {
            200 => {
                let json: serde_json::Value = response.json().await?;
                if let Some(versions) = json.get("versions") {
                    Ok(serde_json::from_value(versions.clone())?)
                } else {
                    Ok(Vec::new())
                }
            }
            404 => Ok(Vec::new()),
            _ => Err(self
                .handle_response::<Vec<VersionResponse>>(response)
                .await
                .unwrap_err()),
        }
    }

    pub async fn get_latest_version_for_platform<S1: AsRef<str>, S2: AsRef<str>>(
        &self,
        app_name: S1,
        platform: S2,
    ) -> Result<Option<LatestVersionResponse>, VersionServerError> {
        let app_name = app_name.as_ref();
        let platform = platform.as_ref();

        if !SUPPORTED_PLATFORMS.contains(&platform) {
            return Err(VersionServerError::UnsupportedPlatform(
                platform.to_string(),
            ));
        }

        let response = self
            .client
            .get(format!(
                "{}/{}/latest/{}",
                self.base_url, app_name, platform
            ))
            .send()
            .await?;

        match response.status().as_u16() {
            200 => Ok(Some(self.handle_response(response).await?)),
            404 => Ok(None),
            _ => Err(self
                .handle_response::<LatestVersionResponse>(response)
                .await
                .unwrap_err()),
        }
    }

    pub async fn download_version<S1: AsRef<str>, S2: AsRef<str>, S3: AsRef<str>>(
        &self,
        app_name: S1,
        platform: S2,
        version: S3,
    ) -> Result<Vec<u8>, VersionServerError> {
        let app_name = app_name.as_ref();
        let platform = platform.as_ref();
        let version = version.as_ref();

        if !SUPPORTED_PLATFORMS.contains(&platform) {
            return Err(VersionServerError::UnsupportedPlatform(
                platform.to_string(),
            ));
        }

        let response = self
            .client
            .get(format!(
                "{}/{}/download/{}/{}",
                self.base_url, app_name, platform, version
            ))
            .send()
            .await?;

        match response.status().as_u16() {
            200 => Ok(response.bytes().await?.to_vec()),
            404 => Err(VersionServerError::VersionNotFound),
            400 => Err(VersionServerError::UnsupportedPlatform(
                platform.to_string(),
            )),
            _ => Err(self.handle_response::<()>(response).await.unwrap_err()),
        }
    }

    pub async fn upload_version<S1: AsRef<str>, S2: AsRef<str>>(
        &self,
        app_name: S1,
        version: S2,
        files: &HashMap<String, Vec<u8>>,
    ) -> Result<UploadResponse, VersionServerError> {
        let app_name = app_name.as_ref();
        let version = version.as_ref().to_string();
        let mut form = multipart::Form::new().text("version", version);

        for (platform, file_content) in files {
            if !SUPPORTED_PLATFORMS.contains(&platform.as_str()) {
                return Err(VersionServerError::UnsupportedPlatform(platform.clone()));
            }

            let file_name = format!("{app_name}-{platform}.zip");

            form = form.part(
                file_name.clone(),
                multipart::Part::bytes(file_content.clone())
                    .file_name(file_name)
                    .mime_str("application/zip")?,
            );
        }

        let request = self
            .client
            .post(format!("{}/{}/upload", self.base_url, app_name))
            .multipart(form);

        let response = self.add_auth_header(request).send().await?;
        self.handle_response(response).await
    }

    pub async fn delete_version<S1: AsRef<str>, S2: AsRef<str>>(
        &self,
        app_name: S1,
        version: S2,
    ) -> Result<DeleteResponse, VersionServerError> {
        let app_name = app_name.as_ref();
        let version = version.as_ref();
        let request = self
            .client
            .delete(format!("{}/{}/delete/{}", self.base_url, app_name, version));

        let response = self.add_auth_header(request).send().await?;

        match response.status().as_u16() {
            200 => Ok(self.handle_response(response).await?),
            404 => Err(VersionServerError::VersionNotFound),
            _ => Err(self
                .handle_response::<DeleteResponse>(response)
                .await
                .unwrap_err()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug, Clone)]
    struct TestContext {
        pub client: VersionServerClient,
        pub test_app: String,
        pub test_version: String,
    }

    impl Default for TestContext {
        fn default() -> Self {
            Self::new()
        }
    }

    impl TestContext {
        fn new() -> Self {
            let base_url = std::env::var("TEST_URL").unwrap();
            let api_key = std::env::var("TEST_API_KEY").unwrap();

            let mut client = VersionServerClient::new(base_url);
            client = client.with_api_key(api_key);

            Self {
                client,
                test_app: "test-app".to_string(),
                test_version: "1.0.0".to_string(),
            }
        }

        async fn create_test_files(&self) -> HashMap<String, Vec<u8>> {
            let mut files = HashMap::new();
            for platform in SUPPORTED_PLATFORMS {
                files.insert(
                    platform.to_string(),
                    format!("test bundle for {platform}").into_bytes(),
                );
            }
            files
        }

        async fn cleanup_version(&self) -> Result<(), VersionServerError> {
            let _ = self
                .client
                .delete_version(&self.test_app, &self.test_version)
                .await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_list_versions_empty() {
        let ctx = TestContext::new();
        let result = ctx.client.list_versions(&ctx.test_app).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_latest_version_not_found() {
        let ctx = TestContext::new();
        let unique_app = format!(
            "{}-notfound-{}",
            ctx.test_app,
            chrono::Utc::now().timestamp()
        );
        let result = ctx
            .client
            .get_latest_version_for_platform(&unique_app, "macos-arm")
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_upload_without_auth() {
        let ctx = TestContext::new();
        let client_no_auth = VersionServerClient::new(ctx.client.base_url());
        let files = ctx.create_test_files().await;
        let result = client_no_auth
            .upload_version(&ctx.test_app, &ctx.test_version, &files)
            .await;
        match result {
            Err(VersionServerError::Authentication) => {}
            Err(VersionServerError::Api { status: 401, .. }) => {}
            other => panic!("Expected authentication error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_upload_and_download_flow() {
        let ctx = TestContext::new();
        ctx.cleanup_version().await.ok();

        let files = ctx.create_test_files().await;
        let upload_result = ctx
            .client
            .upload_version(&ctx.test_app, &ctx.test_version, &files)
            .await;
        assert!(upload_result.is_ok());

        let versions = ctx.client.list_versions(&ctx.test_app).await.unwrap();
        assert!(!versions.is_empty());
        assert_eq!(versions[0].version, ctx.test_version);

        let latest = ctx
            .client
            .get_latest_version_for_platform(&ctx.test_app, "macos-arm")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.version, ctx.test_version);
        assert_eq!(latest.platform, "macos-arm");

        let download_result = ctx
            .client
            .download_version(&ctx.test_app, "macos-arm", &ctx.test_version)
            .await;
        assert!(download_result.is_ok());

        ctx.cleanup_version().await.ok();
    }

    #[tokio::test]
    async fn test_download_nonexistent_version() {
        let ctx = TestContext::new();
        let result = ctx
            .client
            .download_version(&ctx.test_app, "macos-arm", "999.0.0")
            .await;
        assert!(matches!(result, Err(VersionServerError::VersionNotFound)));
    }

    #[tokio::test]
    async fn test_delete_version() {
        let ctx = TestContext::new();
        let unique_app = format!("{}-delete-{}", ctx.test_app, chrono::Utc::now().timestamp());
        let unique_version = format!("{}-{}", ctx.test_version, chrono::Utc::now().timestamp());

        let files = ctx.create_test_files().await;
        ctx.client
            .upload_version(&unique_app, &unique_version, &files)
            .await
            .unwrap();

        let delete_result = ctx
            .client
            .delete_version(&unique_app, &unique_version)
            .await;
        assert!(delete_result.is_ok());

        let versions = ctx.client.list_versions(&unique_app).await.unwrap();
        assert!(versions.is_empty() || !versions.iter().any(|v| v.version == unique_version));
    }
}
