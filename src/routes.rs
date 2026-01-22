//! OCI Distribution Spec route definitions.
//!
//! This module defines route constants derived from the OCI Distribution Specification.
//! Reference: <https://github.com/opencontainers/distribution-spec/blob/main/spec.md#endpoints>
//!
//! Each endpoint is identified with its spec ID (end-1, end-2, etc.) and includes
//! documentation about supported HTTP methods and expected responses.

/// OCI Distribution Spec API version prefix.
pub const API_VERSION: &str = "/v2";

/// Endpoint definitions from the OCI Distribution Spec.
///
/// Full specification can be found here: https://github.com/opencontainers/distribution-spec/blob/fa23d950713cfaa58ac4372fcfd225bf36c660f0/spec.md#endpoints
/// Note: these are manually specified as there is no machine-readable spec (https://github.com/opencontainers/distribution-spec/issues/566)
pub mod endpoints {
    /// end-1: Version check endpoint. (GET)
    pub const VERSION_CHECK: &str = "/v2/";

    /// end-2: Pull blob endpoint. (GET/HEAD)
    /// end-10: Delete blob (DELETE).
    ///
    /// Route parameters:
    /// - `namespace`: The namespace part of the repository name
    /// - `name`: The name part of the repository
    /// - `digest`: The blob's digest (e.g., sha256:...)
    pub const BLOBS: &str = "/v2/{namespace}/{name}/blobs/{digest}";

    /// end-3: Pull manifest endpoint. (GET/HEAD)
    /// end-7: Push manifest (PUT).
    /// end-9: Delete manifest or tag (DELETE).
    ///
    /// Route parameters:
    /// - `namespace`: The namespace part of the repository name
    /// - `name`: The name part of the repository
    /// - `reference`: Either a digest or a tag
    pub const MANIFESTS: &str = "/v2/{namespace}/{name}/manifests/{reference}";

    /// end-8a: List tags endpoint (GET)
    /// end-8b: List paginated tags (GET with ?n=&last=)
    ///
    /// Route parameters:
    /// - `namespace`: The namespace part of the repository name
    /// - `name`: The name part of the repository
    pub const TAGS_LIST: &str = "/v2/{namespace}/{name}/tags/list";

    /// end-4a: Initiate blob upload (POST).
    /// end-4b: Single POST blob upload (POST with ?digest=).
    /// end-11: Cross-repo blob mount (POST with ?mount=&from=).
    ///
    /// Route parameters:
    /// - `namespace`: The namespace part of the repository name
    /// - `name`: The name part of the repository
    pub const BLOB_UPLOADS: &str = "/v2/{namespace}/{name}/blobs/uploads/";

    /// end-5: Chunked blob upload (PATCH).
    /// end-6: Complete blob upload (PUT with ?digest=).
    /// end-13: Get upload status (GET).
    ///
    /// Route parameters:
    /// - `namespace`: The namespace part of the repository name
    /// - `name`: The name part of the repository
    /// - `reference`: The digest or tag of the blob
    pub const BLOB_UPLOAD_SESSION: &str = "/v2/{namespace}/{name}/blobs/uploads/{reference}";

    /// end-12a: List referrers (GET).
    /// end-12b: List referrers with filter (GET with ?artifactType=).
    ///
    /// Route parameters:
    /// - `namespace`: The namespace part of the repository name
    /// - `name`: The name part of the repository
    /// - `digest`: The digest of the manifest
    pub const REFERRERS: &str = "/v2/{namespace}/{name}/referrers/{digest}";
}

/// Validation patterns from the OCI Distribution Spec.
pub mod patterns {
    /// Repository name pattern.
    ///
    /// From spec: `[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*(\/[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*)*`
    pub const NAME_PATTERN: &str =
        r"[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*(\/[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*)*";

    /// Tag reference pattern (max 128 chars).
    ///
    /// From spec: `[a-zA-Z0-9_][a-zA-Z0-9._-]{0,127}`
    pub const TAG_PATTERN: &str = r"[a-zA-Z0-9_][a-zA-Z0-9._-]{0,127}";

    /// Maximum tag length.
    pub const MAX_TAG_LENGTH: usize = 128;
}

/// Error codes from the OCI Distribution Spec.
pub mod error_codes {
    /// code-1: Blob unknown to registry.
    pub const BLOB_UNKNOWN: &str = "BLOB_UNKNOWN";

    /// code-2: Blob upload invalid.
    #[allow(dead_code)]
    pub const BLOB_UPLOAD_INVALID: &str = "BLOB_UPLOAD_INVALID";

    /// code-3: Blob upload unknown to registry.
    #[allow(dead_code)]
    pub const BLOB_UPLOAD_UNKNOWN: &str = "BLOB_UPLOAD_UNKNOWN";

    /// code-4: Provided digest did not match uploaded content.
    #[allow(dead_code)]
    pub const DIGEST_INVALID: &str = "DIGEST_INVALID";

    /// code-5: Manifest references a manifest or blob unknown to registry.
    #[allow(dead_code)]
    pub const MANIFEST_BLOB_UNKNOWN: &str = "MANIFEST_BLOB_UNKNOWN";

    /// code-6: Manifest invalid.
    #[allow(dead_code)]
    pub const MANIFEST_INVALID: &str = "MANIFEST_INVALID";

    /// code-7: Manifest unknown to registry.
    pub const MANIFEST_UNKNOWN: &str = "MANIFEST_UNKNOWN";

    /// code-8: Invalid repository name.
    #[allow(dead_code)]
    pub const NAME_INVALID: &str = "NAME_INVALID";

    /// code-9: Repository name not known to registry.
    pub const NAME_UNKNOWN: &str = "NAME_UNKNOWN";

    /// code-10: Provided length did not match content length.
    #[allow(dead_code)]
    pub const SIZE_INVALID: &str = "SIZE_INVALID";

    /// code-11: Authentication required.
    #[allow(dead_code)]
    pub const UNAUTHORIZED: &str = "UNAUTHORIZED";

    /// code-12: Requested access to the resource is denied.
    #[allow(dead_code)]
    pub const DENIED: &str = "DENIED";

    /// code-13: The operation is unsupported.
    pub const UNSUPPORTED: &str = "UNSUPPORTED";

    /// code-14: Too many requests.
    #[allow(dead_code)]
    pub const TOOMANYREQUESTS: &str = "TOOMANYREQUESTS";
}

/// HTTP headers used in OCI responses.
pub mod headers {
    /// Docker-Content-Digest header.
    ///
    /// From spec: "A successful response SHOULD contain the digest of the uploaded blob
    /// in the header Docker-Content-Digest."
    pub const DOCKER_CONTENT_DIGEST: &str = "Docker-Content-Digest";
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    #[test]
    fn test_version_check_route() {
        assert_eq!(endpoints::VERSION_CHECK, "/v2/");
    }

    #[test]
    fn test_blobs_route_format() {
        // Verify the route contains expected parameters
        assert!(endpoints::BLOBS.contains("{namespace}"));
        assert!(endpoints::BLOBS.contains("{name}"));
        assert!(endpoints::BLOBS.contains("{digest}"));
        assert!(endpoints::BLOBS.starts_with("/v2/"));
        assert!(endpoints::BLOBS.contains("/blobs/"));
    }

    #[test]
    fn test_manifests_route_format() {
        assert!(endpoints::MANIFESTS.contains("{namespace}"));
        assert!(endpoints::MANIFESTS.contains("{name}"));
        assert!(endpoints::MANIFESTS.contains("{reference}"));
        assert!(endpoints::MANIFESTS.starts_with("/v2/"));
        assert!(endpoints::MANIFESTS.contains("/manifests/"));
    }

    #[test]
    fn test_tags_list_route_format() {
        assert!(endpoints::TAGS_LIST.contains("{namespace}"));
        assert!(endpoints::TAGS_LIST.contains("{name}"));
        assert!(endpoints::TAGS_LIST.starts_with("/v2/"));
        assert!(endpoints::TAGS_LIST.ends_with("/tags/list"));
    }

    #[test]
    fn test_name_pattern_valid() {
        let re = Regex::new(&format!("^{}$", patterns::NAME_PATTERN)).unwrap();

        // Valid names from the spec
        assert!(re.is_match("library/ubuntu"));
        assert!(re.is_match("wasi/http"));
        assert!(re.is_match("namespace/name"));
        assert!(re.is_match("simple"));
        assert!(re.is_match("with-dash"));
        assert!(re.is_match("with.dot"));
        assert!(re.is_match("with_underscore"));
        assert!(re.is_match("with__double"));
        assert!(re.is_match("multi/level/path"));
    }

    #[test]
    fn test_name_pattern_invalid() {
        let re = Regex::new(&format!("^{}$", patterns::NAME_PATTERN)).unwrap();

        // Invalid names
        assert!(!re.is_match("UPPERCASE"));
        assert!(!re.is_match("-startwithdash"));
        assert!(!re.is_match(".startwithdot"));
    }

    #[test]
    fn test_tag_pattern_valid() {
        let re = Regex::new(&format!("^{}$", patterns::TAG_PATTERN)).unwrap();

        // Valid tags
        assert!(re.is_match("latest"));
        assert!(re.is_match("v1.0.0"));
        assert!(re.is_match("1.0"));
        assert!(re.is_match("release-1"));
        assert!(re.is_match("_private"));
    }

    #[test]
    fn test_tag_max_length() {
        assert_eq!(patterns::MAX_TAG_LENGTH, 128);
    }

    #[test]
    fn test_error_codes_uppercase() {
        // Spec requires: "containing only uppercase alphabetic characters and underscores"
        let codes = [
            error_codes::BLOB_UNKNOWN,
            error_codes::MANIFEST_UNKNOWN,
            error_codes::NAME_UNKNOWN,
        ];

        for code in codes {
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "Error code '{}' must be uppercase with underscores only",
                code
            );
        }
    }
}
