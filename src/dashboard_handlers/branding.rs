use crate::app::AppState;
use crate::dashboard_handlers::session_helpers::require_admin;
use crate::error::{AppError, AppResult};
use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use image::{ImageFormat, ImageReader};
use sea_orm::ConnectionTrait;
use serde::Serialize;
use std::io::Cursor;

const LOGO_TENANT: &str = "monoize";
const LOGO_FILE_ID: &str = "branding_logo";
const MAX_LOGO_UPLOAD_BYTES: usize = 1024 * 1024;
const MAX_LOGO_EDGE: u32 = 2048;
const MAX_LOGO_PIXELS: u64 = 4_000_000;

#[derive(Debug, Serialize)]
pub struct LogoMutationResponse {
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<&'static str>,
}

pub async fn get_logo(State(state): State<AppState>) -> AppResult<Response> {
    let row = state
        .db_pool
        .read()
        .query_one(state.db_pool.stmt(
            "SELECT bytes FROM file_bytes WHERE tenant_id = $1 AND file_id = $2",
            vec![LOGO_TENANT.into(), LOGO_FILE_ID.into()],
        ))
        .await
        .map_err(|error| {
            AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                error.to_string(),
            )
        })?;
    let Some(row) = row else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            "logo_not_configured",
            "no custom logo is configured",
        ));
    };
    let bytes: Vec<u8> = row.try_get("", "bytes").map_err(|error| {
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            error.to_string(),
        )
    })?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}

pub async fn upload_logo(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> AppResult<impl IntoResponse> {
    require_admin(&headers, &state).await?;
    let mut uploaded: Option<Vec<u8>> = None;
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        AppError::new(StatusCode::BAD_REQUEST, "invalid_logo", error.to_string())
    })? {
        if field.name() != Some("logo") {
            continue;
        }
        if uploaded.is_some() {
            return Err(AppError::new(
                StatusCode::BAD_REQUEST,
                "invalid_logo",
                "only one logo file is allowed",
            ));
        }
        let bytes = field.bytes().await.map_err(|error| {
            AppError::new(StatusCode::BAD_REQUEST, "invalid_logo", error.to_string())
        })?;
        if bytes.len() > MAX_LOGO_UPLOAD_BYTES {
            return Err(AppError::new(
                StatusCode::BAD_REQUEST,
                "invalid_logo",
                "logo file must not exceed 1 MiB",
            ));
        }
        uploaded = Some(normalize_logo(&bytes)?);
    }
    let bytes = uploaded.ok_or_else(|| {
        AppError::new(
            StatusCode::BAD_REQUEST,
            "invalid_logo",
            "multipart field 'logo' is required",
        )
    })?;

    let tx = state.db_pool.begin_write().await.map_err(|error| {
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            error.to_string(),
        )
    })?;
    tx.execute(state.db_pool.stmt(
        "INSERT INTO file_bytes (tenant_id, file_id, bytes) VALUES ($1, $2, $3)
         ON CONFLICT(tenant_id, file_id) DO UPDATE SET bytes = excluded.bytes",
        vec![LOGO_TENANT.into(), LOGO_FILE_ID.into(), bytes.into()],
    ))
    .await
    .map_err(|error| {
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            error.to_string(),
        )
    })?;
    tx.commit().await.map_err(|error| {
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            error.to_string(),
        )
    })?;
    Ok(Json(LogoMutationResponse {
        configured: true,
        url: Some("/api/dashboard/branding/logo"),
    }))
}

pub async fn delete_logo(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    require_admin(&headers, &state).await?;
    state
        .db_pool
        .write()
        .await
        .execute(state.db_pool.stmt(
            "DELETE FROM file_bytes WHERE tenant_id = $1 AND file_id = $2",
            vec![LOGO_TENANT.into(), LOGO_FILE_ID.into()],
        ))
        .await
        .map_err(|error| {
            AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                error.to_string(),
            )
        })?;
    Ok(Json(LogoMutationResponse {
        configured: false,
        url: None,
    }))
}

fn normalize_logo(bytes: &[u8]) -> AppResult<Vec<u8>> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid_logo("could not identify image format"))?;
    let format = reader
        .format()
        .ok_or_else(|| invalid_logo("logo must be PNG, JPEG, or WebP"))?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err(invalid_logo("logo must be PNG, JPEG, or WebP"));
    }
    let dimensions = reader
        .into_dimensions()
        .map_err(|_| invalid_logo("could not read logo dimensions"))?;
    if dimensions.0 > MAX_LOGO_EDGE
        || dimensions.1 > MAX_LOGO_EDGE
        || u64::from(dimensions.0).saturating_mul(u64::from(dimensions.1)) > MAX_LOGO_PIXELS
    {
        return Err(invalid_logo("logo dimensions exceed the allowed limit"));
    }
    let image = image::load_from_memory_with_format(bytes, format)
        .map_err(|_| invalid_logo("logo image is malformed"))?;
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|_| invalid_logo("logo could not be encoded as PNG"))?;
    Ok(output.into_inner())
}

fn invalid_logo(message: &str) -> AppError {
    AppError::new(StatusCode::BAD_REQUEST, "invalid_logo", message)
}
