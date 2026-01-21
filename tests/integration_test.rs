//! Integration tests comparing our implementation output against golden files
//! from ghcr.io/webassembly/wasi/http:0.2.1
//!
//! The golden-test/ folder contains:
//! - manifest.json: The manifest fetched via `oras manifest fetch`
//! - config.blob: The config blob fetched via `oras blob fetch`
//! - layer0.blob: The WASM layer (identical to example/wasi-http.wasm)

use std::collections::HashSet;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Compute SHA256 digest in OCI format
fn sha256_digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    format!("sha256:{:x}", hash)
}

// Structs for deserializing golden files and our output

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct WasmConfig {
    created: String,
    author: Option<String>,
    architecture: String,
    os: String,
    layer_digests: Vec<String>,
    component: Option<ComponentInfo>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ComponentInfo {
    exports: Vec<String>,
    imports: Vec<String>,
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OciManifest {
    schema_version: u8,
    media_type: String,
    config: OciDescriptor,
    layers: Vec<OciDescriptor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OciDescriptor {
    media_type: String,
    digest: String,
    size: i64,
}

// =============================================================================
// Golden file validation tests
// =============================================================================

/// Verify golden-test/layer0.blob is identical to example/wasi-http.wasm
#[test]
fn test_golden_layer_matches_example_wasm() {
    let golden_layer =
        std::fs::read("golden-test/layer0.blob").expect("Failed to read golden layer");
    let example_wasm =
        std::fs::read("example/wasi-http.wasm").expect("Failed to read example wasm");

    assert_eq!(
        golden_layer, example_wasm,
        "Golden layer should be identical to example wasm file"
    );
}

/// Verify golden config digest matches what's in the golden manifest
#[test]
fn test_golden_config_digest_matches_manifest() {
    let golden_manifest: OciManifest =
        serde_json::from_str(include_str!("../golden-test/manifest.json"))
            .expect("Failed to parse golden manifest");

    let golden_config = include_bytes!("../golden-test/config.blob");
    let computed_digest = sha256_digest(golden_config);

    assert_eq!(
        golden_manifest.config.digest, computed_digest,
        "Golden config digest should match manifest reference"
    );

    assert_eq!(
        golden_manifest.config.size,
        golden_config.len() as i64,
        "Golden config size should match manifest"
    );
}

/// Verify golden layer digest matches what's in the golden manifest
#[test]
fn test_golden_layer_digest_matches_manifest() {
    let golden_manifest: OciManifest =
        serde_json::from_str(include_str!("../golden-test/manifest.json"))
            .expect("Failed to parse golden manifest");

    let golden_layer = include_bytes!("../golden-test/layer0.blob");
    let computed_digest = sha256_digest(golden_layer);

    assert_eq!(
        golden_manifest.layers[0].digest, computed_digest,
        "Golden layer digest should match manifest reference"
    );

    assert_eq!(
        golden_manifest.layers[0].size,
        golden_layer.len() as i64,
        "Golden layer size should match manifest"
    );
}

// =============================================================================
// Implementation tests - using the actual public API
// =============================================================================

// Import our actual implementation
use wasm_oci_serve::oci;

/// Test our implementation's layer digest matches the golden manifest
#[test]
fn test_our_layer_digest_matches_golden() {
    let golden_manifest: OciManifest =
        serde_json::from_str(include_str!("../golden-test/manifest.json"))
            .expect("Failed to parse golden manifest");

    let wasm_bytes = std::fs::read("example/wasi-http.wasm").expect("Failed to read wasm");

    // Use our actual implementation
    let our_layer_digest = oci::sha256_digest(&wasm_bytes);

    assert_eq!(
        our_layer_digest, golden_manifest.layers[0].digest,
        "Our layer digest should match golden manifest"
    );
}

/// Test our implementation's config output matches the golden config structure
#[test]
fn test_our_config_matches_golden() {
    let golden_config: WasmConfig =
        serde_json::from_slice(include_bytes!("../golden-test/config.blob"))
            .expect("Failed to parse golden config");

    let wasm_bytes = std::fs::read("example/wasi-http.wasm").expect("Failed to read wasm");

    // Use our actual implementation to load the component
    let entry = oci::load_component(wasm_bytes, Some("wasi-http"))
        .expect("Failed to load component");

    // Parse our generated config
    let our_config: WasmConfig =
        serde_json::from_slice(&entry.config_json).expect("Failed to parse our config");

    // Compare structure (not timestamps, which will differ)
    assert_eq!(
        our_config.architecture, golden_config.architecture,
        "Architecture should match"
    );
    assert_eq!(our_config.os, golden_config.os, "OS should match");
    assert_eq!(
        our_config.layer_digests, golden_config.layer_digests,
        "Layer digests should match"
    );

    // Compare exports as sets (order may differ)
    let our_exports: HashSet<&str> = our_config
        .component
        .as_ref()
        .expect("Our config should have component")
        .exports
        .iter()
        .map(|s| s.as_str())
        .collect();

    let golden_exports: HashSet<&str> = golden_config
        .component
        .as_ref()
        .expect("Golden config should have component")
        .exports
        .iter()
        .map(|s| s.as_str())
        .collect();

    assert_eq!(our_exports, golden_exports, "Exports should match");

    // Compare imports
    let our_imports: HashSet<&str> = our_config
        .component
        .as_ref()
        .unwrap()
        .imports
        .iter()
        .map(|s| s.as_str())
        .collect();

    let golden_imports: HashSet<&str> = golden_config
        .component
        .as_ref()
        .unwrap()
        .imports
        .iter()
        .map(|s| s.as_str())
        .collect();

    assert_eq!(our_imports, golden_imports, "Imports should match");
}

/// Test our implementation's manifest output has correct structure
#[test]
fn test_our_manifest_matches_golden() {
    let golden_manifest: OciManifest =
        serde_json::from_str(include_str!("../golden-test/manifest.json"))
            .expect("Failed to parse golden manifest");

    let wasm_bytes = std::fs::read("example/wasi-http.wasm").expect("Failed to read wasm");

    // Use our actual implementation
    let entry = oci::load_component(wasm_bytes, Some("wasi-http"))
        .expect("Failed to load component");

    // Parse our generated manifest
    let our_manifest: OciManifest =
        serde_json::from_slice(&entry.manifest_json).expect("Failed to parse our manifest");

    // Compare structure
    assert_eq!(
        our_manifest.schema_version, golden_manifest.schema_version,
        "Schema version should match"
    );
    assert_eq!(
        our_manifest.media_type, golden_manifest.media_type,
        "Media type should match"
    );
    assert_eq!(
        our_manifest.config.media_type, golden_manifest.config.media_type,
        "Config media type should match"
    );
    assert_eq!(
        our_manifest.layers.len(),
        golden_manifest.layers.len(),
        "Layer count should match"
    );
    assert_eq!(
        our_manifest.layers[0].media_type, golden_manifest.layers[0].media_type,
        "Layer media type should match"
    );
    assert_eq!(
        our_manifest.layers[0].digest, golden_manifest.layers[0].digest,
        "Layer digest should match"
    );
    assert_eq!(
        our_manifest.layers[0].size, golden_manifest.layers[0].size,
        "Layer size should match"
    );
}
