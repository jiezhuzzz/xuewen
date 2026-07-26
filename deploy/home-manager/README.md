# Running Xuewen with Home Manager

The flake exposes `homeManagerModules.default`, a per-user `systemd --user`
service for Xuewen. It builds the package from this flake, generates
`xuewen.toml` from Nix options, puts `pdftotext` (poppler) on the unit's
`PATH`, and applies user-service hardening.

**Linux only.** The module relies on systemd user units, which Home Manager
does not provide on macOS. On a Mac, use the native desktop app
(`cargo run -p xuewen-desktop`, or the bundled `.dmg`) instead.

## Quick start (flakes)

```nix
# flake.nix
{
  inputs = {
    home-manager.url = "github:nix-community/home-manager";
    xuewen.url = "github:jiezhuzzz/xuewen";
  };

  outputs = { nixpkgs, home-manager, xuewen, ... }: {
    homeConfigurations."me@myhost" = home-manager.lib.homeManagerConfiguration {
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      modules = [
        xuewen.homeManagerModules.default
        ({ ... }: {
          services.xuewen = {
            enable = true;
            # host = "127.0.0.1";  # default; non-loopback auto-adds --allow-remote
            # port = 8080;

            settings.ai = {
              api_key_env = "OPENAI_API_KEY";
              embedding = { model = "text-embedding-3-small"; dims = 1536; };
              summary = { };  # per-paper LLM summaries
              # Paper chat / the Ask tab is Agent Ask — configure [ai.agent.*]
              # separately (needs Node + agent-runner); see the main README.
            };

            # Secrets stay OUT of the Nix store (see below).
            environmentFile = "/run/user/1000/secrets/xuewen.env";
          };
        })
      ];
    };
  };
}
```

The service auto-starts on login (`WantedBy = default.target`). Manage it with
`systemctl --user {start,stop,status} xuewen` and read logs with
`journalctl --user -u xuewen`.

`inbox_dir`, `library_root`, `database_url` and `search.index_dir` default
under `services.xuewen.dataDir` (`$XDG_DATA_HOME/xuewen`, i.e.
`~/.local/share/xuewen`); override them via `settings` if needed.

## Secrets

The generated `xuewen.toml` lands in the world-readable Nix store, so never put
API keys in `settings`. Instead reference an env var with `api_key_env` and
provide it through `environmentFile`:

```
# a file NOT in the Nix store, readable only by you — e.g. via sops-nix
OPENAI_API_KEY=sk-...
```

## Semantic search (optional)

Keyword search works out of the box. Semantic search additionally needs a
Qdrant server on `http://localhost:6333`; point Xuewen at it with:

```nix
services.xuewen.settings.search.qdrant_url = "http://127.0.0.1:6333";
```

Running Qdrant itself is a system concern (there is no Home Manager module for
it); a NixOS host can enable `services.qdrant`, or run it via a container.

## Exposing it

The web UI has **no authentication** and exposes mutating endpoints. Keep the
default loopback bind and front it with an authenticating reverse proxy.
Setting a non-loopback `host` binds publicly (and auto-adds `--allow-remote`);
only do that on a trusted network.

## Options

| Option | Default | Purpose |
| --- | --- | --- |
| `enable` | `false` | Enable the service |
| `package` | flake build | Xuewen package to run |
| `host` / `port` | `127.0.0.1` / `8080` | Bind address |
| `dataDir` | `$XDG_DATA_HOME/xuewen` | Library / DB / index state |
| `environmentFile` | `null` | systemd `EnvironmentFile` for secrets |
| `settings` | `{}` | `xuewen.toml` as a Nix attrset |

For a system-wide (multi-user, `/var/lib`) install, use the NixOS module
instead — see [`../nixos/README.md`](../nixos/README.md).
