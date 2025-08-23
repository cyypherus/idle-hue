use serde::{Deserialize, Serialize};
use worker::*;

#[derive(Serialize, Deserialize, Clone)]
struct AppVersion {
    id: Option<i64>,
    app_name: String,
    version: String,
    timestamp: String,
    platforms: String,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct VersionResponse {
    app_name: String,
    version: String,
    timestamp: String,
    platforms: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct LatestVersionResponse {
    app_name: String,
    platform: String,
    version: String,
    timestamp: String,
}

#[derive(Serialize, Deserialize)]
struct UploadResponse {
    success: bool,
    message: String,
    app_name: String,
    version: String,
    platforms: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize, Deserialize)]
struct DeleteResponse {
    success: bool,
    message: String,
    app_name: String,
    version: String,
}

const SUPPORTED_PLATFORMS: &[&str] = &["macos-arm", "macos-intel", "windows-x86_64-gnu"];
const DB_NAME: &str = "version-server-d1";
const BUCKET_NAME: &str = "version-server-r2";

macro_rules! try_or_500 {
    ($expr:expr, $msg:literal) => {
        match $expr {
            Ok(val) => val,
            Err(e) => {
                return Ok(Response::from_json(&ErrorResponse {
                    error: format!("Internal server error: {}: {}", $msg, e),
                })
                .unwrap()
                .with_status(500));
            }
        }
    };
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    let router = Router::new();

    router
        .post_async("/:app/upload", upload)
        .get_async("/:app/versions", list_versions)
        .get_async("/:app/latest/:platform", get_latest_version_for_platform)
        .get_async("/:app/download/:platform/:version", download_version)
        .delete_async("/:app/delete/:version", delete_version)
        .run(req, env)
        .await
}

async fn upload(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(response) = authenticate_request(&req, &ctx.env).await {
        return Ok(response);
    }

    let app_name = match ctx.param("app") {
        Some(app) => app,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "App name parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    let form_data = match req.form_data().await {
        Ok(form) => form,
        Err(_) => {
            return Response::from_json(&ErrorResponse {
                error: "Invalid form data".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    let version = match form_data.get("version") {
        Some(FormEntry::Field(v)) => v,
        _ => {
            return Response::from_json(&ErrorResponse {
                error: "Version field is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    let bucket = try_or_500!(ctx.env.bucket(BUCKET_NAME), "Failed to get bucket");
    let db = try_or_500!(ctx.env.d1(DB_NAME), "Failed to get database");
    let mut uploaded_platforms = Vec::new();

    for platform in SUPPORTED_PLATFORMS {
        let field_name = format!("{app_name}-{platform}.zip");
        if let Some(FormEntry::File(file)) = form_data.get(&field_name) {
            let file_bytes = try_or_500!(file.bytes().await, "Failed to read file");
            let key = format!("{app_name}/{version}/{app_name}-{platform}.zip");
            try_or_500!(
                bucket.put(&key, file_bytes).execute().await,
                "Failed to upload file"
            );
            uploaded_platforms.push(platform.to_string());
        }
    }

    if uploaded_platforms.is_empty() {
        return Response::from_json(&ErrorResponse {
            error: "No valid platform files uploaded".to_string(),
        })
        .map(|r| r.with_status(400));
    }

    let timestamp = chrono::Utc::now().to_rfc3339();
    let platforms_json = try_or_500!(
        serde_json::to_string(&uploaded_platforms),
        "Failed to serialize platforms"
    );

    let stmt = try_or_500!(db
        .prepare("INSERT OR REPLACE INTO app_versions (app_name, version, timestamp, platforms) VALUES (?1, ?2, ?3, ?4)")
        .bind(&[
            app_name.into(),
            version.clone().into(),
            timestamp.into(),
            platforms_json.into(),
        ]), "Failed to prepare database statement");

    try_or_500!(stmt.run().await, "Failed to execute database query");

    Response::from_json(&UploadResponse {
        success: true,
        message: "Version uploaded successfully".to_string(),
        app_name: app_name.to_string(),
        version,
        platforms: uploaded_platforms,
    })
}

async fn list_versions(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let app_name = match ctx.param("app") {
        Some(app) => app,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "App name parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    let db = try_or_500!(ctx.env.d1(DB_NAME), "Failed to get database");

    let stmt = try_or_500!(db
        .prepare("SELECT app_name, version, timestamp, platforms, created_at FROM app_versions WHERE app_name = ?1 ORDER BY created_at DESC, id DESC")
        .bind(&[app_name.into()]), "Failed to prepare database statement");

    let result = try_or_500!(stmt.all().await, "Failed to execute database query");
    let app_versions = try_or_500!(
        result.results::<AppVersion>(),
        "Failed to parse database results"
    );

    let versions: Vec<VersionResponse> = app_versions
        .into_iter()
        .map(|app_version| {
            let platforms: Vec<String> =
                serde_json::from_str(&app_version.platforms).unwrap_or_else(|_| vec![]);

            VersionResponse {
                app_name: app_version.app_name,
                version: app_version.version,
                timestamp: app_version.timestamp,
                platforms,
            }
        })
        .collect();

    Response::from_json(&serde_json::json!({
        "app_name": app_name,
        "versions": versions
    }))
}

async fn get_latest_version_for_platform(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let app_name = match ctx.param("app") {
        Some(app) => app,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "App name parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    let platform = match ctx.param("platform") {
        Some(p) => p,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Platform parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    if !SUPPORTED_PLATFORMS.contains(&platform.as_str()) {
        return Response::from_json(&ErrorResponse {
            error: format!("Unsupported platform: {platform}"),
        })
        .map(|r| r.with_status(400));
    }

    let db = try_or_500!(ctx.env.d1(DB_NAME), "Failed to get database");

    let stmt = try_or_500!(db
        .prepare("SELECT app_name, version, timestamp, platforms FROM app_versions WHERE app_name = ?1 ORDER BY created_at DESC, id DESC")
        .bind(&[app_name.into()]), "Failed to prepare database statement");

    let result = try_or_500!(stmt.all().await, "Failed to execute database query");
    let versions = try_or_500!(
        result.results::<AppVersion>(),
        "Failed to parse database results"
    );

    for app_version in versions {
        let platforms: Vec<String> =
            serde_json::from_str(&app_version.platforms).unwrap_or_else(|_| vec![]);

        if platforms.contains(&platform.to_string()) {
            return Response::from_json(&LatestVersionResponse {
                app_name: app_version.app_name,
                platform: platform.to_string(),
                version: app_version.version,
                timestamp: app_version.timestamp,
            });
        }
    }

    Response::from_json(&ErrorResponse {
        error: "No versions found for platform".to_string(),
    })
    .map(|r| r.with_status(404))
}

async fn download_version(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let app_name = match ctx.param("app") {
        Some(app) => app,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "App name parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    let platform = match ctx.param("platform") {
        Some(p) => p,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Platform parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    let version = match ctx.param("version") {
        Some(v) => v,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Version parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    if !SUPPORTED_PLATFORMS.contains(&platform.as_str()) {
        return Response::from_json(&ErrorResponse {
            error: format!("Unsupported platform: {platform}"),
        })
        .map(|r| r.with_status(400));
    }

    let db = try_or_500!(ctx.env.d1(DB_NAME), "Failed to get database");

    let stmt = try_or_500!(db
        .prepare("SELECT app_name, version, timestamp, platforms FROM app_versions WHERE app_name = ?1 AND version = ?2")
        .bind(&[app_name.into(), version.into()]), "Failed to prepare database statement");

    let result = try_or_500!(
        stmt.first::<AppVersion>(None).await,
        "Failed to execute database query"
    );

    let app_version = match result {
        Some(v) => v,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Version not found".to_string(),
            })
            .map(|r| r.with_status(404));
        }
    };

    let platforms: Vec<String> =
        serde_json::from_str(&app_version.platforms).unwrap_or_else(|_| vec![]);

    if !platforms.contains(&platform.to_string()) {
        return Response::from_json(&ErrorResponse {
            error: "Platform not available for this version".to_string(),
        })
        .map(|r| r.with_status(404));
    }

    let bucket = try_or_500!(ctx.env.bucket(BUCKET_NAME), "Failed to get bucket");
    let file_key = format!("{app_name}/{version}/{app_name}-{platform}.zip");

    let file_obj = match try_or_500!(
        bucket.get(&file_key).execute().await,
        "Failed to get file from bucket"
    ) {
        Some(obj) => obj,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "File not found".to_string(),
            })
            .map(|r| r.with_status(404));
        }
    };

    let file_bytes = try_or_500!(
        file_obj.body().unwrap().bytes().await,
        "Failed to read file bytes"
    );
    let filename = format!("{app_name}-{platform}-{version}.zip");

    let headers = Headers::new();
    headers.set("Content-Type", "application/zip")?;
    headers.set(
        "Content-Disposition",
        &format!("attachment; filename=\"{filename}\""),
    )?;
    headers.set("Cache-Control", "public, max-age=3600")?;
    headers.set("Content-Length", &file_bytes.len().to_string())?;

    Ok(Response::from_bytes(file_bytes)?.with_headers(headers))
}

async fn delete_version(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(response) = authenticate_request(&req, &ctx.env).await {
        return Ok(response);
    }

    let app_name = match ctx.param("app") {
        Some(app) => app,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "App name parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    let version = match ctx.param("version") {
        Some(v) => v,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Version parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    let db = try_or_500!(ctx.env.d1(DB_NAME), "Failed to get database");
    let bucket = try_or_500!(ctx.env.bucket(BUCKET_NAME), "Failed to get bucket");

    let stmt = try_or_500!(db
        .prepare("SELECT app_name, version, timestamp, platforms FROM app_versions WHERE app_name = ?1 AND version = ?2")
        .bind(&[app_name.into(), version.into()]), "Failed to prepare database statement");

    let result = try_or_500!(
        stmt.first::<AppVersion>(None).await,
        "Failed to execute database query"
    );

    let app_version = match result {
        Some(v) => v,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Version not found".to_string(),
            })
            .map(|r| r.with_status(404));
        }
    };

    let platforms: Vec<String> =
        serde_json::from_str(&app_version.platforms).unwrap_or_else(|_| vec![]);

    for platform in &platforms {
        let file_key = format!("{app_name}/{version}/{app_name}-{platform}.zip");
        try_or_500!(bucket.delete(&file_key).await, "Failed to delete file");
    }

    let stmt = try_or_500!(
        db.prepare("DELETE FROM app_versions WHERE app_name = ?1 AND version = ?2")
            .bind(&[app_name.into(), version.into()]),
        "Failed to prepare delete statement"
    );

    try_or_500!(stmt.run().await, "Failed to execute delete query");

    Response::from_json(&DeleteResponse {
        success: true,
        message: "Version deleted successfully".to_string(),
        app_name: app_name.to_string(),
        version: version.to_string(),
    })
}

async fn authenticate_request(req: &Request, env: &Env) -> std::result::Result<(), Response> {
    let api_key = match req.headers().get("Authorization").map_err(|_| {
        Response::from_json(&ErrorResponse {
            error: "Failed to read headers".to_string(),
        })
        .unwrap()
        .with_status(500)
    })? {
        Some(auth_header) => {
            if let Some(key) = auth_header.strip_prefix("Bearer ") {
                key.to_string()
            } else {
                return Err(Response::from_json(&ErrorResponse {
                    error: "Invalid authorization header format".to_string(),
                })
                .unwrap()
                .with_status(401));
            }
        }
        None => {
            return Err(Response::from_json(&ErrorResponse {
                error: "Authorization header required".to_string(),
            })
            .unwrap()
            .with_status(401));
        }
    };

    let expected_key = match env.secret("API_KEY") {
        Ok(secret) => secret.to_string(),
        Err(e) => {
            return Err(Response::from_json(&ErrorResponse {
                error: format!("Internal server error: Failed to get API key: {e}"),
            })
            .unwrap()
            .with_status(500));
        }
    };

    if api_key != expected_key {
        return Err(Response::from_json(&ErrorResponse {
            error: "Invalid API key".to_string(),
        })
        .unwrap()
        .with_status(401));
    }

    Ok(())
}
