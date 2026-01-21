# WASM Component OCI Registry - Proof of Concept

## Overview

Build a **Rust CLI tool** that demonstrates serving WASM components via OCI registry protocol. The tool will:
1. Take `.wasm` component files as input
2. Generate OCI artifacts (manifest, config) on-demand
3. Serve via minimal HTTP server for testing with `oras pull`

This PoC validates the architecture pattern that could be used for a real server implementation.

## Architecture Approach

```
┌─────────────────────────────────────────────────────────────┐
│                     CLI Tool (Rust)                         │
├─────────────────────────────────────────────────────────────┤
│  1. Load .wasm files from disk (or directory)               │
│  2. Parse & extract metadata on startup                     │
│  3. Compute digests, store in-memory registry               │
│  4. Spin up minimal HTTP server on localhost                │
│  5. Serve OCI endpoints, generate artifacts on-demand       │
└─────────────────────────────────────────────────────────────┘
         │
         ▼
    oras pull localhost:5000/namespace/name:version
```

**Key Insight**: Store only raw WASM bytes + metadata in memory. Generate OCI manifests/configs on-demand from this data.

---

## 1. User Preparation (Metadata Embedding)

Users embed metadata in their WASM files using existing tooling:

```bash
# Using wkg (from wasm-pkg-tools)
wkg wit build --output example/adder.wasm

# wkg.toml specifies metadata
[metadata]
authors = "Author Name"
description = "Package description"
licenses = "MIT"
```

Or programmatically with `wasm_metadata::AddMetadata`:
```rust
let mut meta = AddMetadata::default();
meta.name = AddMetadataField::Set(Some("namespace:name".into()));
meta.version = AddMetadataField::Set(Some(semver::Version::parse("0.1.0")?));
let enriched = meta.to_wasm(&wasm_bytes)?;
```

**Minimum required**: WASM must be a valid component (parseable by `wit_component::decode`).

---

## 2. In-Memory Data Model

For the PoC, everything lives in memory:

```rust
/// Registry entry for a single component version
struct ComponentEntry {
    /// Raw WASM bytes
    wasm_bytes: Vec<u8>,
    wasm_digest: String,  // sha256:...

    /// Pre-computed config (deterministic)
    config_json: Vec<u8>,
    config_digest: String,

    /// Pre-computed manifest (deterministic)
    manifest_json: Vec<u8>,
    manifest_digest: String,

    /// Parsed metadata
    namespace: String,
    name: String,
    version: String,
}

/// In-memory registry
struct Registry {
    /// namespace/name:version -> entry
    by_reference: HashMap<String, Arc<ComponentEntry>>,
    /// digest -> (type, entry) for blob lookups
    by_digest: HashMap<String, (BlobType, Arc<ComponentEntry>)>,
}
```

**Production note**: For a real server, replace `HashMap` with database queries and stream WASM bytes from object storage instead of holding in memory.

---

## 3. OCI Distribution Spec Endpoints

Reference: [OCI Distribution Spec](../distribution-spec/spec.md) defines the HTTP API for OCI registries.

### Implemented Endpoints

These endpoints are required for `oras pull` to work (the **Pull** workflow category):

| ID | Method | Endpoint | Description | Status |
|----|--------|----------|-------------|--------|
| end-1 | `GET` | `/v2/` | Version check | **Implemented** |
| end-2 | `GET`/`HEAD` | `/v2/<name>/blobs/<digest>` | Pull blob | **Implemented** |
| end-3 | `GET`/`HEAD` | `/v2/<name>/manifests/<reference>` | Pull manifest | **Implemented** |
| end-8a | `GET` | `/v2/<name>/tags/list` | List tags | **Implemented** |

### Not Implemented - Push Endpoints

**Reason**: This is a read-only registry that serves WASM files stored onchain. Push operations are out of scope, as projects must be submitted through the smart contract upload mechanism.

| ID | Method | Endpoint | Description |
|----|--------|----------|-------------|
| end-4a | `POST` | `/v2/<name>/blobs/uploads/` | Initiate blob upload |
| end-4b | `POST` | `/v2/<name>/blobs/uploads/?digest=<digest>` | Single POST blob upload |
| end-5 | `PATCH` | `/v2/<name>/blobs/uploads/<reference>` | Chunked blob upload |
| end-6 | `PUT` | `/v2/<name>/blobs/uploads/<reference>?digest=<digest>` | Complete blob upload |
| end-7 | `PUT` | `/v2/<name>/manifests/<reference>` | Push manifest |
| end-11 | `POST` | `/v2/<name>/blobs/uploads/?mount=<digest>&from=<other_name>` | Cross-repo blob mount |
| end-13 | `GET` | `/v2/<name>/blobs/uploads/<reference>` | Get upload status |

### Not Implemented - Content Management Endpoints

**Reason**: Deletion is not allowed in blockchains, and is therefore not supported.

| ID | Method | Endpoint | Description |
|----|--------|----------|-------------|
| end-9 | `DELETE` | `/v2/<name>/manifests/<reference>` | Delete manifest or tag |
| end-10 | `DELETE` | `/v2/<name>/blobs/<digest>` | Delete blob |

### Not Implemented - Referrers API

**Reason**: The referrers API (added in distribution-spec 1.1) is for tracking relationships between manifests (e.g., SBOMs, signatures attached to images). Not needed for basic WASM component serving.

| ID | Method | Endpoint | Description |
|----|--------|----------|-------------|
| end-12a | `GET` | `/v2/<name>/referrers/<digest>` | List referrers |
| end-12b | `GET` | `/v2/<name>/referrers/<digest>?artifactType=<type>` | List referrers with filter |

### Not Implemented - Pagination

**Reason**: Pagination by name is unimplementable in a blockchain setting as-is, as there is no guarantee all projects with a name come from the same person (they must be filtered by the signing key).

| ID | Method | Endpoint | Description |
|----|--------|----------|-------------|
| end-8b | `GET` | `/v2/<name>/tags/list?n=<int>&last=<tagname>` | Paginated tag listing |

### URL Mapping

```
oras pull localhost:5000/wasi/http:0.2.0
                        └─┬─┘ └─┬┘ └─┬─┘
                          │    │    └── version (from wasm metadata)
                          │    └─────── package name
                          └──────────── namespace
```

### Spec Compliance Notes

Per the spec:
- `<name>` must match: `[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*(\/[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*)*`
- `<reference>` must be either a digest (`sha256:...`) or a tag (max 128 chars)
- Successful responses SHOULD include `Docker-Content-Digest` header
- 404 responses SHOULD return JSON error body with appropriate error code

---

## 4. OCI Artifact Structures

### Manifest

These come from `image-spec`

```json
{
  "schemaVersion": 2,
  "mediaType": "application/vnd.oci.image.manifest.v1+json",
  "config": {
    "mediaType": "application/vnd.wasm.config.v0+json",
    "digest": "sha256:...",
    "size": 234
  },
  "layers": [{
    "mediaType": "application/wasm",
    "digest": "sha256:...",
    "size": 50000
  }]
}
```

### Config

```json
{
  "created": "2024-01-15T10:30:00Z",
  "architecture": "wasm",
  "os": "wasip2",
  "layerDigests": ["sha256:..."],
  "component": {
    "exports": ["wasi:http/handler@0.2.0"],
    "imports": []
  }
}
```

---

## 5. CLI Design

### Usage

```bash
# Serve a single component
wasm-oci-serve example/adder.wasm
# Listening on http://localhost:5000
# Available: localhost:5000/wasi/http:0.2.0

# Serve multiple components from a directory
wasm-oci-serve ./example/
# Available:
#   localhost:5000/wasi/http:0.2.0
#   localhost:5000/wasi/cli:0.2.0

# Custom port
wasm-oci-serve --port 8080 example/adder.wasm

# Then test with oras
oras pull localhost:5000/wasi/http:0.2.0 --output pulled.wasm
```

### Component Loading Flow

```
1. Read .wasm file(s) from disk
2. For each file:
   a. Validate with wit_component::decode()
   b. Extract name/version from wasm_metadata
   c. Compute wasm_digest = sha256(bytes)
   d. Generate WasmConfig, compute config_digest
   e. Generate Manifest, compute manifest_digest
   f. Store entry in in-memory registry
3. Start HTTP server
4. Handle OCI requests against in-memory registry
```

---

## 6. Implementation Plan

### Crate Structure

```
custom-registry/
├── Cargo.toml
└── src/
    ├── main.rs          # CLI entry point
    ├── registry.rs      # In-memory registry data structures
    ├── oci.rs           # OCI artifact generation (manifest, config)
    └── server.rs        # HTTP server (axum)
```

### Dependencies

```toml
[dependencies]
# Existing crates from workspace
oci-wasm = { path = "../rust-oci-wasm" }  # WasmConfig, media types
wasm-metadata = "..."                      # Metadata extraction

# New dependencies
axum = "0.7"                               # HTTP server
tokio = { version = "1", features = ["full"] }
sha2 = "0.10"                              # Digest computation
serde_json = "1"                           # JSON serialization
clap = { version = "4", features = ["derive"] }  # CLI args
```

### Implementation Steps

1. **Registry module**: Define `ComponentEntry` and `Registry` structs
2. **OCI module**: Functions to generate manifest/config JSON deterministically
3. **Loader**: Read .wasm files, parse metadata, populate registry
4. **Server**: Axum routes for OCI endpoints
5. **CLI**: Argument parsing, wire it together

---

## 7. Key Files to Reference

From existing codebase:

| File | Use |
|------|-----|
| `rust-oci-wasm/src/config.rs` | `WasmConfig` struct, media types |
| `rust-oci-wasm/src/component.rs` | Component interface extraction |
| `rust-oci-client/src/manifest.rs` | `OciImageManifest` structure |
| `wasm-pkg-client/src/oci/publisher.rs` | Pattern for metadata extraction |

---

## 8. Verification

Test the implementation with `oras`:

```bash
# Start the server
cargo run -- ./test-component.wasm

# In another terminal, pull the component
oras pull localhost:5000/namespace/name:0.1.0 --output test.wasm

# Verify the pulled file matches
sha256sum test-component.wasm test.wasm
# Should match

# Inspect manifest
oras manifest fetch localhost:5000/namespace/name:0.1.0

# List available
curl http://localhost:5000/v2/namespace/name/tags/list
```

---

## 9. TODO List

### 9.1 Route Generation from Spec

**Problem**: In `create_router`, we hard-code route strings manually. This is error-prone and could lead to typos or missing endpoints.

**Potential solutions**:
- [ ] Generate routes from an OpenAPI spec
- [ ] Use constants derived from the distribution-spec
- [ ] Create a macro that validates routes against the spec

### 9.2 WASM-Specific OCI Extensions

**Problem**: The OCI Distribution Spec is generic. We implemented the standard pull endpoints, but unclear if:
- Some endpoints are irrelevant for WASM components specifically
- There are WASM-specific extensions we should support (e.g., from `warg` or other WASM registries)

**Tasks**:
- [ ] Research how `warg` (WebAssembly Registry) extends OCI (or does it even extend OCI? `warg` may be a deprecated tool, replaced by the OCI Registry option)
- [ ] Check if `wasm-pkg-tools` uses any non-standard endpoints
- [ ] Document which endpoints are WASM-relevant vs container-image-specific

### 9.3 Frontend Component Association

**Problem**: Smart contracts (WASM components) need to specify where to find an associated frontend (also a WASM component in an OCI registry).

**Options to investigate**:
- [ ] Use the `target` field in `config.blob` - what is this field actually for?
- [ ] Use component metadata (via `wasm-metadata`)
- [ ] Use OCI manifest annotations
- [ ] Use the referrers API to link frontend as an "attached artifact"

**Questions**:
- What is the `target` field in `WasmConfig.component`? Is it meant for this use case?
- Should the association be bidirectional (contract → frontend, frontend → contract)?

### 9.4 Component Signing & Identity

**Problem**: On-chain, we cannot "reserve" names like traditional registries do. Two different entities could upload components with the same `namespace:name`. Users need a way to verify components come from the same author.

**Key insight**: The only reliable way to know two components come from the same entity is to verify they're signed by the same key.

**Implications**:
- [ ] Research how OCI handles signing (Notary v2? Sigstore/cosign?)
- [ ] Design signing flow for component authors
- [ ] Document that users MUST verify signatures, not trust names
- [ ] Consider: Should `namespace` be derived from the signing key (like `did:key:...`)?

**Pagination considerations**:
- `end-8b` (paginated tags) is problematic: can't paginate by name since names aren't unique per-entity
- Possible solution: Require signing key filter for pagination (e.g., `/v2/<name>/tags/list?signer=<pubkey>`). We need to investigate if such a filter is supported by OCI registries already (and if so, make it mandatory for our implementation).
- Alternative: Leave pagination unimplemented and document why

### 9.5 Documentation

- [ ] Add README.md with usage instructions
- [ ] Document the trust model (signatures required, names not trustworthy)
- [ ] Create examples for common workflows

---

## 10. Out of Scope (no plans to make this production)

If building a real server from this pattern:
- Replace in-memory HashMap with PostgreSQL + Redis cache
- Stream WASM bytes from object storage (S3/GCS) instead of memory
- Add authentication (Bearer tokens)
- Add rate limiting
- Pre-compute and store digests at upload time for consistency
