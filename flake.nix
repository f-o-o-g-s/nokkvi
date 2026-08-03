{
  description = "A fast and beautiful Rust/Iced desktop client for Navidrome";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      nixpkgsFor = forAllSystems (system: nixpkgs.legacyPackages.${system});
    in
    {
      formatter = forAllSystems (system: nixpkgsFor.${system}.nixfmt);

      overlays.default = final: prev: {
        nokkvi = self.packages.${prev.stdenv.hostPlatform.system}.default;
      };

      packages = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
          runtimeLibs = with pkgs; [
            pipewire
            alsa-lib
            fontconfig
            freetype
            libglvnd
            libxkbcommon
            wayland
            libX11
            libXcursor
            libXrandr
            libXi
            mesa
            vulkan-loader
            openssl
            dbus
          ];
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "nokkvi";
            version = "0.18.4";

            src = ./.;

            cargoHash = "sha256-RjcAjOWrPDqdHlqDEu/mzEmvb76iT5E/t0rRB0iEl8g=";
            doCheck = false;

            nativeBuildInputs = with pkgs; [
              pkg-config
              cmake
              makeWrapper
              rustPlatform.bindgenHook
            ];

            buildInputs = runtimeLibs ++ [ pkgs.gsettings-desktop-schemas ];

            postInstall = ''
              wrapProgram $out/bin/nokkvi \
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath runtimeLibs}" \
                --prefix XDG_DATA_DIRS : "$out/share:${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}"

              install -Dm644 assets/org.nokkvi.nokkvi.desktop -t $out/share/applications/
              install -Dm644 assets/org.nokkvi.nokkvi.svg -t $out/share/icons/hicolor/scalable/apps/
              install -Dm644 assets/org.nokkvi.nokkvi.png -t $out/share/icons/hicolor/256x256/apps/
            '';

            meta = with pkgs.lib; {
              description = "A fast and beautiful Rust/Iced desktop client for Navidrome";
              homepage = "https://github.com/f-o-o-g-s/nokkvi";
              license = licenses.gpl3Only;
              mainProgram = "nokkvi";
              platforms = platforms.linux;
            };
          };
        }
      );

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${nixpkgsFor.${system}.lib.getExe self.packages.${system}.default}";
        };
      });

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
          targetPkg = self.packages.${system}.default;
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ targetPkg ];
            nativeBuildInputs = with pkgs; [
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer
            ];
            env.LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath targetPkg.buildInputs;
          };
        }
      );
    };
}
