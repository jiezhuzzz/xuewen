# Deploying Xuewen to Kubernetes

## Build and push the image

    skopeo login ghcr.io          # GitHub username + a token with write:packages
    nix run .#push                # pushes ghcr.io/jiezhuzzz/xuewen:<rev> and :latest

Run `nix run .#push` from the repo root (it resolves the flake in the
current directory).

## Deploy

    kubectl apply -f deploy/k8s/

Optional semantic search — create the `xuewen-secrets` Secret with the
embedding API key:

    kubectl create secret generic xuewen-secrets --from-literal=OPENAI_API_KEY=sk-...

then restart the pod. Alternative, file-based flow: `cp
deploy/k8s/secret.yaml.example deploy/k8s/secret.yaml`, fill in
`OPENAI_API_KEY`, and `kubectl apply -f deploy/k8s/secret.yaml`
(`secret.yaml` is gitignored, so it's never picked up by `kubectl apply -f
deploy/k8s/` and never accidentally committed). Any OpenAI-compatible
endpoint works — edit the ConfigMap's `[search.embedding]` block and
`kubectl rollout restart deployment/xuewen`.

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
  (`kubectl exec deploy/xuewen -- xuewen --config /etc/xuewen/xuewen.toml
  index rebuild --vectors-only` — the explicit `--config` is required
  because the default looks for `./xuewen.toml`).
- Validate manifests offline: `kubectl apply --dry-run=client --validate=false -f deploy/k8s/`
- Local smoke test without a cluster:

      nix run .#load                      # prints the loaded image:tag
      docker images ghcr.io/jiezhuzzz/xuewen   # find the tag (git short rev, or "dev")
      docker run --rm -p 8080:8080 -v xuewen-data:/data ghcr.io/jiezhuzzz/xuewen:<tag>
      curl http://localhost:8080/api/stats

- Inbox watching (`xuewen watch`) is deliberately not run in the pod
  (one process per container). Import through the web UI, or for one-off
  files without the UI:

      kubectl port-forward svc/xuewen 8080:8080
      curl -F file=@paper.pdf http://localhost:8080/api/papers

  `kubectl cp` does NOT work here — the image has no `tar`.

- Vector rebuild in-pod:

      kubectl exec deploy/xuewen -- xuewen --config /etc/xuewen/xuewen.toml index rebuild --vectors-only

  The explicit `--config` is required — without it, `xuewen` looks for
  `./xuewen.toml` and won't find the mounted ConfigMap.
