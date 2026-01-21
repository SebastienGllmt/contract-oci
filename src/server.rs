//! HTTP server implementing OCI registry endpoints.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Serialize;

use crate::oci::{WASM_CONFIG_MEDIA_TYPE, WASM_LAYER_MEDIA_TYPE, WASM_MANIFEST_MEDIA_TYPE};
use crate::registry::{BlobType, Registry};

/// Shared application state.
pub struct AppState {
    pub registry: Registry,
}

/// Create the router with all OCI endpoints.
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // OCI version check
        .route("/v2/", get(version_check))
        // Manifest endpoints - use path with wildcards for namespace/name
        .route(
            "/v2/{namespace}/{name}/manifests/{reference}",
            get(get_manifest).head(head_manifest),
        )
        // Blob endpoints
        .route(
            "/v2/{namespace}/{name}/blobs/{digest}",
            get(get_blob).head(head_blob),
        )
        // Tags list
        .route("/v2/{namespace}/{name}/tags/list", get(list_tags))
        .with_state(state)
}

/// GET /v2/ - Version check endpoint.
async fn version_check() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        "{}",
    )
}

/// GET /v2/{namespace}/{name}/manifests/{reference}
async fn get_manifest(
    State(state): State<Arc<AppState>>,
    Path((namespace, name, reference)): Path<(String, String, String)>,
) -> Response {
    let full_ref = format!("{}/{}:{}", namespace, name, reference);

    // Try to find by tag first, then by digest
    let entry = state
        .registry
        .get_by_reference(&full_ref)
        .or_else(|| {
            // Maybe it's a digest reference
            if reference.starts_with("sha256:") {
                state
                    .registry
                    .get_blob(&reference)
                    .and_then(|(blob_type, entry)| {
                        if blob_type == BlobType::Manifest {
                            Some(entry)
                        } else {
                            None
                        }
                    })
            } else {
                None
            }
        });

    match entry {
        Some(entry) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, WASM_MANIFEST_MEDIA_TYPE)
            .header("Docker-Content-Digest", &entry.manifest_digest)
            .header(header::CONTENT_LENGTH, entry.manifest_json.len())
            .body(Body::from(entry.manifest_json.clone()))
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"errors":[{"code":"MANIFEST_UNKNOWN","message":"manifest unknown"}]}"#,
            ))
            .unwrap(),
    }
}

/// HEAD /v2/{namespace}/{name}/manifests/{reference}
async fn head_manifest(
    State(state): State<Arc<AppState>>,
    Path((namespace, name, reference)): Path<(String, String, String)>,
) -> Response {
    let full_ref = format!("{}/{}:{}", namespace, name, reference);

    let entry = state
        .registry
        .get_by_reference(&full_ref)
        .or_else(|| {
            if reference.starts_with("sha256:") {
                state
                    .registry
                    .get_blob(&reference)
                    .and_then(|(blob_type, entry)| {
                        if blob_type == BlobType::Manifest {
                            Some(entry)
                        } else {
                            None
                        }
                    })
            } else {
                None
            }
        });

    match entry {
        Some(entry) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, WASM_MANIFEST_MEDIA_TYPE)
            .header("Docker-Content-Digest", &entry.manifest_digest)
            .header(header::CONTENT_LENGTH, entry.manifest_json.len())
            .body(Body::empty())
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap(),
    }
}

/// GET /v2/{namespace}/{name}/blobs/{digest}
async fn get_blob(
    State(state): State<Arc<AppState>>,
    Path((_namespace, _name, digest)): Path<(String, String, String)>,
) -> Response {
    match state.registry.get_blob(&digest) {
        Some((blob_type, entry)) => {
            let (content_type, data) = match blob_type {
                BlobType::Wasm => (WASM_LAYER_MEDIA_TYPE, entry.wasm_bytes.clone()),
                BlobType::Config => (WASM_CONFIG_MEDIA_TYPE, entry.config_json.clone()),
                BlobType::Manifest => (WASM_MANIFEST_MEDIA_TYPE, entry.manifest_json.clone()),
            };

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header("Docker-Content-Digest", &digest)
                .header(header::CONTENT_LENGTH, data.len())
                .body(Body::from(data))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"errors":[{"code":"BLOB_UNKNOWN","message":"blob unknown"}]}"#,
            ))
            .unwrap(),
    }
}

/// HEAD /v2/{namespace}/{name}/blobs/{digest}
async fn head_blob(
    State(state): State<Arc<AppState>>,
    Path((_namespace, _name, digest)): Path<(String, String, String)>,
) -> Response {
    match state.registry.get_blob(&digest) {
        Some((blob_type, entry)) => {
            let (content_type, size) = match blob_type {
                BlobType::Wasm => (WASM_LAYER_MEDIA_TYPE, entry.wasm_bytes.len()),
                BlobType::Config => (WASM_CONFIG_MEDIA_TYPE, entry.config_json.len()),
                BlobType::Manifest => (WASM_MANIFEST_MEDIA_TYPE, entry.manifest_json.len()),
            };

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header("Docker-Content-Digest", &digest)
                .header(header::CONTENT_LENGTH, size)
                .body(Body::empty())
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap(),
    }
}

/// Tags list response.
#[derive(Serialize)]
struct TagsListResponse {
    name: String,
    tags: Vec<String>,
}

/// GET /v2/{namespace}/{name}/tags/list
async fn list_tags(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
) -> Response {
    let repository = format!("{}/{}", namespace, name);

    match state.registry.get_tags(&repository) {
        Some(tags) => {
            let response = TagsListResponse {
                name: repository,
                tags: tags.clone(),
            };
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&response).unwrap()))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"errors":[{"code":"NAME_UNKNOWN","message":"repository name not known to registry"}]}"#,
            ))
            .unwrap(),
    }
}
