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
          npmDepsHash = "sha256-NxSVwo7RN0/GHNzqgnwrf0+pw3jUvTlOC7cJG0HqW2Y=";
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
              in !(under "frontend" || under "docs" || under "deploy");
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
    };
}
