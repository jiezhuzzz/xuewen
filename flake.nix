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
      # Bound locally (rather than read back through `self.packages`) so the
      # x86_64-linux image override below can extend it without a
      # self-referential infinite recursion.
      perSystemPackages = forAll (pkgs: rec {
        frontend = pkgs.buildNpmPackage {
          pname = "xuewen-frontend";
          version = "0.1.0";
          src = ./frontend;
          npmDepsHash = "sha256-enRlx7yii4sBjjjB6APvqREmu3fSQrk05UlbbXnf2e0=";
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
        xuewen = pkgs.rustPlatform.buildRustPackage {
          pname = "xuewen";
          version = "0.1.0";
          # Exclude the frontend sources (dist comes from the `frontend`
          # package), docs, and deploy manifests so editing them never
          # rebuilds the backend.
          src = pkgs.lib.cleanSourceWith {
            src = self;
            filter = path: _type:
              let
                rel = pkgs.lib.removePrefix (toString self + "/") (toString path);
                under = dir: rel == dir || pkgs.lib.hasPrefix (dir + "/") rel;
              in !(under "frontend" || under "docs" || under "deploy"
                || rel == "flake.nix" || rel == "flake.lock"
                || rel == ".gitignore" || rel == ".envrc");
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
          nativeCheckInputs = [ pkgs.poppler-utils ];
        };
        default = xuewen;
      });
    in {
      # Convenience overlay: `pkgs.xuewen` / `pkgs.xuewen-frontend`.
      overlays.default = final: prev: {
        xuewen = self.packages.${prev.stdenv.hostPlatform.system}.xuewen;
        xuewen-frontend = self.packages.${prev.stdenv.hostPlatform.system}.frontend;
      };

      # `nixosModules.default` is the batteries-included module: it defaults
      # `services.xuewen.package` to this flake's build for the host system.
      # `nixosModules.xuewen` is the bare module (set `package` yourself).
      nixosModules.xuewen = ./deploy/nixos/module.nix;
      nixosModules.default = { pkgs, lib, ... }: {
        imports = [ self.nixosModules.xuewen ];
        services.xuewen.package =
          lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.xuewen;
      };

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

      packages = perSystemPackages // {
        # Image + registry wiring are Linux/amd64-only.
        x86_64-linux = let
          pkgs = nixpkgs.legacyPackages.x86_64-linux;
          base = perSystemPackages.x86_64-linux; # frontend/xuewen from forAll above
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

              [ai]
              api_key_env = "OPENAI_API_KEY"
              model       = "gpt-4o-mini"   # default for chat/summary/daily when enabled

              [ai.embedding]
              model = "text-embedding-3-small"
              dims  = 1536
            '';
          };
          # Empty /data owned 1000:1000 baked into the image so Docker's
          # named-volume copy-up (which mirrors the mountpoint's ownership)
          # doesn't leave the volume root-owned and unwritable by the
          # container's non-root user.
          dataDir = pkgs.runCommand "xuewen-data-dir" { } "mkdir -p $out/data";
          image = n2c.buildImage {
            name = "ghcr.io/jiezhuzzz/xuewen";
            inherit tag;
            # Layer 1: runtime deps that rarely change (cheap re-pulls on
            # app updates). The app closure lands in the final layer via
            # the Entrypoint reference.
            layers = [
              (n2c.buildLayer { deps = [ pkgs.poppler-utils pkgs.cacert ]; })
            ];
            copyToRoot = [ configFile dataDir ];
            perms = [
              {
                path = dataDir;
                regex = "/data";
                mode = "0755";
                uid = 1000;
                gid = 1000;
              }
            ];
            config = {
              Entrypoint = [
                "${base.xuewen}/bin/xuewen"
                "--config" "/etc/xuewen/xuewen.toml"
                "serve" "--host" "0.0.0.0" "--port" "8080" "--allow-remote"
              ];
              Env = [
                "PATH=${base.xuewen}/bin:${pkgs.poppler-utils}/bin"
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
          if [ "${tag}" = "dev" ]; then
            echo "dirty tree — pushed :dev only, not :latest"
          else
            nix run .#image.copyTo -- docker://ghcr.io/jiezhuzzz/xuewen:latest
          fi
        '';
        load = pkgs.writeShellScriptBin "xuewen-load" ''
          set -euo pipefail
          nix run .#image.copyToDockerDaemon
        '';
      in {
        push = {
          type = "app";
          program = "${push}/bin/xuewen-push";
          meta.description = "push the xuewen image to ghcr";
        };
        load = {
          type = "app";
          program = "${load}/bin/xuewen-load";
          meta.description = "load the xuewen image into the local docker daemon";
        };
      };

      checks = let
        base = forAll (pkgs: {
          frontend = self.packages.${pkgs.stdenv.hostPlatform.system}.frontend;
          xuewen = self.packages.${pkgs.stdenv.hostPlatform.system}.xuewen;
        });
      in base // {
        x86_64-linux = base.x86_64-linux // {
          image = self.packages.x86_64-linux.image;
          # Boots a VM, enables the module, and hits the API. Linux-only
          # (needs KVM); run with `nix build .#checks.x86_64-linux.nixos-module`.
          nixos-module = nixpkgs.legacyPackages.x86_64-linux.testers.runNixOSTest {
            name = "xuewen-module";
            nodes.machine = { ... }: {
              imports = [ self.nixosModules.default ];
              services.xuewen.enable = true;
            };
            testScript = ''
              machine.wait_for_unit("xuewen.service")
              machine.wait_for_open_port(8080)
              machine.succeed("curl -sf http://127.0.0.1:8080/api/stats | grep -q '\"total\"'")
            '';
          };
        };
      };
    };
}
