{
  description = "A basic Rust devshell for NixOS users developing hoolamike";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
          config.allowUnfree = true;
        };
      in
        with pkgs; {
          devShells.default = mkShell {
            buildInputs =
              [
                openssl
                pkg-config
                cacert
                clang
                cargo-make
                trunk
                libarchive
                # compression
                # p7zip
                p7zip-rar
                # mold-wrapped # couldn't get mold to work
                # for tests
                glib
                gdk-pixbuf
                stdenv.cc
                atkmm
                pango
                gdk-pixbuf-xlib
                gtk3
                libsoup_3
                webkitgtk_4_1
                xdotool
                xdo
                just
                jless
                jq

                # xdelta3 bindings
                llvmPackages_latest.libclang.lib

                (rust-bin
                  .selectLatestNightlyWith (toolchain:
                  toolchain
                    .default
                    .override {
                    extensions = ["rust-src" "rust-analyzer" "clippy"];
                    targets = ["x86_64-pc-windows-gnu" "x86_64-unknown-linux-gnu"];
                  }))
              ]
              ++ pkgs.lib.optionals pkg.stdenv.isDarwin [
                darwin.apple_sdk.frameworks.SystemConfiguration
              ];

            shellHook = ''
              export LIBCLANG_PATH=${pkgs.lib.makeLibraryPath [pkgs.llvmPackages_latest.libclang.lib]};
            '';
          };
        }
    );
}
