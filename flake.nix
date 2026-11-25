{
  description = "Fill in PDF forms the way they were meant to be filled in";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      eachSystem =
        f:
        lib.genAttrs systems (
          system:
          f (import nixpkgs {
            inherit system;
            # PolyForm Strict is not a free licence; allow only this package.
            config.allowUnfreePredicate = pkg: lib.getName pkg == "pdfr";
          })
        );

      # dlopened at run time, so Nix cannot detect them as dependencies.
      runtimeLibs =
        pkgs: with pkgs; [
          libglvnd
          libxkbcommon
          wayland
          libx11
          libxcb
          libxcursor
          libxrandr
          libxi
        ];
    in
    {
      packages = eachSystem (pkgs: rec {
        default = pdfr;
        pdfr = pkgs.rustPlatform.buildRustPackage {
          pname = "pdfr";
          version = (lib.importTOML ./Cargo.toml).workspace.package.version;
          src = self;
          cargoLock.lockFile = ./Cargo.lock;

          cargoBuildFlags = [ "-p" "pdfr" ];
          # The GUI tests need a display.
          cargoTestFlags = [
            "-p"
            "pdfr-core"
            "-p"
            "pdfr-assist"
          ];

          nativeBuildInputs = lib.optionals pkgs.stdenv.hostPlatform.isLinux [
            pkgs.pkg-config
            pkgs.makeWrapper
          ];
          buildInputs = lib.optionals pkgs.stdenv.hostPlatform.isLinux [
            pkgs.wayland
            pkgs.libxkbcommon
          ];

          postInstall = lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
            mv $out/bin/pdfr pdfr-built
            packaging/linux/install.sh pdfr-built "" $out
            wrapProgram $out/bin/pdfr \
              --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath (runtimeLibs pkgs)}
          '';

          meta = {
            description = "Fill in PDF forms the way they were meant to be filled in";
            homepage = "https://github.com/kjellberg/pdfr";
            license = {
              fullName = "PolyForm Strict License 1.0.0";
              url = "https://polyformproject.org/licenses/strict/1.0.0/";
              free = false;
              redistributable = true;
            };
            mainProgram = "pdfr";
            platforms = systems;
          };
        };
      });

      devShells = eachSystem (pkgs: {
        default = pkgs.mkShell {
          packages =
            with pkgs;
            [
              cargo
              rustc
              rustfmt
              clippy
              rust-analyzer
            ]
            ++ lib.optionals stdenv.hostPlatform.isLinux [
              pkg-config
              wayland
              libxkbcommon
            ];
          env = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            LD_LIBRARY_PATH = lib.makeLibraryPath (runtimeLibs pkgs);
          };
        };
      });
    };
}
