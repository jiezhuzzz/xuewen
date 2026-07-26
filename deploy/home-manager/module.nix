# Home Manager module for Xuewen. Exposed from the flake as
# `homeManagerModules.default` (which also fills in `services.xuewen.package`
# from the flake's build) and `homeManagerModules.xuewen` (this file; set
# `services.xuewen.package` yourself).
#
# Runs Xuewen as a per-user `systemd --user` service. Linux only: it relies on
# systemd user units, which Home Manager does not provide on Darwin. macOS
# users have the native desktop app (`xuewen-desktop`) instead.
{ config, lib, pkgs, ... }:

let
  cfg = config.services.xuewen;
  tomlFormat = pkgs.formats.toml { };

  # Paths the backend requires. They default under `dataDir`; anything the
  # user puts in `settings` wins (recursiveUpdate is deep, so setting
  # `settings.search.qdrant_url` keeps the `index_dir` default below).
  derivedSettings = {
    inbox_dir = "${cfg.dataDir}/inbox";
    library_root = "${cfg.dataDir}/library";
    database_url = "sqlite:${cfg.dataDir}/library.db";
    search.index_dir = "${cfg.dataDir}/search-index";
  };
  configFile = tomlFormat.generate "xuewen.toml"
    (lib.recursiveUpdate derivedSettings cfg.settings);

  # Mirror the backend's own `web::is_loopback_host`: non-loopback binds serve
  # unauthenticated mutating endpoints, so `serve` refuses them without
  # `--allow-remote`.
  isLoopback = h: h == "localhost" || h == "::1" || lib.hasPrefix "127." h;

  # Agent Ask ([ai.agent.*]) spawns `node` for its runner; its presence also
  # decides the MemoryDenyWriteExecute hardening below.
  agentConfigured = lib.hasAttrByPath [ "ai" "agent" ] cfg.settings;

  # pdftotext (poppler-utils) is required for PDF text extraction, which the
  # ingest pipeline and paper chat both depend on. git backs the repo-attach
  # endpoint (PUT /api/papers/{id}/code shallow-clones into the agent
  # workspace). node is only needed when [ai.agent.*] is configured.
  runtimePath = lib.makeBinPath ([ pkgs.poppler-utils pkgs.git ]
    ++ lib.optional agentConfigured pkgs.nodejs);
in
{
  options.services.xuewen = {
    enable = lib.mkEnableOption "Xuewen, a self-hosted reference manager";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalMD "the flake's `xuewen` package (via `homeManagerModules.default`)";
      description = ''
        The xuewen package to run. `homeManagerModules.default` sets this to the
        flake's build; with the bare `homeManagerModules.xuewen` you must set it.
      '';
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = ''
        Address to bind. The web UI has no authentication and exposes mutating
        endpoints, so a non-loopback address adds `--allow-remote` and should
        sit behind an authenticating reverse proxy.
      '';
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8080;
      description = "TCP port to bind.";
    };

    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "${config.xdg.dataHome}/xuewen";
      defaultText = lib.literalExpression ''"''${config.xdg.dataHome}/xuewen"'';
      description = ''
        State directory holding the inbox, library, SQLite database and search
        index. Created automatically before the service starts.
      '';
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/run/user/1000/secrets/xuewen.env";
      description = ''
        A systemd `EnvironmentFile` holding secrets that must stay out of the
        world-readable Nix store — e.g. `OPENAI_API_KEY=sk-…` for the
        `api_key_env` referenced by `[ai.*]`.
      '';
    };

    settings = lib.mkOption {
      type = tomlFormat.type;
      default = { };
      example = lib.literalExpression ''
        {
          ai = {
            api_key_env = "OPENAI_API_KEY";
            model = "gpt-4o-mini";
            embedding = { model = "text-embedding-3-small"; dims = 1536; };
            chat.models = [{ label = "GPT-4o mini"; model = "gpt-4o-mini"; }];
            summary = { };
          };
        }
      '';
      description = ''
        `xuewen.toml` as a Nix attrset. `inbox_dir`, `library_root`,
        `database_url` and `search.index_dir` default under
        {option}`services.xuewen.dataDir`; set them here to override.

        Do NOT put API keys here — the generated file lands in the
        world-readable Nix store. Use `api_key_env` together with
        {option}`services.xuewen.environmentFile` instead.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = cfg.package != null;
      message = "services.xuewen.package must be set (use homeManagerModules.default, or set it explicitly).";
    }];

    warnings = lib.optional (!isLoopback cfg.host)
      "services.xuewen binds the non-loopback address ${cfg.host}; the web UI has no auth. Put it behind an authenticating reverse proxy.";

    systemd.user.services.xuewen = {
      Unit = {
        Description = "Xuewen reference manager";
        After = [ "network-online.target" ];
        Wants = [ "network-online.target" ];
      };
      Install.WantedBy = [ "default.target" ];
      Service = {
        # The data dir lives under $HOME, so create it before first launch
        # rather than through system tmpfiles.
        ExecStartPre = "${pkgs.coreutils}/bin/mkdir -p ${cfg.dataDir}";
        ExecStart = lib.escapeShellArgs ([
          "${cfg.package}/bin/xuewen"
          "--config" "${configFile}"
          "serve" "--host" cfg.host "--port" (toString cfg.port)
        ] ++ lib.optional (!isLoopback cfg.host) "--allow-remote");
        WorkingDirectory = cfg.dataDir;
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
        Environment = [
          "RUST_LOG=info"
          # reqwest talks HTTPS to arXiv/Crossref/OpenAI; give it a CA bundle.
          "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          # pdftotext, git, and (when Agent Ask is on) node are resolved from here.
          "PATH=${runtimePath}"
        ];
        Restart = "on-failure";
        RestartSec = 5;

        # Hardening appropriate for a user unit whose state lives under $HOME.
        # ProtectHome is deliberately NOT set — it would hide the data dir from
        # the service itself.
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictRealtime = true;
        LockPersonality = true;
        # node's JIT needs writable-then-executable mappings, so this hardening
        # must relax when Agent Ask spawns the runner.
        MemoryDenyWriteExecute = !agentConfigured;
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
      };
    };
  };
}
