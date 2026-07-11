# Container Image + K8s Deploy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `nix run .#push` builds frontend + backend (tests run in-sandbox), assembles a layered OCI image with nix2container, and pushes `ghcr.io/jiezhuzzz/xuewen:<rev>` + `:latest`; `deploy/k8s/` holds ready-to-apply manifests.

**Architecture:** The flake becomes the single source of truth for artifacts: `buildNpmPackage` (frontend, vitest in checkPhase) → `buildRustPackage` (embeds frontend dist, full cargo test in sandbox) → `nix2container.buildImage` (deps layer: poppler+cacert; app layer: binary + baked `/etc/xuewen/xuewen.toml`). K8s runs it as a single-replica Recreate Deployment with one `/data` PVC, a ConfigMap shadowing the baked config, and a Qdrant sidecar Deployment.

**Tech Stack:** Nix flakes, nix2container (`github:nlewo/nix2container`), buildNpmPackage, rustPlatform.buildRustPackage, skopeo, Kubernetes YAML (no Helm/kustomize).

**Spec:** `docs/superpowers/specs/2026-07-09-container-design.md`

## Global Constraints

- Image name/tag: `ghcr.io/jiezhuzzz/xuewen`, `tag = self.shortRev or "dev"`; push also copies to `:latest`.
- x86_64-linux only for `image` and apps; `frontend`/`xuewen` packages build for the flake's four existing systems.
- Container: `serve` only, `--host 0.0.0.0 --port 8080 --allow-remote`, non-root `User = "1000:1000"`, `WorkingDir = /data`, config at `/etc/xuewen/xuewen.toml`, `PATH` containing only `${poppler_utils}/bin`, `SSL_CERT_FILE` from `pkgs.cacert`, `RUST_LOG=info`.
- All state under `/data`: `inbox`, `library`, `library.db`, `search-index`. Qdrant URL in-cluster: `http://xuewen-qdrant:6333`.
- K8s: `replicas: 1` + `strategy: Recreate` (SQLite/Tantivy single-writer). Probes: `httpGet /api/stats :8080`. Secret env `OPENAI_API_KEY` optional.
- No new source-code changes to `src/` or `frontend/src` — this feature is flake + YAML + docs only.
- Nix commands run from the repo/worktree root. First builds download toolchains — allow several minutes; do not kill them early.
- nix2container API-drift note: `buildImage`/`buildLayer`/`copyTo` names are from the project README at the time of writing; if the pinned rev differs, keep the spec's behavior and adjust attribute names per `nix flake show github:nlewo/nix2container` — do not change the image's OCI config values.
- Commit style: conventional commits (`build(nix): …`, `feat(deploy): …`). Do NOT commit `docs/superpowers/**`.

---

### Task 1: Flake input + `packages.frontend`

**Files:**
- Modify: `flake.nix` (full new content below — replaces the file)
- Modify: `.gitignore` (add nix `result` symlinks)

**Interfaces:**
- Produces: flake outputs `packages.<system>.frontend` — a derivation whose `$out` IS the built `dist/` directory (i.e. `$out/index.html` exists). Task 2 consumes it as `frontend`.
- Produces: `nix2container` flake input for Task 3.

- [ ] **Step 1: Replace `flake.nix`** with:

```nix
{
  description = "Xuewen — self-hosted reference manager";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nix2container = {
      url = "github:nlewo/nix2container";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs = { self, nixpkgs, nix2container }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo rustc rustfmt clippy rust-analyzer
            poppler-utils   # provides `pdftotext`
            sqlite
            nodejs          # frontend build (npm)
            pkg-config
          ];
        };
      });

      packages = forAll (pkgs: rec {
        frontend = pkgs.buildNpmPackage {
          pname = "xuewen-frontend";
          version = "0.1.0";
          src = ./frontend;
          npmDepsHash = pkgs.lib.fakeHash; # replaced in Step 3
          # `npm run build` is the default buildPhase; run vitest before install.
          doCheck = true;
          checkPhase = ''
            runHook preCheck
            npm test
            runHook postCheck
          '';
          installPhase = ''
            runHook preInstall
            cp -r dist $out
            runHook postInstall
          '';
        };
        default = frontend; # replaced by `xuewen` in Task 2
      });
    };
}
```

- [ ] **Step 2: Add nix build artifacts to `.gitignore`** (append):

```
result
result-*
```

- [ ] **Step 3: Compute the real `npmDepsHash`.**

Run: `nix build .#frontend 2>&1 | tail -5`
Expected: FAIL with a hash mismatch: `specified: sha256-AAAA…  got: sha256-<real>`
Copy the `got:` value into `npmDepsHash`, replacing `pkgs.lib.fakeHash`.

- [ ] **Step 4: Build to verify (runs vitest in the sandbox)**

Run: `nix build .#frontend && ls result/index.html`
Expected: build succeeds (check log shows the 47 vitest tests passing: `nix log .#frontend | grep "Tests "` → `47 passed`); `result/index.html` exists.

- [ ] **Step 5: Commit**

```bash
git add flake.nix flake.lock .gitignore
git commit -m "build(nix): frontend package via buildNpmPackage + nix2container input"
```

---

### Task 2: `packages.xuewen` (backend with embedded UI, tests in sandbox)

**Files:**
- Modify: `flake.nix` (inside the `packages = forAll (pkgs: rec { … })` set)

**Interfaces:**
- Consumes: `frontend` (Task 1) — `$out` is the dist directory.
- Produces: `packages.<system>.xuewen` (= `packages.default`) with `$out/bin/xuewen`. Task 3 consumes it as `xuewen`.

- [ ] **Step 1: Add the package** inside the `rec { … }` after `frontend`, and change `default`:

```nix
        xuewen = pkgs.rustPlatform.buildRustPackage {
          pname = "xuewen";
          version = "0.1.0";
          # Exclude the frontend sources (dist comes from the `frontend`
          # package), docs, and deploy manifests so editing them never
          # rebuilds the backend.
          src = pkgs.lib.cleanSourceWith {
            src = self;
            filter = path: _type:
              let rel = pkgs.lib.removePrefix (toString self + "/") (toString path);
              in !(pkgs.lib.hasPrefix "frontend" rel
                || pkgs.lib.hasPrefix "docs" rel
                || pkgs.lib.hasPrefix "deploy" rel);
          };
          cargoLock.lockFile = ./Cargo.lock;
          # rust-embed reads frontend/dist at compile time; build.rs would
          # write a placeholder if this were missing.
          preBuild = ''
            mkdir -p frontend/dist
            cp -r ${frontend}/. frontend/dist/
          '';
          # The full test suite runs in the sandbox: wiremock binds loopback
          # (available in Nix builds) and pdftotext comes from poppler.
          nativeCheckInputs = [ pkgs.poppler_utils ];
        };
        default = xuewen;
```

(Remove the temporary `default = frontend;` line from Task 1.)

- [ ] **Step 2: Build and verify tests ran**

Run: `nix build .#xuewen`
Expected: succeeds after compiling (several minutes cold). Then:
Run: `nix log .#xuewen | grep -E "^test result" | head -12`
Expected: 10 `test result: ok` lines (177 lib tests + integration suites), 0 failed.
Run: `./result/bin/xuewen --help | head -3`
Expected: usage text with the `serve`, `search`, `index` subcommands available.

- [ ] **Step 3: Verify the real UI is embedded (not the placeholder)**

Run: `strings result/bin/xuewen | grep -c "The API is running"`
Expected: `0` (the build.rs placeholder text must NOT be present; the real dist was embedded).

- [ ] **Step 4: Commit**

```bash
git add flake.nix
git commit -m "build(nix): xuewen package embedding the built frontend, tests in sandbox"
```

---

### Task 3: nix2container image + push/load apps + checks

**Files:**
- Modify: `flake.nix` (image/config/apps/checks; x86_64-linux only for image+apps)

**Interfaces:**
- Consumes: `xuewen`, `frontend` packages; `nix2container` input.
- Produces: `packages.x86_64-linux.image` (nix2container image derivation with `copyTo`/`copyToDockerDaemon` passthru), `apps.x86_64-linux.push`, `apps.x86_64-linux.load`, `checks`.

- [ ] **Step 1: Add image + apps + checks.** Restructure the `outputs` `let` to expose an x86_64 helper, then add the new outputs. Final shape of the non-devShell outputs:

```nix
      packages = forAll (pkgs: rec {
        frontend = …;  # unchanged from Task 1
        xuewen = …;    # unchanged from Task 2
        default = xuewen;
      }) // {
        # Image + registry wiring are Linux/amd64-only.
        x86_64-linux = let
          pkgs = nixpkgs.legacyPackages.x86_64-linux;
          base = self.packages.x86_64-linux; # frontend/xuewen from forAll above
          n2c = nix2container.packages.x86_64-linux.nix2container;
          tag = self.shortRev or "dev";
          # Baked default config: single /data volume, in-cluster Qdrant,
          # key from the environment. A mounted ConfigMap shadows this file.
          configFile = pkgs.writeTextFile {
            name = "xuewen-default-config";
            destination = "/etc/xuewen/xuewen.toml";
            text = ''
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
            '';
          };
          image = n2c.buildImage {
            name = "ghcr.io/jiezhuzzz/xuewen";
            inherit tag;
            # Layer 1: runtime deps that rarely change (cheap re-pulls on
            # app updates). The app closure lands in the final layer via
            # the Entrypoint reference.
            layers = [
              (n2c.buildLayer { deps = [ pkgs.poppler_utils pkgs.cacert ]; })
            ];
            copyToRoot = [ configFile ];
            config = {
              Entrypoint = [
                "${base.xuewen}/bin/xuewen"
                "--config" "/etc/xuewen/xuewen.toml"
                "serve" "--host" "0.0.0.0" "--port" "8080" "--allow-remote"
              ];
              Env = [
                "PATH=${pkgs.poppler_utils}/bin"
                "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
                "RUST_LOG=info"
              ];
              User = "1000:1000";
              WorkingDir = "/data";
              ExposedPorts = { "8080/tcp" = { }; };
            };
          };
        in base // { inherit image; };
      };

      apps.x86_64-linux = let
        pkgs = nixpkgs.legacyPackages.x86_64-linux;
        tag = self.shortRev or "dev";
        push = pkgs.writeShellScriptBin "xuewen-push" ''
          set -euo pipefail
          # Prereq: skopeo login ghcr.io (documented in deploy/k8s/README.md)
          nix run .#image.copyTo -- docker://ghcr.io/jiezhuzzz/xuewen:${tag}
          nix run .#image.copyTo -- docker://ghcr.io/jiezhuzzz/xuewen:latest
        '';
        load = pkgs.writeShellScriptBin "xuewen-load" ''
          set -euo pipefail
          nix run .#image.copyToDockerDaemon
        '';
      in {
        push = { type = "app"; program = "${push}/bin/xuewen-push"; };
        load = { type = "app"; program = "${load}/bin/xuewen-load"; };
      };

      checks = forAll (pkgs: {
        frontend = self.packages.${pkgs.system}.frontend;
        xuewen = self.packages.${pkgs.system}.xuewen;
      });
```

Note the `// { x86_64-linux = … }` merge overrides the `forAll` entry for that system — `base // { inherit image; }` keeps `frontend`/`xuewen`/`default` and adds `image`. Keep the existing `devShells` block untouched.

- [ ] **Step 2: Evaluate + build the image description**

Run: `nix flake show 2>/dev/null | grep -A2 image` — expect `image` under `packages.x86_64-linux`.
Run: `nix build .#image && cat result | head -c 200`
Expected: succeeds quickly (image is a JSON description, layers stay in the store); output starts with JSON.

- [ ] **Step 3: Inspect the OCI config end to end**

Run:
```bash
nix run .#image.copyTo -- oci:/tmp/xuewen-oci-check:check
nix shell nixpkgs#skopeo -c skopeo inspect --config oci:/tmp/xuewen-oci-check:check \
  | nix shell nixpkgs#jq -c jq '{User: .config.User, Entrypoint: .config.Entrypoint, Env: .config.Env, Ports: .config.ExposedPorts}'
rm -rf /tmp/xuewen-oci-check
```
Expected: `User == "1000:1000"`; Entrypoint ends with `serve --host 0.0.0.0 --port 8080 --allow-remote`; Env contains the three variables; port `8080/tcp` exposed.

- [ ] **Step 4: `nix flake check`**

Run: `nix flake check`
Expected: builds frontend + xuewen for the host system (cached from Tasks 1-2), no errors.

- [ ] **Step 5: Commit**

```bash
git add flake.nix
git commit -m "build(nix): nix2container image with push/load apps"
```

---

### Task 4: Kubernetes manifests + deploy README

**Files:**
- Create: `deploy/k8s/xuewen.yaml`
- Create: `deploy/k8s/xuewen-config.yaml`
- Create: `deploy/k8s/qdrant.yaml`
- Create: `deploy/k8s/secret.example.yaml`
- Create: `deploy/k8s/README.md`

**Interfaces:**
- Consumes: image `ghcr.io/jiezhuzzz/xuewen:latest` (Task 3), config file shape from the spec, Service name `xuewen-qdrant` (must match the baked/ConfigMap `qdrant_url`).

- [ ] **Step 1: `deploy/k8s/xuewen.yaml`**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: xuewen
  labels: { app: xuewen }
spec:
  replicas: 1            # SQLite + Tantivy are single-writer: never scale this up
  strategy:
    type: Recreate       # old pod must release /data before the new one mounts it
  selector:
    matchLabels: { app: xuewen }
  template:
    metadata:
      labels: { app: xuewen }
    spec:
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        runAsGroup: 1000
        fsGroup: 1000
      containers:
        - name: xuewen
          image: ghcr.io/jiezhuzzz/xuewen:latest
          ports:
            - containerPort: 8080
              name: http
          envFrom:
            - secretRef:
                name: xuewen-secrets
                optional: true   # keyword search works without the embedding key
          volumeMounts:
            - name: data
              mountPath: /data
            - name: config
              mountPath: /etc/xuewen
          readinessProbe:
            httpGet: { path: /api/stats, port: http }
            initialDelaySeconds: 3
            periodSeconds: 10
          livenessProbe:
            httpGet: { path: /api/stats, port: http }
            initialDelaySeconds: 10
            periodSeconds: 30
          resources:
            requests: { cpu: 100m, memory: 256Mi }
            limits: { memory: 1Gi }
      volumes:
        - name: data
          persistentVolumeClaim:
            claimName: xuewen-data
        - name: config
          configMap:
            name: xuewen-config
---
apiVersion: v1
kind: Service
metadata:
  name: xuewen
spec:
  selector: { app: xuewen }
  ports:
    - port: 8080
      targetPort: http
      name: http
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: xuewen-data
spec:
  accessModes: [ReadWriteOnce]
  resources:
    requests:
      storage: 10Gi
```

- [ ] **Step 2: `deploy/k8s/xuewen-config.yaml`**

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: xuewen-config
data:
  xuewen.toml: |
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

- [ ] **Step 3: `deploy/k8s/qdrant.yaml`** — before writing, look up the newest stable tag: `nix shell nixpkgs#skopeo -c skopeo list-tags docker://docker.io/qdrant/qdrant | nix shell nixpkgs#jq -c jq -r '.Tags[]' | grep -E '^v1\.[0-9]+\.[0-9]+$' | sort -V | tail -1` and substitute it for `v1.15.5` below if newer:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: xuewen-qdrant
  labels: { app: xuewen-qdrant }
spec:
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels: { app: xuewen-qdrant }
  template:
    metadata:
      labels: { app: xuewen-qdrant }
    spec:
      containers:
        - name: qdrant
          image: docker.io/qdrant/qdrant:v1.15.5
          ports:
            - containerPort: 6333
              name: http
          volumeMounts:
            - name: storage
              mountPath: /qdrant/storage
          readinessProbe:
            httpGet: { path: /readyz, port: http }
            initialDelaySeconds: 3
            periodSeconds: 10
          resources:
            requests: { cpu: 100m, memory: 256Mi }
            limits: { memory: 2Gi }
      volumes:
        - name: storage
          persistentVolumeClaim:
            claimName: xuewen-qdrant-storage
---
apiVersion: v1
kind: Service
metadata:
  name: xuewen-qdrant
spec:
  selector: { app: xuewen-qdrant }
  ports:
    - port: 6333
      targetPort: http
      name: http
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: xuewen-qdrant-storage
spec:
  accessModes: [ReadWriteOnce]
  resources:
    requests:
      storage: 5Gi
```

- [ ] **Step 4: `deploy/k8s/secret.example.yaml`**

```yaml
# Copy to secret.yaml, fill in the key, then: kubectl apply -f secret.yaml
# Without this Secret, Xuewen runs keyword-only (semantic search shows
# "unavailable" in the UI with the reason).
apiVersion: v1
kind: Secret
metadata:
  name: xuewen-secrets
type: Opaque
stringData:
  OPENAI_API_KEY: ""
```

- [ ] **Step 5: `deploy/k8s/README.md`**

```markdown
# Deploying Xuewen to Kubernetes

## Build and push the image

    skopeo login ghcr.io          # GitHub username + a token with write:packages
    nix run .#push                # pushes ghcr.io/jiezhuzzz/xuewen:<rev> and :latest

## Deploy

    kubectl apply -f deploy/k8s/

Optional semantic search: copy `secret.example.yaml` to `secret.yaml`,
fill in `OPENAI_API_KEY`, and `kubectl apply -f secret.yaml` (then restart
the pod). Any OpenAI-compatible endpoint works — edit the ConfigMap's
`[search.embedding]` block and `kubectl rollout restart deployment/xuewen`.

## Expose it — read this first

The pod has NO authentication: anyone who can reach the Service can import
and delete papers. Keep the Service ClusterIP-only and put your own
authenticating ingress (oauth2-proxy, Authelia, Tailscale, …) in front.
No Ingress manifest ships here because it is cluster-specific.

## Notes

- `replicas` must stay 1 (SQLite and Tantivy are single-writer); the
  Recreate strategy makes rollouts safe on the shared PVC.
- All state lives in the `xuewen-data` PVC (`/data`): the SQLite DB, PDFs,
  inbox, and the Tantivy index. Qdrant state is separate and rebuildable
  (`xuewen index rebuild --vectors-only` inside the pod).
- Validate manifests offline: `kubectl apply --dry-run=client --validate=false -f deploy/k8s/`
- Local smoke test without a cluster:

      nix run .#load                      # prints the loaded image:tag
      docker images ghcr.io/jiezhuzzz/xuewen   # find the tag (git short rev, or "dev")
      docker run --rm -p 8080:8080 -v xuewen-data:/data ghcr.io/jiezhuzzz/xuewen:<tag>
      curl http://localhost:8080/api/stats

- Inbox watching (`xuewen watch`) is deliberately not run in the pod
  (one process per container). Import through the web UI, or for one-off
  files: `kubectl cp paper.pdf <pod>:/data/inbox/ && kubectl exec <pod> --
  /nix/store/…/bin/xuewen --config /etc/xuewen/xuewen.toml ingest
  /data/inbox/paper.pdf` (tab-complete the store path inside the pod —
  it is image-specific).
```

- [ ] **Step 6: Validate the manifests**

Run: `nix shell nixpkgs#kubeconform -c kubeconform -strict -summary deploy/k8s/xuewen.yaml deploy/k8s/xuewen-config.yaml deploy/k8s/qdrant.yaml deploy/k8s/secret.example.yaml`
Expected: `Valid: N, Invalid: 0, Errors: 0` (kubeconform fetches schemas from the network; if offline, fall back to `nix shell nixpkgs#yq-go -c sh -c 'for f in deploy/k8s/*.yaml; do yq e "." "$f" >/dev/null || exit 1; done'` for syntax only and note that in the report).

- [ ] **Step 7: Commit**

```bash
git add deploy/k8s
git commit -m "feat(deploy): kubernetes manifests and deploy README"
```
