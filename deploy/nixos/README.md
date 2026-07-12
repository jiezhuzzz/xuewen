# Running Xuewen on NixOS

The flake exposes `nixosModules.default`, a systemd service for Xuewen. It
builds the package from this flake, generates `xuewen.toml` from Nix options,
runs `pdftotext` (poppler) in the unit's `PATH`, and applies systemd hardening.

## Quick start (flakes)

```nix
# flake.nix
{
  inputs.xuewen.url = "github:jiezhuzzz/xuewen";

  outputs = { nixpkgs, xuewen, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        xuewen.nixosModules.default
        ({ ... }: {
          services.xuewen = {
            enable = true;
            # host = "127.0.0.1";  # default; non-loopback auto-adds --allow-remote
            # port = 8080;

            settings.ai = {
              api_key_env = "OPENAI_API_KEY";
              embedding = { model = "text-embedding-3-small"; dims = 1536; };
              chat.models = [{
                label = "GPT-4o mini";
                model = "gpt-4o-mini";
                # reasoning_effort = "high";  # reasoning models only
              }];
            };

            # Secrets stay OUT of the Nix store (see below).
            environmentFile = "/run/secrets/xuewen.env";
          };
        })
      ];
    };
  };
}
```

`inbox_dir`, `library_root`, `database_url` and `search.index_dir` default
under `services.xuewen.dataDir` (`/var/lib/xuewen`); override them via
`settings` if needed.

## Secrets

The generated `xuewen.toml` lands in the world-readable Nix store, so never put
API keys in `settings`. Instead reference an env var with `api_key_env` and
provide it through `environmentFile`:

```
# /run/secrets/xuewen.env  (0600, root-owned, e.g. via sops-nix / agenix)
OPENAI_API_KEY=sk-...
```

## Semantic search (optional)

Keyword search works out of the box. Semantic search additionally needs a
Qdrant server; NixOS ships one:

```nix
services.qdrant.enable = true;   # listens on 127.0.0.1:6333
services.xuewen.settings.search.qdrant_url = "http://127.0.0.1:6333";
```

## Exposing it

The web UI has **no authentication** and exposes mutating endpoints. Keep the
default loopback bind and front it with an authenticating reverse proxy, e.g.:

```nix
services.nginx.virtualHosts."papers.example.com" = {
  enableACME = true; forceSSL = true;
  locations."/".proxyPass = "http://127.0.0.1:8080";
  # add basic auth / oauth2-proxy here
};
```

Setting a non-loopback `host` binds publicly (and auto-adds `--allow-remote`);
only do that on a trusted network.

## Options

| Option | Default | Purpose |
| --- | --- | --- |
| `enable` | `false` | Enable the service |
| `package` | flake build | Xuewen package to run |
| `host` / `port` | `127.0.0.1` / `8080` | Bind address |
| `openFirewall` | `false` | Open the port |
| `dataDir` | `/var/lib/xuewen` | Library / DB / index state |
| `user` / `group` | `xuewen` | Service identity (auto-created) |
| `environmentFile` | `null` | systemd `EnvironmentFile` for secrets |
| `settings` | `{}` | `xuewen.toml` as a Nix attrset |

## Test

A VM test boots a machine, enables the module, and hits the API (Linux + KVM):

```
nix build .#checks.x86_64-linux.nixos-module -L
```
