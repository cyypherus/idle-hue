use anyhow::Result;
use reqwest::{Client, Response};
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

    pub async fn get_latest_version<S1: AsRef<str>, S2: AsRef<str>>(
        &self,
        app_name: S1,
        platform: S2,
    ) -> Result<Option<VersionResponse>, VersionServerError> {
        let platform = platform.as_ref();

        if !SUPPORTED_PLATFORMS.contains(&platform) {
            return Err(VersionServerError::UnsupportedPlatform(
                platform.to_string(),
            ));
        }

        let versions = self.list_versions(app_name).await?;
        Ok(versions
            .into_iter()
            .find(|version| version.platforms.contains(&platform.to_string())))
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
        const CHUNK_SIZE: usize = 50 * 1024 * 1024; // 50MB chunks

        let app_name = app_name.as_ref();
        let version = version.as_ref();

        // Always use multipart upload

        for (platform, file_content) in files {
            if !SUPPORTED_PLATFORMS.contains(&platform.as_str()) {
                return Err(VersionServerError::UnsupportedPlatform(platform.clone()));
            }

            // Calculate SHA256 hash
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(file_content);
            let hash = format!("{:x}", hasher.finalize());

            // Create multipart upload
            let create_response = self
                .add_auth_header(
                    self.client
                        .post(format!("{}/{}/upload", self.base_url, app_name))
                        .query(&[
                            ("action", "mpu-create"),
                            ("version", version),
                            ("platform", platform),
                        ]),
                )
                .send()
                .await?;

            let create_result: MultipartCreateResponse =
                self.handle_response(create_response).await?;
            let upload_id = &create_result.upload_id;

            // Upload parts
            let chunks: Vec<&[u8]> = file_content.chunks(CHUNK_SIZE).collect();
            let mut parts = Vec::new();

            for (part_number, chunk) in chunks.iter().enumerate() {
                let part_num = (part_number + 1) as u16;

                let upload_response = self
                    .add_auth_header(
                        self.client
                            .put(format!("{}/{}/upload", self.base_url, app_name))
                            .query(&[
                                ("action", "mpu-uploadpart"),
                                ("uploadId", upload_id),
                                ("partNumber", &part_num.to_string()),
                                ("version", version),
                                ("platform", platform),
                            ])
                            .body(chunk.to_vec()),
                    )
                    .send()
                    .await?;

                let part_result: MultipartPartResponse =
                    self.handle_response(upload_response).await?;
                parts.push(serde_json::json!({
                    "partNumber": part_result.part_number,
                    "etag": part_result.etag
                }));
            }

            // Complete multipart upload
            let complete_response = self
                .add_auth_header(
                    self.client
                        .post(format!("{}/{}/upload", self.base_url, app_name))
                        .query(&[
                            ("action", "mpu-complete"),
                            ("uploadId", upload_id),
                            ("version", version),
                            ("platform", platform),
                        ])
                        .json(&serde_json::json!({"parts": parts})),
                )
                .send()
                .await?;

            let _complete_result: MultipartCompleteResponse =
                self.handle_response(complete_response).await?;

            // Register the completed upload
            let register_response = self
                .add_auth_header(
                    self.client
                        .post(format!("{}/{}/upload/finish", self.base_url, app_name))
                        .json(&CompleteVersionRequest {
                            version: version.to_string(),
                            platform: platform.clone(),
                            sha256: hash,
                        }),
                )
                .send()
                .await?;

            let register_result: CompleteVersionResponse =
                self.handle_response(register_response).await?;

            if !register_result.success {
                return Err(VersionServerError::Api {
                    status: 500,
                    message: format!("Failed to register version: {}", register_result.message),
                });
            }
        }

        Ok(UploadResponse {
            success: true,
            message: "Version uploaded successfully".to_string(),
            app_name: app_name.to_string(),
            version: version.to_string(),
            platforms: files.keys().cloned().collect(),
        })
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
            .get_latest_version(&unique_app, "macos-arm")
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
            .get_latest_version(&ctx.test_app, "macos-arm")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.version, ctx.test_version);
        assert!(latest.platforms.contains(&"macos-arm".to_string()));

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
