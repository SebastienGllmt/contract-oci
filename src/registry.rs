//! In-memory registry data structures for WASM components.

use std::collections::HashMap;
use std::sync::Arc;

/// Type of blob stored in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobType {
    /// WASM component bytes
    Wasm,
    /// OCI config JSON
    Config,
    /// OCI manifest JSON
    Manifest,
}

/// Registry entry for a single component version.
#[derive(Debug)]
pub struct ComponentEntry {
    /// Raw WASM bytes
    pub wasm_bytes: Vec<u8>,
    /// sha256:... digest of WASM bytes
    pub wasm_digest: String,

    /// Pre-computed config JSON (deterministic)
    pub config_json: Vec<u8>,
    /// sha256:... digest of config
    pub config_digest: String,

    /// Pre-computed manifest JSON (deterministic)
    pub manifest_json: Vec<u8>,
    /// sha256:... digest of manifest
    pub manifest_digest: String,

    /// Parsed metadata
    pub namespace: String,
    pub name: String,
    pub version: String,
}

impl ComponentEntry {
    /// Returns the full reference string (namespace/name:version).
    pub fn reference(&self) -> String {
        format!("{}/{}:{}", self.namespace, self.name, self.version)
    }

    /// Returns the repository path (namespace/name).
    pub fn repository(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }
}

/// In-memory registry storing component entries.
#[derive(Debug, Default)]
pub struct Registry {
    /// namespace/name:version -> entry
    by_reference: HashMap<String, Arc<ComponentEntry>>,
    /// digest -> (type, entry) for blob lookups
    by_digest: HashMap<String, (BlobType, Arc<ComponentEntry>)>,
    /// repository (namespace/name) -> list of versions
    tags_by_repo: HashMap<String, Vec<String>>,
}

impl Registry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a component entry in the registry.
    pub fn register(&mut self, entry: ComponentEntry) {
        let reference = entry.reference();
        let repository = entry.repository();
        let version = entry.version.clone();

        let entry = Arc::new(entry);

        // Index by reference
        self.by_reference.insert(reference, Arc::clone(&entry));

        // Index by digests
        self.by_digest.insert(
            entry.wasm_digest.clone(),
            (BlobType::Wasm, Arc::clone(&entry)),
        );
        self.by_digest.insert(
            entry.config_digest.clone(),
            (BlobType::Config, Arc::clone(&entry)),
        );
        self.by_digest.insert(
            entry.manifest_digest.clone(),
            (BlobType::Manifest, Arc::clone(&entry)),
        );

        // Index tags by repository
        self.tags_by_repo
            .entry(repository)
            .or_default()
            .push(version);
    }

    /// Look up an entry by reference (namespace/name:version or namespace/name@sha256:...).
    pub fn get_by_reference(&self, reference: &str) -> Option<Arc<ComponentEntry>> {
        // First try direct lookup
        if let Some(entry) = self.by_reference.get(reference) {
            return Some(Arc::clone(entry));
        }

        // Check if it's a digest reference (namespace/name@sha256:...)
        if let Some(digest_start) = reference.find('@') {
            let digest = &reference[digest_start + 1..];
            if let Some((BlobType::Manifest, entry)) = self.by_digest.get(digest) {
                return Some(Arc::clone(entry));
            }
        }

        None
    }

    /// Look up a blob by digest.
    pub fn get_blob(&self, digest: &str) -> Option<(BlobType, Arc<ComponentEntry>)> {
        self.by_digest
            .get(digest)
            .map(|(t, e)| (*t, Arc::clone(e)))
    }

    /// Get all tags for a repository.
    pub fn get_tags(&self, repository: &str) -> Option<&Vec<String>> {
        self.tags_by_repo.get(repository)
    }

    /// Get all registered entries.
    pub fn entries(&self) -> impl Iterator<Item = &Arc<ComponentEntry>> {
        self.by_reference.values()
    }
}
