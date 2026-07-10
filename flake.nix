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
        default = frontend; # replaced by `xuewen` in Task 2
      });
    };
}
