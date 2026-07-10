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
