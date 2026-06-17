<p align="center">
  <img src="assets/color-logo.svg" alt="Yaoshi logo" width="96" height="128">
</p>

<h1 align="center">Yaoshi</h1>

<p align="center">
  Debian-native image builder, installer payload generator, and local machine dashboard.
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue"></a>
  <img alt="Rust 1.96.0" src="https://img.shields.io/badge/rust-1.96.0-f46623">
  <img alt="Debian trixie" src="https://img.shields.io/badge/debian-trixie-a81d33">
</p>

---

Yaoshi creates a USB-writable installer image that installs a root-only Debian system and boots into Yaoshi Dashboard. It is built as a Rust workspace around deterministic local image construction, a content-addressed artifact cache, a sparse zstd target-write payload, a minimal disk installer, and a read-only terminal dashboard.

The normative product and architecture specification is [docs/design.md](docs/design.md).

## What It Builds

Yaoshi produces a raw disk image at:

```text
./.yaoshi/out/yaoshi.img
```

That image contains:

- A UEFI-bootable installer media envelope.
- A Debian trixie installed target payload.
- A sparse `YAOSHI_PAYLOAD_V1` write plan for fast target installation.
- Yaoshi runtime assets for first boot, root SSH access, rescue shell, and dashboard startup.

USB boot uses the UEFI fallback path:

```text
/EFI/BOOT/BOOTX64.EFI
```

The installed system starts Yaoshi Dashboard on `tty1`. `Alt+F2` opens the `tty2` direct root rescue shell.

## Architecture

```text
yaoshi CLI
  -> Debian package root via mmdebstrap
  -> customized installed root
  -> ext4 root filesystem
  -> FAT32 ESP and installer boot media
  -> sparse zstd target-write payload
  -> USB-writable raw image
```

The installer is intentionally small. It is not a live Debian environment and does not include a shell, package manager, service manager, SSH daemon, or dashboard. Its job is to validate the payload, select a target disk, write declared extents, flush the target, and reboot or power off.

## Workspace

| Crate | Purpose |
| --- | --- |
| `yaoshi` | User-facing CLI entrypoint. |
| `yaoshi-build` | Build pipeline, templates, cache orchestration, and artifact graph. |
| `yaoshi-common` | Shared constants, errors, disk, layout, and filesystem utilities. |
| `yaoshi-debian` | Debian artifact integration helpers. |
| `yaoshi-image` | MBR, GPT, FAT32, ext4 validation, and image writing. |
| `yaoshi-initramfs` | Installer initramfs generation. |
| `yaoshi-payload` | Sparse target-write payload format and validation. |
| `yaoshi-installer` | Minimal installer runtime. |
| `yaoshi-dashboard` | Read-only installed-system dashboard. |
| `yaoshi-screen` | Shared terminal renderer. |
| `yaoshi-test` | Integration and conformance test helpers. |

## Development

The repository is configured for `direnv` through `.envrc`. The dev shell is defined in [flake.nix](flake.nix) and includes the Rust toolchain, cargo-nextest, QEMU, OVMF, mmdebstrap, e2fsprogs, and supporting build tools.

Enter the environment:

```bash
direnv allow
```

Run the CLI:

```bash
cargo run --locked -p yaoshi --
cargo run --locked -p yaoshi -- --version
```

The product configuration file is:

```text
./.yaoshi/yaoshi.toml
```

## Validation

Run the default development tests:

```bash
cargo nextest run --locked --workspace
```

Run the QEMU release acceptance profile:

```bash
cargo nextest run --locked --workspace --profile qemu
```

The `qemu` profile runs live QEMU functional tests. QEMU firmware discovery follows the rules in [docs/design.md](docs/design.md).

## License

Yaoshi is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).

Copyright 2026 Eggybyte Technology.
