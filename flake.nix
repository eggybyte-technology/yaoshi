{
  description = "Yaoshi development shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          rust-overlay.overlays.default
        ];
      };

      rustToolchain = pkgs.rust-bin.stable."1.96.0".minimal.override {
        extensions = [
          "rustfmt"
          "clippy"
          "rust-src"
        ];
        targets = [
          "x86_64-unknown-linux-musl"
        ];
      };

      mmdebstrapSrc = pkgs.fetchzip {
        url = "https://mirrors.aliyun.com/debian/pool/main/m/mmdebstrap/mmdebstrap_1.5.7.orig.tar.gz";
        sha256 = "1mxm09l9cvcypk3f5xi9mzj5b5iry0gciwcfiynh5nlif8abci1z";
      };

      mmdebstrapRuntimePath = pkgs.lib.makeBinPath [
        pkgs.libarchive
        pkgs.gnutar
        pkgs.gzip
        pkgs.xz
        pkgs.bzip2
        pkgs.zstd
        pkgs.coreutils
        pkgs.util-linux
        pkgs.findutils
        pkgs.gnugrep
        pkgs.gnused
        pkgs.gawk
        pkgs.gnupg
        pkgs.debootstrap
        pkgs.fakeroot
        pkgs.fakechroot
        pkgs.perl
      ];

      mmdebstrapPerl5Lib = pkgs.perlPackages.makePerlPath [
        pkgs.perlPackages.ArchiveLibarchive
      ];

      mmdebstrapLibraryPath = pkgs.lib.makeLibraryPath [
        pkgs.libarchive
      ];

      mmdebstrap = pkgs.stdenvNoCC.mkDerivation {
        pname = "mmdebstrap";
        version = "1.5.7";
        src = mmdebstrapSrc;
        nativeBuildInputs = [
          pkgs.makeWrapper
          pkgs.perl
          pkgs.glibc.dev
          pkgs.linuxHeaders
        ];
        dontBuild = true;
        installPhase = ''
          runHook preInstall
          mkdir -p "$out/bin" "$out/libexec" "$out/lib/perl5/site_perl"
          cp "$src/mmdebstrap" "$out/libexec/mmdebstrap"
          chmod 0755 "$out/libexec/mmdebstrap"

          export CPATH="${pkgs.glibc.dev}/include:${pkgs.linuxHeaders}/include"
          (cd "${pkgs.glibc.dev}/include" && h2ph -Q -a -d "$out/lib/perl5/site_perl" syscall.h sys/ioctl.h) || true

          makeWrapper "${pkgs.perl}/bin/perl" "$out/bin/mmdebstrap" \
            --add-flags "$out/libexec/mmdebstrap" \
            --set PERL5LIB "$out/lib/perl5/site_perl:${mmdebstrapPerl5Lib}" \
            --prefix LD_LIBRARY_PATH : "${mmdebstrapLibraryPath}" \
            --prefix PATH : "${mmdebstrapRuntimePath}"
          runHook postInstall
        '';
      };

      bashEnv = pkgs.writeText "yaoshi-devshell-bash-env" ''
        if [ -n "''${YAOSHI_DEV_SHELL_PATH:-}" ]; then
          export PATH="$YAOSHI_DEV_SHELL_PATH:$PATH"
        fi
      '';

      devShellPackages = [
        rustToolchain
        pkgs.rust-analyzer
        pkgs.cargo-nextest
        pkgs.sccache
        pkgs.qemu
        pkgs.OVMF.fd
        pkgs.openssh
        pkgs.pkgsCross.musl64.stdenv.cc
        pkgs.xz
        pkgs.e2fsprogs
        mmdebstrap
      ];

      devShellPath = pkgs.lib.makeBinPath devShellPackages;
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = devShellPackages;

        shellHook = ''
          export YAOSHI_DEV_SHELL_PATH="${devShellPath}"
          export PATH="$YAOSHI_DEV_SHELL_PATH:$PATH"
          export SCCACHE_DIR="$PWD/.yaoshi/sccache"
          unset SCCACHE_BUCKET
          unset SCCACHE_ENDPOINT
          unset SCCACHE_REGION
          export BASH_ENV="${bashEnv}"
        '';
      };
    };
}
