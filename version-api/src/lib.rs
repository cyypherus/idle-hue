use version_api_models::*;
use worker::*;
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
        .post_async("/:app/upload", handle_multipart_post)
        .put_async("/:app/upload", handle_multipart_put)
        .delete_async("/:app/upload", handle_multipart_delete)
        .post_async("/:app/upload/finish", complete_version_upload)
        .get_async("/:app/versions", list_versions)
        .get_async("/:app/download/:platform/:version", download_version)
        .delete_async("/:app/delete/:version", delete_version)
        .run(req, env)
        .await
}

async fn handle_multipart_post(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
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

    let url = req.url()?;
    let action = match url
        .query_pairs()
        .find(|(key, _)| key == "action")
        .map(|(_, value)| value.to_string())
    {
        Some(a) => a,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Action parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    match action.as_str() {
        "mpu-create" => {
            let version = match url
                .query_pairs()
                .find(|(key, _)| key == "version")
                .map(|(_, value)| value.to_string())
            {
                Some(v) => v,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "version parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let platform = match url
                .query_pairs()
                .find(|(key, _)| key == "platform")
                .map(|(_, value)| value.to_string())
            {
                Some(p) => p,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "platform parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let key = format!("{app_name}/{version}/{app_name}-{platform}.zip");

            let bucket = try_or_500!(ctx.env.bucket(BUCKET_NAME), "Failed to get bucket");
            let multipart_upload = try_or_500!(
                bucket.create_multipart_upload(&key).execute().await,
                "Failed to create multipart upload"
            );

            let upload_id = multipart_upload.upload_id().await;
            Response::from_json(&MultipartCreateResponse {
                key: key.to_string(),
                upload_id,
            })
        }
        "mpu-complete" => {
            let version = match url
                .query_pairs()
                .find(|(key, _)| key == "version")
                .map(|(_, value)| value.to_string())
            {
                Some(v) => v,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "version parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let platform = match url
                .query_pairs()
                .find(|(key, _)| key == "platform")
                .map(|(_, value)| value.to_string())
            {
                Some(p) => p,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "platform parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let upload_id = match url
                .query_pairs()
                .find(|(key, _)| key == "uploadId")
                .map(|(_, value)| value.to_string())
            {
                Some(id) => id,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "uploadId parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let key = format!("{app_name}/{version}/{app_name}-{platform}.zip");

            #[derive(serde::Deserialize)]
            struct CompleteBody {
                parts: Vec<serde_json::Value>,
            }

            let body: CompleteBody = try_or_500!(req.json().await, "Failed to parse request body");

            let bucket = try_or_500!(ctx.env.bucket(BUCKET_NAME), "Failed to get bucket");
            let multipart_upload = try_or_500!(
                bucket.resume_multipart_upload(&key, &upload_id),
                "Failed to resume multipart upload"
            );

            let parts: Vec<worker::UploadedPart> = body
                .parts
                .into_iter()
                .filter_map(|part| {
                    let part_number = part.get("partNumber")?.as_u64()? as u16;
                    let etag = part.get("etag")?.as_str()?.to_string();
                    Some(worker::UploadedPart::new(part_number, etag))
                })
                .collect();

            let object = try_or_500!(
                multipart_upload.complete(parts).await,
                "Failed to complete multipart upload"
            );

            Response::from_json(&MultipartCompleteResponse {
                success: true,
                etag: object.http_etag(),
            })
        }
        _ => Response::from_json(&ErrorResponse {
            error: format!("Unknown action {action} for POST"),
        })
        .map(|r| r.with_status(400)),
    }
}

async fn handle_multipart_put(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
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

    let url = req.url()?;
    let action = match url
        .query_pairs()
        .find(|(key, _)| key == "action")
        .map(|(_, value)| value.to_string())
    {
        Some(a) => a,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Action parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    match action.as_str() {
        "mpu-uploadpart" => {
            let version = match url
                .query_pairs()
                .find(|(key, _)| key == "version")
                .map(|(_, value)| value.to_string())
            {
                Some(v) => v,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "version parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let platform = match url
                .query_pairs()
                .find(|(key, _)| key == "platform")
                .map(|(_, value)| value.to_string())
            {
                Some(p) => p,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "platform parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let upload_id = match url
                .query_pairs()
                .find(|(key, _)| key == "uploadId")
                .map(|(_, value)| value.to_string())
            {
                Some(id) => id,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "uploadId parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let part_number = match url
                .query_pairs()
                .find(|(key, _)| key == "partNumber")
                .and_then(|(_, value)| value.parse::<u16>().ok())
            {
                Some(num) => num,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "partNumber parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let key = format!("{app_name}/{version}/{app_name}-{platform}.zip");
            let body_bytes = try_or_500!(req.bytes().await, "Failed to read request body");

            let bucket = try_or_500!(ctx.env.bucket(BUCKET_NAME), "Failed to get bucket");
            let multipart_upload = try_or_500!(
                bucket.resume_multipart_upload(&key, &upload_id),
                "Failed to resume multipart upload"
            );

            let uploaded_part = try_or_500!(
                multipart_upload.upload_part(part_number, body_bytes).await,
                "Failed to upload part"
            );

            Response::from_json(&MultipartPartResponse {
                part_number: uploaded_part.part_number(),
                etag: uploaded_part.etag(),
            })
        }
        _ => Response::from_json(&ErrorResponse {
            error: format!("Unknown action {action} for PUT"),
        })
        .map(|r| r.with_status(400)),
    }
}

async fn handle_multipart_delete(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(response) = authenticate_request(&_req, &ctx.env).await {
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

    let url = _req.url()?;
    let action = match url
        .query_pairs()
        .find(|(key, _)| key == "action")
        .map(|(_, value)| value.to_string())
    {
        Some(a) => a,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Action parameter is required".to_string(),
            })
            .map(|r| r.with_status(400));
        }
    };

    match action.as_str() {
        "mpu-abort" => {
            let version = match url
                .query_pairs()
                .find(|(key, _)| key == "version")
                .map(|(_, value)| value.to_string())
            {
                Some(v) => v,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "version parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let platform = match url
                .query_pairs()
                .find(|(key, _)| key == "platform")
                .map(|(_, value)| value.to_string())
            {
                Some(p) => p,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "platform parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let upload_id = match url
                .query_pairs()
                .find(|(key, _)| key == "uploadId")
                .map(|(_, value)| value.to_string())
            {
                Some(id) => id,
                None => {
                    return Response::from_json(&ErrorResponse {
                        error: "uploadId parameter is required".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            let key = format!("{app_name}/{version}/{app_name}-{platform}.zip");

            let bucket = try_or_500!(ctx.env.bucket(BUCKET_NAME), "Failed to get bucket");
            let multipart_upload = try_or_500!(
                bucket.resume_multipart_upload(&key, &upload_id),
                "Failed to resume multipart upload"
            );

            try_or_500!(
                multipart_upload.abort().await,
                "Failed to abort multipart upload"
            );

            Ok(Response::empty()?.with_status(204))
        }
        _ => Response::from_json(&ErrorResponse {
            error: format!("Unknown action {action} for DELETE"),
        })
        .map(|r| r.with_status(400)),
    }
}

async fn complete_version_upload(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
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

    let request: CompleteVersionRequest =
        try_or_500!(req.json().await, "Failed to parse request body");

    let db = try_or_500!(ctx.env.d1(DB_NAME), "Failed to get database");
    let timestamp = chrono::Utc::now().to_rfc3339();

    let stmt = try_or_500!(db
        .prepare("INSERT OR REPLACE INTO app_versions (app_name, version, platform, timestamp, sha256) VALUES (?1, ?2, ?3, ?4, ?5)")
        .bind(&[
            app_name.into(),
            request.version.clone().into(),
            request.platform.clone().into(),
            timestamp.into(),
            request.sha256.into(),
        ]), "Failed to prepare database statement");

    try_or_500!(stmt.run().await, "Failed to execute database query");

    Response::from_json(&CompleteVersionResponse {
        success: true,
        message: "Version upload completed successfully".to_string(),
        app_name: app_name.to_string(),
        version: request.version,
        platform: request.platform,
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
        .prepare("SELECT version, timestamp, platform, sha256, created_at FROM app_versions WHERE app_name = ?1 ORDER BY created_at DESC")
        .bind(&[app_name.into()]), "Failed to prepare database statement");

    let result = try_or_500!(stmt.all().await, "Failed to execute database query");

    let mut version_map: std::collections::HashMap<String, VersionResponse> =
        std::collections::HashMap::new();

    for row in result.results::<serde_json::Value>().unwrap_or_default() {
        let version = row["version"].as_str().unwrap_or("").to_string();
        let timestamp = row["timestamp"].as_str().unwrap_or("").to_string();
        let platform = row["platform"].as_str().unwrap_or("").to_string();
        let sha256 = row["sha256"].as_str().unwrap_or("").to_string();

        let key = version.clone();

        let version_response = version_map.entry(key).or_insert_with(|| VersionResponse {
            app_name: app_name.to_string(),
            version: version.clone(),
            timestamp: timestamp.clone(),
            platforms: Vec::new(),
            sha256s: std::collections::HashMap::new(),
        });

        // Use the latest timestamp for this version
        if timestamp > version_response.timestamp {
            version_response.timestamp = timestamp;
        }

        version_response.platforms.push(platform.clone());
        version_response.sha256s.insert(platform, sha256);
    }

    let mut versions: Vec<VersionResponse> = version_map.into_values().collect();
    versions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    Response::from_json(&serde_json::json!({
        "app_name": app_name,
        "versions": versions
    }))
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
        .prepare("SELECT app_name, version, timestamp, platform, sha256 FROM app_versions WHERE app_name = ?1 AND version = ?2 AND platform = ?3")
        .bind(&[app_name.into(), version.into(), platform.into()]), "Failed to prepare database statement");

    let result = try_or_500!(
        stmt.first::<AppVersion>(None).await,
        "Failed to execute database query"
    );

    let _app_version = match result {
        Some(v) => v,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Version not found for platform".to_string(),
            })
            .map(|r| r.with_status(404));
        }
    };

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

    let filename = format!("{app_name}-{platform}-{version}.zip");
    let headers = Headers::new();
    headers.set("Content-Type", "application/zip")?;
    headers.set(
        "Content-Disposition",
        &format!("attachment; filename=\"{filename}\""),
    )?;
    headers.set("Cache-Control", "public, max-age=3600")?;
    headers.set("Content-Length", &file_obj.size().to_string())?;

    // Stream the file directly from R2 without loading into memory
    let body = match file_obj.body() {
        Some(body) => body,
        None => {
            return Response::from_json(&ErrorResponse {
                error: "Failed to get file body stream".to_string(),
            })
            .map(|r| r.with_status(500));
        }
    };

    let stream = try_or_500!(body.stream(), "Failed to get file stream");
    Ok(Response::from_stream(stream)?.with_headers(headers))
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

    let stmt = try_or_500!(
        db.prepare("SELECT platform FROM app_versions WHERE app_name = ?1 AND version = ?2")
            .bind(&[app_name.into(), version.into()]),
        "Failed to prepare database statement"
    );

    let result = try_or_500!(stmt.all().await, "Failed to execute database query");
    let platforms: Vec<String> = result
        .results::<serde_json::Value>()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| row["platform"].as_str().map(|s| s.to_string()))
        .collect();

    if platforms.is_empty() {
        return Response::from_json(&ErrorResponse {
            error: "Version not found".to_string(),
        })
        .map(|r| r.with_status(404));
    }

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
