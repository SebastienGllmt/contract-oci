//! OCI artifact generation for WASM components.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::registry::ComponentEntry;

// Media types
pub const WASM_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
pub const WASM_CONFIG_MEDIA_TYPE: &str = "application/vnd.wasm.config.v0+json";
pub const WASM_LAYER_MEDIA_TYPE: &str = "application/wasm";

/// OCI descriptor for referencing blobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OciDescriptor {
    pub media_type: String,
    pub digest: String,
    pub size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<std::collections::BTreeMap<String, String>>,
}

/// OCI image manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OciManifest {
    pub schema_version: u8,
    pub media_type: String,
    pub config: OciDescriptor,
    pub layers: Vec<OciDescriptor>,
}

/// Component information embedded in config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentInfo {
    pub exports: Vec<String>,
    pub imports: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// WASM config blob structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmConfig {
    pub created: DateTime<Utc>,
    pub architecture: String,
    pub os: String,
    pub layer_digests: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<ComponentInfo>,
}

/// Compute SHA256 digest of bytes in OCI format (sha256:hex).
pub fn sha256_digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    format!("sha256:{:x}", hash)
}

/// Extract component info (exports/imports) from WASM bytes.
fn extract_component_info(wasm_bytes: &[u8]) -> Result<ComponentInfo> {
    use std::collections::HashSet;

    let decoded = wit_component::decode(wasm_bytes).context("Failed to decode WASM component")?;

    match decoded {
        wit_component::DecodedWasm::Component(resolve, world_id) => {
            // For components, extract from the world's exports/imports
            let world = &resolve.worlds[world_id];

            let exports: Vec<String> = world
                .exports
                .keys()
                .map(|key| resolve.name_world_key(key))
                .collect();

            let imports: Vec<String> = world
                .imports
                .keys()
                .map(|key| resolve.name_world_key(key))
                .collect();

            Ok(ComponentInfo {
                exports,
                imports,
                target: None,
            })
        }
        wit_component::DecodedWasm::WitPackage(resolve, pkg_id) => {
            // For WIT packages, collect exports from all worlds and package interfaces
            let pkg = &resolve.packages[pkg_id];

            let mut exports = HashSet::new();

            // Collect from worlds
            for (_name, world_id) in &pkg.worlds {
                let world = &resolve.worlds[*world_id];

                // Add world exports
                for key in world.exports.keys() {
                    exports.insert(resolve.name_world_key(key));
                }

                // Add fully qualified world name
                let mut fq_world = format!("{}:{}/{}", pkg.name.namespace, pkg.name.name, world.name);
                if let Some(ver) = pkg.name.version.as_ref() {
                    fq_world.push('@');
                    fq_world.push_str(&ver.to_string());
                }
                exports.insert(fq_world);
            }

            // Add package interface IDs
            for iface_id in pkg.interfaces.values() {
                if let Some(id) = resolve.id_of(*iface_id) {
                    exports.insert(id);
                }
            }

            Ok(ComponentInfo {
                exports: exports.into_iter().collect(),
                imports: vec![],
                target: None,
            })
        }
    }
}

/// Extract metadata (namespace, name, version) from WASM component.
/// If filename_fallback is provided, it will be used when metadata is missing.
pub fn extract_metadata(
    wasm_bytes: &[u8],
    filename_fallback: Option<&str>,
) -> Result<(String, String, String)> {
    let payload = wasm_metadata::Payload::from_binary(wasm_bytes)
        .context("Failed to parse WASM metadata")?;
    let meta = payload.metadata();

    // Try to get name from metadata, fall back to filename
    let full_name = meta
        .name
        .as_ref()
        .map(|n| n.to_string())
        .or_else(|| filename_fallback.map(|f| f.to_string()))
        .context("WASM component missing 'name' in metadata and no filename provided.")?;

    // Parse namespace:name format
    let (namespace, name) = if let Some(colon_pos) = full_name.find(':') {
        (
            full_name[..colon_pos].to_string(),
            full_name[colon_pos + 1..].to_string(),
        )
    } else {
        // If no colon, use "local" as namespace
        ("local".to_string(), full_name)
    };

    // Get version from metadata
    let version = meta
        .version
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "0.0.0".to_string());

    Ok((namespace, name, version))
}

/// Generate config JSON and its digest from WASM bytes.
pub fn generate_config(wasm_bytes: &[u8], wasm_digest: &str) -> Result<(Vec<u8>, String)> {
    let component_info = extract_component_info(wasm_bytes)?;

    let config = WasmConfig {
        created: Utc::now(),
        architecture: "wasm".to_string(),
        os: "wasip2".to_string(),
        layer_digests: vec![wasm_digest.to_string()],
        component: Some(component_info),
    };

    let config_json = serde_json::to_vec_pretty(&config).context("Failed to serialize config")?;
    let config_digest = sha256_digest(&config_json);

    Ok((config_json, config_digest))
}

/// Generate manifest JSON and its digest.
pub fn generate_manifest(
    wasm_size: usize,
    wasm_digest: &str,
    config_size: usize,
    config_digest: &str,
    layer_title: &str,
) -> Result<(Vec<u8>, String)> {
    use std::collections::BTreeMap;

    // Add title annotation so oras knows the filename
    let mut layer_annotations = BTreeMap::new();
    layer_annotations.insert(
        "org.opencontainers.image.title".to_string(),
        layer_title.to_string(),
    );

    let manifest = OciManifest {
        schema_version: 2,
        media_type: WASM_MANIFEST_MEDIA_TYPE.to_string(),
        config: OciDescriptor {
            media_type: WASM_CONFIG_MEDIA_TYPE.to_string(),
            digest: config_digest.to_string(),
            size: config_size as i64,
            annotations: None,
        },
        layers: vec![OciDescriptor {
            media_type: WASM_LAYER_MEDIA_TYPE.to_string(),
            digest: wasm_digest.to_string(),
            size: wasm_size as i64,
            annotations: Some(layer_annotations),
        }],
    };

    let manifest_json =
        serde_json::to_vec_pretty(&manifest).context("Failed to serialize manifest")?;
    let manifest_digest = sha256_digest(&manifest_json);

    Ok((manifest_json, manifest_digest))
}

/// Load a WASM file and create a ComponentEntry with all OCI artifacts.
/// If filename_hint is provided, it will be used as a fallback name when metadata is missing.
pub fn load_component(wasm_bytes: Vec<u8>, filename_hint: Option<&str>) -> Result<ComponentEntry> {
    // Extract metadata
    let (namespace, name, version) = extract_metadata(&wasm_bytes, filename_hint)?;

    // Compute WASM digest
    let wasm_digest = sha256_digest(&wasm_bytes);

    // Generate config
    let (config_json, config_digest) = generate_config(&wasm_bytes, &wasm_digest)?;

    // Layer title for oras to use as filename
    let layer_title = format!("{}.wasm", name);

    // Generate manifest
    let (manifest_json, manifest_digest) = generate_manifest(
        wasm_bytes.len(),
        &wasm_digest,
        config_json.len(),
        &config_digest,
        &layer_title,
    )?;

    Ok(ComponentEntry {
        wasm_bytes,
        wasm_digest,
        config_json,
        config_digest,
        manifest_json,
        manifest_digest,
        namespace,
        name,
        version,
    })
}
