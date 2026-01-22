//! HTTP server implementing OCI registry endpoints.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde::Serialize;

use crate::oci::{WASM_CONFIG_MEDIA_TYPE, WASM_LAYER_MEDIA_TYPE, WASM_MANIFEST_MEDIA_TYPE};
use crate::registry::{BlobType, Registry};
use crate::routes::{endpoints, error_codes, headers};

/// Shared application state.
pub struct AppState {
    pub registry: Registry,
}

/// Build an OCI-compliant error response.
///
/// From the spec: https://github.com/opencontainers/distribution-spec/blob/fa23d950713cfaa58ac4372fcfd225bf36c660f0/spec.md#error-codes
/// A 4XX response code from the registry MAY return a body in JSON format
/// with an "errors" array containing objects with "code", "message", and optional "detail" fields.
fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let body = format!(
        r#"{{"errors":[{{"code":"{}","message":"{}"}}]}}"#,
        code, message
    );
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

/// Create the router with all OCI endpoints.
///
/// Routes are defined in the `routes` module based on the OCI Distribution Spec.
/// See: <https://github.com/opencontainers/distribution-spec/blob/main/spec.md#endpoints>
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // end-1: OCI version check (GET)
        .route(endpoints::VERSION_CHECK, get(version_check))
        // end-2: Pull blob (GET/HEAD)
        // end-10: Delete blob (DELETE) - UNSUPPORTED
        .route(
            endpoints::BLOBS,
            get(get_blob)
                .head(head_blob)
                .delete(delete_blob_unsupported),
        )
        // end-3: Pull manifest (GET/HEAD)
        // end-7: Push manifest (PUT) - UNSUPPORTED
        // end-9: Delete manifest (DELETE) - UNSUPPORTED
        .route(
            endpoints::MANIFESTS,
            get(get_manifest)
                .head(head_manifest)
                .put(put_manifest_unsupported)
                .delete(delete_manifest_unsupported),
        )
        // end-4a: Initiate blob upload (POST) - UNSUPPORTED
        // end-4b: Single POST blob upload (POST with ?digest=) - UNSUPPORTED
        // end-11: Cross-repo blob mount (POST with ?mount=&from=) - UNSUPPORTED
        .route(endpoints::BLOB_UPLOADS, post(blob_upload_unsupported))
        // end-5: Chunked blob upload (PATCH) - UNSUPPORTED
        // end-6: Complete blob upload (PUT) - UNSUPPORTED
        // end-13: Get upload status (GET) - UNSUPPORTED
        .route(
            endpoints::BLOB_UPLOAD_SESSION,
            get(blob_upload_session_unsupported)
                .patch(blob_upload_session_unsupported)
                .put(blob_upload_session_unsupported),
        )
        // end-8a: List tags (GET)
        // end-8b: List paginated tags (GET with ?n=&last=)
        .route(endpoints::TAGS_LIST, get(list_tags))
        // end-12a: List referrers (GET) - UNSUPPORTED
        // end-12b: List referrers with filter (GET with ?artifactType=) - UNSUPPORTED
        .route(endpoints::REFERRERS, get(referrers_unsupported))
        .with_state(state)
}

/// end-1: OCI version check
async fn version_check() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        "{}",
    )
}

/// end-2: Pull blob endpoint. (GET)
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
                .header(headers::DOCKER_CONTENT_DIGEST, &digest)
                .header(header::CONTENT_LENGTH, data.len())
                .body(Body::from(data))
                .unwrap()
        }
        None => error_response(
            StatusCode::NOT_FOUND,
            error_codes::BLOB_UNKNOWN,
            "blob unknown",
        ),
    }
}

/// end-2: Pull blob endpoint. (HEAD)
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
                .header(headers::DOCKER_CONTENT_DIGEST, &digest)
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

/// end-3: Manifest endpoints (GET)
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
            .header(headers::DOCKER_CONTENT_DIGEST, &entry.manifest_digest)
            .header(header::CONTENT_LENGTH, entry.manifest_json.len())
            .body(Body::from(entry.manifest_json.clone()))
            .unwrap(),
        None => error_response(
            StatusCode::NOT_FOUND,
            error_codes::MANIFEST_UNKNOWN,
            "manifest unknown",
        ),
    }
}

/// end-3: Manifest endpoints (HEAD)
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
            .header(headers::DOCKER_CONTENT_DIGEST, &entry.manifest_digest)
            .header(header::CONTENT_LENGTH, entry.manifest_json.len())
            .body(Body::empty())
            .unwrap(),
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

/// Query parameters for tag listing (end-8b).
#[derive(Deserialize, Default)]
struct TagsListQuery {
    /// Number of tags to return.
    n: Option<usize>,
    /// Return tags lexically after this value (exclusive).
    last: Option<String>,
}

/// end-8a: List tags endpoint (GET)
/// end-8b: List paginated tags (GET with ?n=&last=)
async fn list_tags(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
    Query(query): Query<TagsListQuery>,
) -> Response {
    let repository = format!("{}/{}", namespace, name);

    match state.registry.get_tags(&repository) {
        Some(tags) => {
            // Tags from registry are already sorted lexically.
            // Apply pagination per spec:
            // - `last`: start after this tag (exclusive)
            // - `n`: limit number of results
            let filtered_tags: Vec<String> = tags
                .iter()
                .filter(|tag| {
                    // If `last` is provided, only include tags that come after it lexically
                    match &query.last {
                        Some(last) => tag.as_str() > last.as_str(),
                        None => true,
                    }
                })
                .take(query.n.unwrap_or(usize::MAX))
                .cloned()
                .collect();

            let response = TagsListResponse {
                name: repository,
                tags: filtered_tags,
            };
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&response).unwrap()))
                .unwrap()
        }
        None => error_response(
            StatusCode::NOT_FOUND,
            error_codes::NAME_UNKNOWN,
            "repository name not known to registry",
        ),
    }
}


// =============================================================================
// Unsupported endpoints
// =============================================================================

/// Helper for unsupported operation responses.
fn unsupported_response(operation: &str) -> Response {
    error_response(
        StatusCode::METHOD_NOT_ALLOWED,
        error_codes::UNSUPPORTED,
        &format!("{} is not supported by this registry", operation),
    )
}

/// end-4a/4b/11: Blob upload initiation (POST) - UNSUPPORTED
async fn blob_upload_unsupported(
    Path((_namespace, _name)): Path<(String, String)>,
) -> Response {
    unsupported_response("blob uploads")
}

/// end-5/6/13: Blob upload session operations (GET/PATCH/PUT) - UNSUPPORTED
async fn blob_upload_session_unsupported(
    Path((_namespace, _name, _reference)): Path<(String, String, String)>,
) -> Response {
    unsupported_response("blob upload sessions")
}

/// end-7: Push manifest (PUT) - UNSUPPORTED
async fn put_manifest_unsupported(
    Path((_namespace, _name, _reference)): Path<(String, String, String)>,
) -> Response {
    unsupported_response("pushing manifests")
}

/// end-9: Delete manifest or tag (DELETE) - UNSUPPORTED
async fn delete_manifest_unsupported(
    Path((_namespace, _name, _reference)): Path<(String, String, String)>,
) -> Response {
    unsupported_response("deleting manifests")
}

/// end-10: Delete blob (DELETE) - UNSUPPORTED
async fn delete_blob_unsupported(
    Path((_namespace, _name, _digest)): Path<(String, String, String)>,
) -> Response {
    unsupported_response("deleting blobs")
}

/// end-12a/12b: List referrers (GET) - UNSUPPORTED
async fn referrers_unsupported(
    Path((_namespace, _name, _digest)): Path<(String, String, String)>,
) -> Response {
    unsupported_response("referrers API")
}
