# NixOS module for Xuewen. Exposed from the flake as `nixosModules.default`
# (which also fills in `services.xuewen.package` from the flake's build) and
# `nixosModules.xuewen` (this file; set `services.xuewen.package` yourself).
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
in
{
  options.services.xuewen = {
    enable = lib.mkEnableOption "Xuewen, a self-hosted reference manager";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalMD "the flake's `xuewen` package (via `nixosModules.default`)";
      description = ''
        The xuewen package to run. `nixosModules.default` sets this to the
        flake's build; with the bare `nixosModules.xuewen` you must set it.
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

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open {option}`services.xuewen.port` in the firewall.";
    };

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/xuewen";
      description = ''
        State directory holding the inbox, library, SQLite database and search
        index. Created automatically with the right ownership.
      '';
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "xuewen";
      description = "User the service runs as (created when left at the default).";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "xuewen";
      description = "Group the service runs as (created when left at the default).";
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/run/secrets/xuewen.env";
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
      message = "services.xuewen.package must be set (use nixosModules.default, or set it explicitly).";
    }];

    warnings = lib.optional (!isLoopback cfg.host)
      "services.xuewen binds the non-loopback address ${cfg.host}; the web UI has no auth. Put it behind an authenticating reverse proxy.";

    users.users = lib.mkIf (cfg.user == "xuewen") {
      xuewen = {
        isSystemUser = true;
        group = cfg.group;
        home = cfg.dataDir;
        description = "Xuewen service user";
      };
    };
    users.groups = lib.mkIf (cfg.group == "xuewen") { xuewen = { }; };

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];

    systemd.tmpfiles.rules = [
      "d ${cfg.dataDir} 0750 ${cfg.user} ${cfg.group} - -"
    ];

    systemd.services.xuewen = {
      description = "Xuewen reference manager";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      # pdftotext (poppler-utils) is required for PDF text extraction, which the
      # ingest pipeline and paper chat both depend on. git backs the repo-attach
      # endpoint (PUT /api/papers/{id}/code shallow-clones into the agent
      # workspace).
      path = [ pkgs.poppler-utils pkgs.git ];
      environment = {
        RUST_LOG = lib.mkDefault "info";
        # reqwest talks HTTPS to arXiv/Crossref/OpenAI; give it a CA bundle
        # under the hardened (ProtectSystem=strict) sandbox.
        SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
      };
      serviceConfig = {
        ExecStart = lib.escapeShellArgs ([
          "${cfg.package}/bin/xuewen"
          "--config" "${configFile}"
          "serve" "--host" cfg.host "--port" (toString cfg.port)
        ] ++ lib.optional (!isLoopback cfg.host) "--allow-remote");
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = cfg.dataDir;
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) [ cfg.environmentFile ];
        Restart = "on-failure";
        RestartSec = 5;

        # Hardening: the service only ever writes under dataDir.
        ReadWritePaths = [ cfg.dataDir ];
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectControlGroups = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        RestrictNamespaces = true;
        RestrictRealtime = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
      };
    };
  };
}
