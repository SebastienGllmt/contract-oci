# WASM Component OCI Registry

## Overview

A **Rust CLI tool** that demonstrates serving WASM components via OCI registry protocol. The tool will:
1. Take `.wasm` component files as input
2. Generate OCI artifacts (manifest, config) on-demand
3. Serve via minimal HTTP server for testing with `oras pull`

This leverages [v1.1](https://opencontainers.org/posts/blog/2024-03-13-image-and-distribution-1-1/) of the OCI Image and Distribution specifications. Notably, it leverages the [OCI Artifact](https://oras.land/docs/concepts/artifact/) base to support the [Wasm OCI Artifact Layout](https://tag-runtime.cncf.io/wgs/wasm/deliverables/wasm-oci-artifact/).

It is a content-addressed registry, meaning it's primarily designed for immutable storage and retrieval of artifacts (and not named repositories).

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

### TBD - Referrers API

**Reason**: The referrers API (added in distribution-spec 1.1) is for tracking relationships between manifests (e.g., SBOMs, signatures attached to images).

It may be used to aggregate components by the same author, or to track frontends associated to dApps.

| ID | Method | Endpoint | Description |
|----|--------|----------|-------------|
| end-12a | `GET` | `/v2/<name>/referrers/<digest>` | List referrers |
| end-12b | `GET` | `/v2/<name>/referrers/<digest>?artifactType=<type>` | List referrers with filter |

### Partially Implemented - Tag listing

**Reason**: Tags are unimplementable in a blockchain setting as-is, as there is no guarantee all projects with a name come from the same person (they must be filtered by the signing key). Therefore, the only "tag" in the version number sense is `1.0.0`, adn there will never be multiple tags per project.

| ID | Method | Endpoint | Description |
|----|--------|----------|-------------|
| end-8b | `GET` | `/v2/<name>/tags/list?n=<int>&last=<tagname>` | Paginated tag listing |
| end-8a | `GET` | `/v2/<name>/tags/list` | List tags |

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

**Solution implemented**: Created `src/routes.rs` module with:
- [x] Route constants derived from the distribution-spec (`endpoints` module)
- [x] Validation patterns from the spec (`patterns` module)
- [x] Error code constants (`error_codes` module)
- [x] Header constants (`headers` module)
- [x] Tests to validate routes match the spec format
- [x] Updated `server.rs` to use these constants

### 9.2 WASM-Specific OCI Extensions

**Problem**: The OCI Distribution Spec is generic. We implemented the standard pull endpoints, but unclear if:
- Some endpoints are irrelevant for WASM components specifically
- There are WASM-specific extensions we should support (note: this is NOT related to `warg), but rather in relation to [Wasm OCI Artifact Layout](https://tag-runtime.cncf.io/wgs/wasm/deliverables/wasm-oci-artifact/)

**Tasks**:
- [ ] Check if `wasm-pkg-tools` uses any non-standard endpoints
- [ ] Document which endpoints are WASM-relevant vs container-image-specific
- [ ] Compare our implementation to [Wasm OCI Artifact Layout](https://tag-runtime.cncf.io/wgs/wasm/deliverables/wasm-oci-artifact/)

### 9.3 Frontend Component Association

**Problem**: Smart contracts (WASM components) need to specify where to find an associated frontend (also a WASM component in an OCI registry).

**Options investigated**:

#### Option A: The `target` field in WasmConfig.component

- [x] **Investigated** - NOT suitable for this use case

**Findings**: The `target` field exists in the `Component` struct (in `rust-oci-wasm/src/component.rs`):
```rust
pub struct Component {
    pub exports: Vec<String>,
    pub imports: Vec<String>,
    pub target: Option<String>,  // "optional metadata for indexing"
}
```

The field is documented as "optional metadata for indexing. Implementations MAY use this information to fetch other data to inspect the specified world." It's intended for referencing the WIT world/interface the component targets, not for frontend association. Currently always set to `None` in all code paths.

**Verdict**: ❌ Not designed for frontend linking. Should not be repurposed.

---

#### Option B: Component metadata via wasm-metadata

- [x] **Investigated** - Possible but not ideal

**Findings**: The `wasm-metadata` crate (v0.240) supports these standard fields:
- `name`, `version`, `authors`, `description`, `licenses`, `source`, `homepage`, `revision`, `processed_by`

These are embedded directly in the WASM binary via `AddMetadata::to_wasm()`.

**Advantages**:
- Metadata travels with the WASM binary itself
- Well-established tooling (`wkg wit build`, `wasm-metadata` crate)

**Disadvantages**:
- No custom key-value support in the standard API
- Would require modifying the WASM binary for each update
- Frontend references would be "baked in" at build time

**Verdict**: ⚠️ Possible via `source` or `homepage` fields as a workaround, but not a clean solution.

---

#### Option C: OCI Manifest Annotations ✅ RECOMMENDED (Simple)

- [x] **Investigated** - Well-suited for simple association

**Findings**: OCI manifests support arbitrary key-value annotations. The codebase already uses them extensively:
```rust
// In wasm-pkg-client/src/oci/publisher.rs
annotations.insert("org.opencontainers.image.version".to_string(), version);
annotations.insert("org.opencontainers.image.description".to_string(), desc);
```

**Implementation approach**:
```rust
// Define custom annotation keys (use reverse domain notation)
pub const VIBE_FRONTEND_URL: &str = "io.vibe.wasm.frontend.url";
pub const VIBE_FRONTEND_DIGEST: &str = "io.vibe.wasm.frontend.digest";

// Add to manifest annotations
annotations.insert(
    VIBE_FRONTEND_URL.to_string(),
    "registry.example.com/org/frontend:1.0.0".to_string(),
);
annotations.insert(
    VIBE_FRONTEND_DIGEST.to_string(),
    "sha256:abc123...".to_string(),
);
```

**Resulting manifest**:
```json
{
  "schemaVersion": 2,
  "mediaType": "application/vnd.oci.image.manifest.v1+json",
  "annotations": {
    "io.vibe.wasm.frontend.url": "registry.example.com/org/frontend:1.0.0",
    "io.vibe.wasm.frontend.digest": "sha256:abc123..."
  },
  ...
}
```

**Advantages**:
- Simple to implement (already have annotation support)
- Standard OCI pattern (follows spec conventions)
- No registry changes needed
- Works with existing tooling (`oras manifest fetch`)

**Disadvantages**:
- One-directional only (contract → frontend)
- No querying capability (can't ask "what frontends exist for this contract?")

**Verdict**: ✅ Best option for simple, immediate implementation.

---

#### Option D: OCI Referrers API ✅ RECOMMENDED (Advanced)

- [x] **Investigated** - Best for bidirectional relationships

**Findings**: The Referrers API (distribution-spec 1.1) allows manifests to declare a `subject` field pointing to another manifest. This creates a queryable relationship graph.

**How it works**:

1. **Smart Contract Manifest** pushed first → gets digest `sha256:contract123...`

2. **Frontend Manifest** pushed with `subject` pointing to contract:
```json
{
  "schemaVersion": 2,
  "mediaType": "application/vnd.oci.image.manifest.v1+json",
  "artifactType": "application/vnd.vibe.frontend.v1",
  "subject": {
    "mediaType": "application/vnd.oci.image.manifest.v1+json",
    "digest": "sha256:contract123...",
    "size": 1234
  },
  "annotations": {
    "io.vibe.frontend.framework": "React",
    "io.vibe.frontend.compatible-versions": ">=1.0.0 <2.0.0"
  },
  "layers": [...]
}
```

3. **Query frontends for a contract**:
```
GET /v2/org/project/referrers/sha256:contract123?artifactType=application/vnd.vibe.frontend.v1
```

Returns an image index listing all frontends that reference this contract.

**Advantages**:
- Bidirectional: Find contract→frontends OR inspect frontend→contract
- Queryable at registry level
- Supports filtering by `artifactType`
- Rich metadata via annotations
- OCI-standardized (not a custom extension)
- Multiple frontends can reference one contract
- Version compatibility tracking via annotations

**Disadvantages**:
- Requires implementing `end-12a` endpoint in our registry (currently marked "Not Implemented")
- More complex than simple annotations
- Requires registry support (fallback to tag-based schema if 404)

**Verdict**: ✅ Best option for production-grade relationship tracking.

---

### Additional Complexities Discovered

The initial analysis assumed a simpler model. Real-world requirements introduce these complications:

#### Complexity 1: Multiple Independent Frontends

Different developers may create different frontends for the same smart contract (e.g., a "power user" UI vs a "simple" UI, or competing implementations). This means:

- **Contract → Frontend** association cannot be 1:1
- The contract author may not control (or even know about) all frontends
- Users need a way to discover available frontends for a contract

**Implication**: The association direction matters. Frontends should point TO contracts, not vice versa.

#### Complexity 2: Canonical Frontend with Updateability

A dApp author may want to designate a "canonical" or "official" frontend, but:

- Frontends need to be updatable without redeploying the contract (which may be immutable on-chain)
- This requires a **mutable pointer** that can be updated by an authorized party
- The pointer must be **signed** to prove the contract author endorses it

**Implication**: Requires signing infrastructure (see 9.4). The "canonical frontend" designation is essentially a signed statement by the contract author, not baked into the contract itself.

#### Complexity 3: Asymmetric Registry Architecture

**Critical insight**: Frontends and contracts live in fundamentally different registries:

| | Smart Contracts | Frontends |
|---|---|---|
| **Registry** | Blockchain nodes (our custom registry) | Traditional OCI registries (ghcr.io, etc.) |
| **URL stability** | ❌ No canonical URL - every fullnode is its own registry | ✅ Stable URLs (ghcr.io/org/frontend:1.0.0) |
| **Addressing** | Must be node-independent (e.g., by content hash or on-chain identifier) | Standard OCI references |
| **Push capability** | ❌ Read-only (content comes from blockchain) | ✅ Normal push/pull |

This asymmetry breaks several assumptions:

1. **Referrers API won't work** for frontend→contract links:
   - Referrers API requires both artifacts in the SAME registry
   - Frontend on ghcr.io cannot use `subject` to reference a contract on a blockchain node
   - Even if it could, there's no stable URL to put in the `subject.digest` reference

2. **Contract→Frontend annotations work** (one direction):
   - Contract manifest can include `io.vibe.wasm.frontend.url: "ghcr.io/org/frontend:1.0.0"`
   - But this is baked in at contract publish time and not updatable

3. **Frontend→Contract references need a different approach**:
   - Cannot use OCI `subject` field (cross-registry)
   - Must use **annotations with a custom addressing scheme**

---

### Revised Architecture

Given these complexities, the association model needs to be:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        FRONTEND (on ghcr.io)                            │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ Manifest annotations:                                            │   │
│  │   io.vibe.contract.chain: "vibe-mainnet"                        │   │
│  │   io.vibe.contract.address: "0x1234..."                         │   │
│  │   io.vibe.contract.digest: "sha256:abc..."  (content hash)      │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ References (node-independent addressing)
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                   SMART CONTRACT (on any blockchain node)               │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ On-chain data / component metadata:                              │   │
│  │   - Contract code (immutable)                                    │   │
│  │   - Content digest (for verification)                            │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ Canonical Frontend Pointer (SEPARATE, SIGNED, MUTABLE):          │   │
│  │   Stored: on-chain or in a signed attestation                    │   │
│  │   io.vibe.canonical-frontend.url: "ghcr.io/author/ui:2.0.0"     │   │
│  │   io.vibe.canonical-frontend.digest: "sha256:def456..."         │   │
│  │   Signed by: <contract-author-pubkey>                            │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

### Revised Recommendation

#### Direction 1: Frontend → Contract (Discovery: "What contract does this frontend use?")

**Use: OCI Manifest Annotations with node-independent addressing**

```json
{
  "annotations": {
    "io.vibe.contract.chain": "vibe-mainnet",
    "io.vibe.contract.address": "0x1234abcd...",
    "io.vibe.contract.digest": "sha256:abc123..."
  }
}
```

- Frontend authors add these annotations when publishing to ghcr.io
- Any OCI client can read them (`oras manifest fetch`)
- Node-independent: client resolves the contract through any available node

#### Direction 2: Contract → Canonical Frontend (Discovery: "What's the official frontend?")

**Use: Signed attestation (requires 9.4 signing infrastructure)**

This CANNOT be in the contract's OCI manifest because:
- Contract manifests are generated from immutable on-chain data
- Frontend URL needs to be updatable

Options (pending 9.4 investigation):
1. **On-chain storage**: Contract has an updateable "frontend pointer" field
2. **Signed attestation**: A separate signed document (e.g., using Sigstore/cosign) that declares the canonical frontend
3. **DNS-like resolution**: `_frontend.contractname.vibe` TXT record (decentralized DNS?)

**Recommendation**: Defer to 9.4. The canonical frontend pointer is fundamentally a signing/identity problem, not an OCI problem.

#### Direction 3: Discover all frontends for a contract

**Challenge**: No registry-level query is possible across ghcr.io

Options:
1. **Off-chain index**: A service that crawls OCI registries for `io.vibe.contract.*` annotations
2. **On-chain registration**: Frontends register themselves on-chain (requires tx)
3. **Social discovery**: Frontend authors announce via other channels

**Recommendation**: Out of scope for the OCI registry itself. This is an indexing/discovery service problem.

---

### Summary of What Works

| Use Case | Solution | Status |
|----------|----------|--------|
| Frontend declares which contract it's for | Manifest annotations with chain/address/digest | ✅ Works now |
| Contract declares canonical frontend | Signed attestation (needs signing infra) | ⏳ Blocked on 9.4 |
| Discover all frontends for a contract | External indexing service | 🔮 Future work |
| Verify frontend is "official" | Check signature matches contract author | ⏳ Blocked on 9.4 |

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
