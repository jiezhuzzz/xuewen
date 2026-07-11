# Design: Container image + Kubernetes deploy (nix2container)

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-09
**Status:** Approved (design phase)

## 1. Purpose

Xuewen currently runs only from a dev checkout. To deploy it on a Kubernetes
cluster we need a container image and the manifests around it. The flake
already pins the toolchain; this feature makes it the single source of truth
for deployable artifacts: `nix run .#push` builds the frontend, compiles the
binary (running the full test suite in the sandbox), assembles a layered OCI
image with nix2container, and pushes it to GitHub Container Registry — no
Dockerfile, no Docker daemon.

## 2. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Image builder | **nix2container** (flake input `github:nlewo/nix2container`), chosen over a Dockerfile (duplicates flake pinning) and `dockerTools` (tarballs in store, slow pushes) |
| Registry / name | **`ghcr.io/jiezhuzzz/xuewen`**, tag = git short rev (fallback `dev` when dirty) + `latest` |
| Architecture | **x86_64-linux only** |
| Container process | **`serve` only** (one process per container; inbox watching = optional second container sharing the volume, documented not shipped) |
| Deploy target | **Kubernetes**: example manifests in `deploy/k8s/` (Deployment+Service+PVC+ConfigMap, Qdrant, Secret template), applied with `kubectl apply -f` |
| State | Single volume **`/data`** (inbox, library, library.db, search-index) |
| Config | Baked default at `/etc/xuewen/xuewen.toml`; K8s ConfigMap mounts over it |
| User | Non-root **1000:1000** baked into the image; `fsGroup: 1000` in the manifest |
| Health | Existing **`GET /api/stats`** as liveness + readiness probe (no new code) |

**Out of scope (YAGNI):** Ingress manifest (cluster-specific; README notes
the pod has no auth and must sit behind ingress auth), Helm/kustomize,
multi-arch manifest lists, automated image smoke tests (documented manual
steps instead), CI wiring, `watch` sidecar manifest.

## 3. Flake changes (`flake.nix`)

New input:

```nix
inputs.nix2container = {
  url = "github:nlewo/nix2container";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

`devShells` stay as they are. New outputs (packages built for all existing
systems where they make sense; `image`/apps only for `x86_64-linux`):

### 3.1 `packages.frontend`

`pkgs.buildNpmPackage`:
- `src = ./frontend`, `npmDepsHash` pinned (computed at implementation time
  via a first build with `lib.fakeHash`).
- `checkPhase`: `npm test` (vitest; jsdom, no network — sandbox-safe).
- `installPhase`: copy `dist/` to `$out` (the directory itself is the output).

### 3.2 `packages.xuewen` (= `packages.default`)

`pkgs.rustPlatform.buildRustPackage`:
- `src = ./.` (filtered to exclude `frontend/` sources, `docs/`, `deploy/`
  so backend rebuilds don't trigger on manifest edits; `frontend/dist` comes
  from the package below).
- `cargoLock.lockFile = ./Cargo.lock`.
- `preBuild`: `mkdir -p frontend/dist && cp -r ${frontend}/* frontend/dist/`
  — rust-embed then embeds the real UI (`build.rs`'s placeholder only fires
  when dist is absent; unchanged).
- `doCheck = true`, `nativeCheckInputs = [ pkgs.poppler_utils ]` — the full
  Rust suite runs in the sandbox (wiremock binds loopback, which Nix build
  sandboxes provide; no test touches the real network).

### 3.3 `packages.image`

`nix2container.packages.x86_64-linux.nix2container.buildImage`:

- `name = "ghcr.io/jiezhuzzz/xuewen"`,
  `tag = self.shortRev or "dev"`.
- **Layers** (explicit, for pull-cache efficiency):
  1. deps layer: `pkgs.poppler_utils`, `pkgs.cacert` (rarely changes),
  2. app layer: the `xuewen` package + the baked config (below).
- Baked config: `/etc/xuewen/xuewen.toml` written with `pkgs.writeTextDir`
  (wrapped so the file lands at that path):

```toml
inbox_dir     = "/data/inbox"
library_root  = "/data/library"
database_url  = "sqlite:/data/library.db"

[search]
index_dir         = "/data/search-index"
qdrant_url        = "http://xuewen-qdrant:6333"
qdrant_collection = "xuewen"

[search.embedding]
base_url    = "https://api.openai.com/v1"
model       = "text-embedding-3-small"
dims        = 1536
api_key_env = "OPENAI_API_KEY"
```

- **OCI config:**
  - `Entrypoint = [ "…/bin/xuewen" "--config" "/etc/xuewen/xuewen.toml"
    "serve" "--host" "0.0.0.0" "--port" "8080" "--allow-remote" ]`
    (0.0.0.0 is mandatory in a container; `--allow-remote` acknowledges the
    no-auth UI — network exposure is the cluster's responsibility).
  - `Env`: `PATH=${poppler_utils}/bin` — the only external binary the app
    invokes is `pdftotext`; the exec-form Entrypoint needs no shell —
    `SSL_CERT_FILE=${cacert}/etc/ssl/certs/ca-bundle.crt`, `RUST_LOG=info`.
  - `User = "1000:1000"`, `WorkingDir = "/data"`, `ExposedPorts = {"8080/tcp"}`.
- The app `create_dir_all`s everything it needs under `/data`; an empty
  volume works. The `[search.embedding]` section is harmless without a key:
  semantic search reports unavailable, keyword search works.

### 3.4 Apps

- `apps.push`: script (writeShellApplication) that calls the image's
  skopeo copy twice: to `ghcr.io/jiezhuzzz/xuewen:<tag>` and `:latest`.
  Precondition documented: `skopeo login ghcr.io`.
- `apps.load`: nix2container's `copyToDockerDaemon` for local testing.
- `checks`: alias the three packages so `nix flake check` builds (and
  thereby tests) everything.

## 4. Kubernetes manifests (`deploy/k8s/`)

Plain YAML, order-independent, `kubectl apply -f deploy/k8s/`:

| File | Contents |
|---|---|
| `xuewen.yaml` | Deployment: `replicas: 1`, `strategy: Recreate` (SQLite is single-writer; old pod must release the PVC before the new one starts), image `ghcr.io/jiezhuzzz/xuewen:latest`, `securityContext` (runAsNonRoot, runAsUser/Group 1000, fsGroup 1000), volumeMounts `/data` (PVC `xuewen-data`, 10 Gi) + `/etc/xuewen` (ConfigMap), `envFrom` optional Secret `xuewen-secrets`, liveness+readiness `httpGet /api/stats :8080`. Service ClusterIP :8080. PVC. |
| `xuewen-config.yaml` | ConfigMap `xuewen-config` with the full `xuewen.toml` (same content as baked; edit cluster-side to change model/Qdrant URL) |
| `qdrant.yaml` | Deployment `qdrant/qdrant` pinned to an exact release tag (the current stable is looked up once at implementation time — never `latest`), PVC 5 Gi mounted at `/qdrant/storage`, Service `xuewen-qdrant` :6333 |
| `secret.example.yaml` | Template: `stringData: OPENAI_API_KEY: ""` + comment saying to fill and rename; **not** required for keyword-only operation |
| `README.md` | ghcr login, `nix run .#push`, apply order, secret creation, `kubectl apply --dry-run=client` validation, ingress/auth warning, note on running a `watch` sidecar if ever wanted |

## 5. Error handling / operational notes

- **No auth on the pod:** README states the Service must only be exposed
  through an authenticating ingress or private network.
- **Scaling:** replicas must stay 1 (SQLite + Tantivy single-writer);
  Recreate strategy enforces safe rollouts.
- **Qdrant down / key missing:** existing degradation applies (keyword-only,
  reason in UI); probes stay green because `/api/stats` doesn't depend on
  the search tiers.
- **Image provenance:** tag = git rev; `latest` is a convenience alias
  pushed atomically after the rev tag.

## 6. Testing

- `nix build .#xuewen` — full Rust suite (238 tests) in the sandbox; build
  fails on any test failure. `nix build .#frontend` — vitest suite.
- `nix flake check` — builds all packages.
- Manual image smoke test (documented in `deploy/k8s/README.md`):
  `nix run .#load`, `docker run --rm -p 8080:8080 -v xuewen-data:/data
  ghcr.io/jiezhuzzz/xuewen:<tag>`, `curl localhost:8080/api/stats`, confirm
  the UI loads and `pdftotext` works by importing a PDF via the UI.
- Manifests: `kubectl apply --dry-run=client -f deploy/k8s/` documented.
