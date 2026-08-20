use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct WebAssets;

pub async fn static_handler(uri: Uri, _req: Request<Body>) -> Response {
    let mut path = uri.path().trim_start_matches('/').to_string();

    if path.is_empty() {
        path = "index.html".to_string();
    }

    // Try serving exact file
    if let Some(file) = WebAssets::get(&path) {
        let mime = mime_guess::from_path(&path).first_or_octet_stream();
        let mut response = (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            file.data,
        )
            .into_response();

        // Caching headers
        if path.starts_with("assets/") {
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
        } else if path == "sw.js" || path == "manifest.webmanifest" || path == "index.html" {
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-cache, no-store, must-revalidate"),
            );
        }

        return response;
    }

    // If path starts with api/ or health, return 404 instead of SPA fallback
    if path.starts_with("api/") || path == "health" {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    // SPA fallback to index.html
    if let Some(index) = WebAssets::get("index.html") {
        let mut response = (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            index.data,
        )
            .into_response();
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        );
        return response;
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}
