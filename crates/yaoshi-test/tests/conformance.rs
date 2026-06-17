use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

fn rust_source_text(root: &Path) -> String {
    fn visit(path: &Path, out: &mut String) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push_str(&fs::read_to_string(path).unwrap());
                out.push('\n');
            }
        }
    }

    let mut out = String::new();
    visit(root, &mut out);
    out
}

#[test]
fn workspace_has_design_crates_and_no_legacy_product_crates() {
    let root = yaoshi_test::repo_root();
    let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    for member in [
        "crates/yaoshi",
        "crates/yaoshi-build",
        "crates/yaoshi-common",
        "crates/yaoshi-debian",
        "crates/yaoshi-image",
        "crates/yaoshi-initramfs",
        "crates/yaoshi-payload",
        "crates/yaoshi-screen",
        "crates/yaoshi-installer",
        "crates/yaoshi-dashboard",
        "crates/yaoshi-test",
    ] {
        assert!(cargo.contains(member), "missing workspace member {member}");
    }
    assert!(!cargo.contains("crates/yaoshi-tui"));
    assert!(!cargo.contains("crates/yaoshi-console"));
    assert!(!cargo.contains("crates/yaoshi-kernel"));
    assert!(!root.join("crates/yaoshi-tui").exists());
    assert!(!root.join("crates/yaoshi-console").exists());
    assert!(!root.join("crates/yaoshi-kernel").exists());
}

#[test]
fn repo_surface_uses_direct_commands_and_devshell_tools_only() {
    let root = yaoshi_test::repo_root();
    assert_eq!(
        fs::read_to_string(root.join(".envrc")).unwrap(),
        "use flake\n"
    );
    assert_eq!(
        fs::read_to_string(root.join(".cargo/config.toml")).unwrap(),
        "[build]\nrustc-wrapper = \"sccache\"\n\n[target.x86_64-unknown-linux-musl]\nlinker = \"x86_64-unknown-linux-musl-gcc\"\n"
    );
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    assert!(readme.contains("cargo run --locked -p yaoshi --"));
    assert!(readme.contains(".yaoshi/out/yaoshi.img"));
    assert!(readme.contains("Yaoshi Dashboard"));
    assert!(!readme.contains("Yaoshi Console"));
    assert!(!readme.contains("Section 16.2"));

    let flake = fs::read_to_string(root.join("flake.nix")).unwrap();
    for required in [
        "cargo-nextest",
        "qemu",
        "openssh",
        "e2fsprogs",
        "x86_64-unknown-linux-musl",
        "mmdebstrap",
        "sccache",
    ] {
        assert!(flake.contains(required), "flake missing {required}");
    }
    for forbidden in [
        "pkgs.gnumake",
        "pkgs.gcc",
        "pkgs.bc",
        "pkgs.bison",
        "pkgs.flex",
        "/usr/bin/mmdebstrap",
        "yaoshi-host-tools",
        "yaoshiBashDevshellAdapter",
    ] {
        assert!(
            !flake.contains(forbidden),
            "flake keeps forbidden host tool surface {forbidden}"
        );
    }
}

#[test]
fn rust_source_files_stay_bounded() {
    fn visit(path: &std::path::Path, oversized: &mut Vec<(String, usize)>) {
        let entries = fs::read_dir(path).unwrap_or_else(|err| {
            panic!("read source directory {}: {err}", path.display());
        });
        for entry in entries {
            let entry = entry.expect("read source directory entry");
            let path = entry.path();
            if path.is_dir() {
                visit(&path, oversized);
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && path
                    .components()
                    .any(|component| component.as_os_str() == "src")
            {
                let text = fs::read_to_string(&path)
                    .unwrap_or_else(|err| panic!("read Rust source {}: {err}", path.display()));
                let lines = text.lines().count();
                if lines > 1200 {
                    oversized.push((path.display().to_string(), lines));
                }
            }
        }
    }

    let root = yaoshi_test::repo_root();
    let mut oversized = Vec::new();
    visit(&root.join("crates"), &mut oversized);
    assert!(
        oversized.is_empty(),
        "Rust source files exceed 1200 lines: {oversized:?}"
    );
}

#[test]
fn repository_state_root_split_and_template_inventory_match_design() {
    let root = yaoshi_test::repo_root();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("git stdout is utf8")
    };
    assert_eq!(
        fs::read_to_string(root.join("keys/root.pub.example")).unwrap(),
        "# Put one or more OpenSSH public key lines in ./keys/root.pub.\n"
    );
    let gitignore = fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(gitignore.lines().any(|line| line == "/.yaoshi/"));
    assert!(
        !gitignore
            .lines()
            .any(|line| line.contains(".yaoshi") && line.starts_with('!'))
    );
    assert_eq!(
        git(&["check-ignore", "--no-index", ".yaoshi/yaoshi.toml"]),
        ".yaoshi/yaoshi.toml\n"
    );

    let inventory = [
        "config/yaoshi.toml.askama",
        "config/build-system.sh.askama",
        "config/first-boot.sh.askama",
        "debian/build-system-runner.sh.askama",
        "debian/root-export-helper.sh.askama",
        "debian/mke2fs.conf.askama",
        "overlay/prepare-launcher.sh.askama",
        "overlay/first-boot-launcher.sh.askama",
        "overlay/fstab.askama",
        "overlay/hostname.askama",
        "overlay/systemd/yaoshi-dashboard.service.askama",
        "overlay/systemd/yaoshi-root-shell.service.askama",
        "overlay/systemd/yaoshi-prepare.service.askama",
        "overlay/systemd/yaoshi-first-boot.service.askama",
        "overlay/systemd/ssh-order.conf.askama",
        "overlay/network/20-yaoshi-dhcp.network.askama",
        "overlay/ssh/10-yaoshi.conf.askama",
        "overlay/repart/20-yaoshi-root.conf.askama",
        "esp/boot/KERNEL-RELEASE.askama",
        "esp/config/HOSTNAME.askama",
        "esp/config/AUTHKEYS.askama",
        "esp/runtime/PREPARE.askama",
        "esp/loader/loader.conf.askama",
        "esp/loader/yaoshi.conf.askama",
        "installer-boot/boot/KERNEL-RELEASE.askama",
        "installer-boot/loader/loader.conf.askama",
        "installer-boot/loader/yaoshi-installer.conf.askama",
    ];
    for rel in inventory {
        assert!(
            root.join("crates/yaoshi-build/templates")
                .join(rel)
                .is_file(),
            "missing template {rel}"
        );
    }
    let templates_rs =
        fs::read_to_string(root.join("crates/yaoshi-build/src/templates.rs")).unwrap();
    assert_eq!(
        templates_rs.matches("#[derive(Template)]").count(),
        inventory.len()
    );
    assert_eq!(
        templates_rs.matches("escape = \"none\"").count(),
        inventory.len()
    );
    for banned in ["audit", "provenance", "report", "approval", "explanation"] {
        assert!(
            !templates_rs.contains(banned),
            "template context source contains banned field word {banned}"
        );
    }
    let build_rs = fs::read_to_string(root.join("crates/yaoshi-build/src/lib.rs")).unwrap();
    for banned in ["include_str!", "Tera", "Handlebars", "MiniJinja", "Liquid"] {
        assert!(
            !build_rs.contains(banned),
            "banned template mechanism {banned}"
        );
    }
}

#[test]
fn qemu_profile_selects_only_complete_flow() {
    let root = yaoshi_test::repo_root();
    let nextest = fs::read_to_string(root.join(".config/nextest.toml")).unwrap();
    assert!(nextest.contains("[profile.qemu]"));
    assert!(nextest.contains("test-threads = 1"));
    assert!(nextest.contains("default-filter = 'test(qemu::flow)'"));
    assert!(!nextest.contains("test(qemu::boot_virtio)"));
    assert!(!nextest.contains("test(qemu::boot_nvme)"));
    assert!(!nextest.contains("test(qemu::boot_ahci)"));
    assert!(!nextest.contains("test(qemu::boot_usb_storage_hid)"));
    assert!(!nextest.contains("default-filter = 'test(qemu::)'"));

    let this_file =
        fs::read_to_string(root.join("crates/yaoshi-test/tests/conformance.rs")).unwrap();
    assert!(this_file.contains("fn flow()"));
    assert!(this_file.contains("yaoshi_build::ensure_current_installer_image"));
    for required in [
        "installer-prepare-first",
        "installer-confirm-safe",
        "installer-confirm-install",
        "installer-write-start",
        "installer-write-copy-image",
        "installer-write-finalize",
        "dashboard-ready",
        "phase=image-ready",
        "phase=installer-media-detach",
        "phase=ssh-ready",
        "phase=dashboard-ready",
        "phase=target-ready-check",
        "YAOSHI_IMAGE_READY status=complete result=",
        "YAOSHI_QMP v=1 op=query-kvm",
    ] {
        assert!(this_file.contains(required), "qemu flow missing {required}");
    }
    let old_stamp_test = ["fn ", "published_image_stamp_is_current"].concat();
    assert!(!this_file.contains(&old_stamp_test));
    let old_installer = ["fn ", "installer()"].concat();
    let old_installed = ["fn ", "installed()"].concat();
    assert!(!this_file.contains(&old_installer));
    assert!(!this_file.contains(&old_installed));
}

#[test]
fn real_dashboard_check_contract_matches_design() {
    let root = yaoshi_test::repo_root();
    let host = fs::read_to_string(root.join("crates/yaoshi-test/src/bin/real-dashboard-check.rs"))
        .unwrap();
    let capture =
        fs::read_to_string(root.join("crates/yaoshi-test/src/bin/yaoshi-real-capture.rs")).unwrap();
    let qemu_harness =
        fs::read_to_string(root.join("crates/yaoshi-test/tests/conformance.rs")).unwrap();

    assert!(host.contains("real-dashboard-check accepts no command-line arguments"));
    assert!(host.contains("YAOSHI_REAL_HOST"));
    assert!(host.contains("YAOSHI_REAL_SSH_KEY"));
    assert!(host.contains("BatchMode=yes"));
    assert!(host.contains(".yaoshi/check/real-dashboard"));
    assert!(host.contains("/boot/YAOSHI/DASHBOARD/YAOSHI-DASHBOARD"));
    assert!(host.contains("/tmp/yaoshi-real-capture"));
    assert!(host.contains("systemctl restart yaoshi-dashboard.service"));
    assert!(host.contains("tty1.cells.txt"));
    assert!(host.contains("tty1.attrs.bin"));
    assert!(host.contains("sources.txt"));
    assert!(host.contains("evaluation.txt"));
    assert!(host.contains("framebuffer.info"));
    assert!(host.contains("framebuffer.raw"));
    assert!(host.contains("framebuffer.ppm"));
    assert!(!host.contains("qemu-system"));
    assert!(!host.contains("ensure_current_installer_image"));
    assert!(!host.contains("/dev/fb0"));

    assert!(capture.contains("/dev/vcs1"));
    assert!(capture.contains("/dev/vcsa1"));
    assert!(capture.contains("/dev/fb0"));
    assert!(capture.contains("framebuffer_capture=skipped"));
    for reason in [
        "fb0-absent",
        "fb0-unreadable",
        "fbioctl-unavailable",
        "fb-format-unsupported",
        "fb-geometry-inconsistent",
        "fb-grid-mismatch",
    ] {
        assert!(
            capture.contains(reason),
            "missing framebuffer skip reason {reason}"
        );
    }

    for forbidden in [
        ["screen", "dump"].concat(),
        ["screen", "-png"].concat(),
        [".", "png"].concat(),
        ["real-dashboard", "-strict"].concat(),
    ] {
        assert!(
            !qemu_harness.contains(&forbidden),
            "QEMU harness keeps forbidden surface {forbidden}"
        );
    }
}

#[test]
fn installed_rootfs_package_include_list_matches_design() {
    assert_eq!(
        yaoshi_common::DEBIAN_PACKAGES,
        &[
            "systemd",
            "systemd-sysv",
            "udev",
            "ca-certificates",
            "debian-archive-keyring",
            "bash",
            "systemd-repart",
            "systemd-resolved",
            "openssh-server",
            "e2fsprogs",
            "linux-image-amd64",
            "initramfs-tools",
            "systemd-boot-efi-amd64-signed",
        ]
    );
    for forbidden in ["grub-", "cloud-init", "kbd", "dkms", "linux-headers-"] {
        assert!(
            !yaoshi_common::DEBIAN_PACKAGES
                .iter()
                .any(|package| package.starts_with(forbidden)),
            "forbidden explicit package remains included: {forbidden}"
        );
    }
    assert!(!yaoshi_common::DEBIAN_PACKAGES.contains(&"dracut-install"));
}

#[test]
fn build_overlay_uses_dashboard_runtime_contract() {
    let root = yaoshi_test::repo_root();
    let build = rust_source_text(&root.join("crates/yaoshi-build/src"));
    let dashboard_service = fs::read_to_string(
        root.join("crates/yaoshi-build/templates/overlay/systemd/yaoshi-dashboard.service.askama"),
    )
    .unwrap();
    let prepare_service = fs::read_to_string(
        root.join("crates/yaoshi-build/templates/overlay/systemd/yaoshi-prepare.service.askama"),
    )
    .unwrap();
    assert!(build.contains("-p"));
    assert!(build.contains("yaoshi-dashboard"));
    assert!(build.contains("usr/bin/yaoshi-dashboard"));
    assert!(build.contains("etc/systemd/system/yaoshi-dashboard.service"));
    assert!(dashboard_service.contains("Description=Yaoshi Dashboard"));
    assert!(dashboard_service.contains("ExecStart=/usr/bin/yaoshi-dashboard"));
    assert!(dashboard_service.contains("RequiresMountsFor=/boot"));
    assert!(build.contains("/boot/YAOSHI/DASHBOARD/YAOSHI-DASHBOARD"));
    assert!(prepare_service.contains(
        "Before=yaoshi-first-boot.service yaoshi-dashboard.service yaoshi-root-shell.service ssh.service"
    ));
    assert!(!build.contains("/usr/lib/yaoshi-esp"));
    assert!(!build.contains("build.linux_source"));
    assert!(!build.contains("yaoshi_kernel"));
    assert!(!build.contains("yaoshi-console"));
    assert!(!build.contains("Operations Console"));
}

#[test]
fn build_graph_uses_virtual_target_graph_without_raw_or_write_plan() {
    let root = yaoshi_test::repo_root();
    let build = rust_source_text(&root.join("crates/yaoshi-build/src"));
    assert!(build.contains("installed-target.graph.json"));
    assert!(build.contains("pack-installed-system-payload-from-target-graph"));
    assert!(build.contains("VirtualInstalledTargetGraph"));
    assert!(build.contains("build_payload_from_virtual_target_graph"));
    assert!(build.contains(&[".yaoshi/work/image/installed-target", ".raw"].concat()));
    assert!(build.contains(&[".yaoshi/work/image/installed-target", ".write-plan"].concat()));
    for forbidden in [
        ["build-installed-target", "-raw"].concat(),
        ["derive-target", "-write-plan"].concat(),
        ["build_payload_from_target", "_plan"].concat(),
    ] {
        assert!(
            !build.contains(&forbidden),
            "build path keeps forbidden materialized target artifact {forbidden}"
        );
    }
    assert!(!build.contains("pack-installed-target-gpt-image"));
    assert!(!build.contains("compute-installed-target-required-map"));
    assert!(!build.contains("resize2fs -M"));
    assert!(!build.contains("debugfs -w"));

    let payload = rust_source_text(&root.join("crates/yaoshi-payload/src"));
    assert!(payload.contains("build_payload_from_virtual_target_graph"));
    for forbidden in [
        ["build_payload_from_target", "_plan"].concat(),
        ["read_target", "_write_plan"].concat(),
        ["installed_target", "_raw_sha256"].concat(),
        ["target_write", "_plan_sha256"].concat(),
        ["layout", "_graph_sha256"].concat(),
    ] {
        assert!(
            !payload.contains(&forbidden),
            "payload path keeps forbidden artifact digest or write plan API {forbidden}"
        );
    }
    assert!(!payload.contains("\"components\""));
    assert!(!payload.contains("build_payload_from_components"));
    assert!(!payload.contains("compute_required_map_from_components"));
}

#[test]
fn installer_initramfs_is_split_into_base_and_app_layers() {
    let root = yaoshi_test::repo_root();
    let common = fs::read_to_string(root.join("crates/yaoshi-common/src/constants.rs")).unwrap();
    assert!(common.contains("INSTALLER-BASE.CPIO.ZST"));
    assert!(common.contains("INSTALLER-APP.CPIO.ZST"));
    assert!(common.contains("initrd /YAOSHI/INSTALLER-BASE.CPIO.ZST"));
    assert!(common.contains("initrd /YAOSHI/INSTALLER-APP.CPIO.ZST"));
    assert!(!common.contains("YAOSHI/INSTALLER.CPIO.ZST"));

    let build = rust_source_text(&root.join("crates/yaoshi-build/src"));
    for required in [
        "pack-installer-base-initramfs-zstd",
        "pack-installer-app-initramfs-zstd",
        "base.tree",
        "app.tree",
        "INSTALLER_BASE_INITRAMFS_FAT_PATH",
        "INSTALLER_APP_INITRAMFS_FAT_PATH",
    ] {
        assert!(
            build.contains(required),
            "missing split initramfs path {required}"
        );
    }
    assert!(!build.contains("compress-installer-initramfs-zstd"));
    assert!(!build.contains("pack-installer-initramfs-newc"));
    assert!(!build.contains("initramfs/tree"));

    let initramfs = fs::read_to_string(root.join("crates/yaoshi-initramfs/src/lib.rs")).unwrap();
    assert!(initramfs.contains("render_installer_base_tree"));
    assert!(initramfs.contains("render_installer_app_tree"));
    assert!(!initramfs.contains("pub fn render_installer_tree"));
}

#[test]
fn screen_contract_uses_surface_envelopes_and_exact_styles() {
    assert_eq!(
        yaoshi_screen::size_class_for_surface(yaoshi_screen::Surface::Installer, 95, 28),
        yaoshi_screen::SizeClass::Unsupported
    );
    assert_eq!(
        yaoshi_screen::size_class_for_surface(yaoshi_screen::Surface::Installer, 96, 28),
        yaoshi_screen::SizeClass::Supported
    );
    assert_eq!(
        yaoshi_screen::size_class_for_surface(yaoshi_screen::Surface::Dashboard, 79, 24),
        yaoshi_screen::SizeClass::Unsupported
    );
    assert_eq!(
        yaoshi_screen::size_class_for_surface(yaoshi_screen::Surface::Dashboard, 80, 24),
        yaoshi_screen::SizeClass::Supported
    );
    assert_eq!(yaoshi_screen::CONSOLE_FONT_PROFILES.len(), 2);
    assert_eq!(
        yaoshi_screen::CONSOLE_FONT_PROFILES[0].name,
        "Terminus10x20"
    );

    let (fg, bg, modifier) =
        yaoshi_screen::semantic_style_definition(yaoshi_screen::SemanticStyle::Action);
    assert_eq!(format!("{fg:?}"), "White");
    assert_eq!(format!("{bg:?}"), "Reset");
    assert!(format!("{modifier:?}").contains("NONE"));
    let (_, _, gauge_critical) =
        yaoshi_screen::semantic_style_definition(yaoshi_screen::SemanticStyle::GaugeCritical);
    assert!(format!("{gauge_critical:?}").contains("NONE"));
}

#[test]
fn rendered_frames_follow_v0_0_1_grammar_and_ban_legacy_terms() {
    let display =
        yaoshi_screen::NegotiatedDisplay::fixture(yaoshi_screen::Surface::Installer, 112, 32);
    let prepare = yaoshi_screen::InstallerScene::Prepare(yaoshi_screen::PrepareModel {
        display,
        media_check_elapsed: Duration::ZERO,
        modules_state: "not-started".to_string(),
        media_state: "not-started".to_string(),
        payload_state: "not opened".to_string(),
        disk_inspection_state: "not inspected".to_string(),
        target_minimum_bytes: None,
        payload_planned_extent_bytes: None,
        payload_container_bytes: None,
        payload_zero_extent_bytes: None,
    });
    let installer = yaoshi_screen::render_installer_scene(&prepare, 112, 32);
    assert!(installer.cells_text.contains("Yaoshi Installer - Prepare"));
    assert!(
        installer
            .cells_text
            .contains("safe - no disk will be changed")
    );
    assert!(
        installer
            .cells_text
            .contains("Power off without opening a target disk.")
    );
    assert!(
        installer
            .cells_text
            .contains("status    preparing installer")
    );
    assert!(
        installer
            .cells_text
            .contains("current   starting input and storage drivers")
    );
    assert!(installer.cells_text.contains("[done]    console ready"));
    assert!(
        installer
            .cells_text
            .contains("[active]  input and storage drivers")
    );
    assert!(installer.cells_text.contains("[pending] target disk scan"));
    assert!(!installer.cells_text.contains("startup   "));
    assert!(!installer.cells_text.contains("modules   "));
    assert!(!installer.cells_text.contains("display   "));

    let dashboard = yaoshi_screen::dashboard::scene(&yaoshi_test::fixture_dashboard(), (184, 52));
    assert!(dashboard.cells_text.contains("Yaoshi Dashboard v0.0.1"));
    for required in [
        "System",
        "Compute",
        "Memory",
        "Storage",
        "Network",
        "Prepare applied  Root expanded  SSH ready  Net ready  Health ok",
        "Access    ssh root@192.0.2.10:22 ready - tty2 running",
    ] {
        assert!(
            dashboard.cells_text.contains(required),
            "missing {required}"
        );
    }
    assert!(!dashboard.cells_text.contains("Events"));
    assert!(!dashboard.cells_text.contains("History"));
    assert!(!dashboard.cells_text.contains("Display   "));
    assert!(!dashboard.cells_text.contains("Services  "));
    assert!(!dashboard.cells_text.contains("Read-only - tty2 root shell"));
    assert!(!dashboard.cells_text.contains("SSH       root@"));
    assert!(!dashboard.cells_text.contains("keys 1"));
    for frame in [&installer.cells_text, &dashboard.cells_text] {
        assert_no_banned_rendered_terms(frame);
    }
}

#[test]
fn renderer_design_profiles_match_current_review_contract() {
    let display =
        yaoshi_screen::NegotiatedDisplay::fixture(yaoshi_screen::Surface::Installer, 112, 32);
    let prepare = yaoshi_screen::InstallerScene::Prepare(yaoshi_screen::PrepareModel {
        display,
        media_check_elapsed: Duration::ZERO,
        modules_state: "not-started".to_string(),
        media_state: "not-started".to_string(),
        payload_state: "not opened".to_string(),
        disk_inspection_state: "not inspected".to_string(),
        target_minimum_bytes: None,
        payload_planned_extent_bytes: None,
        payload_container_bytes: None,
        payload_zero_extent_bytes: None,
    });
    for (frame, width, height) in [
        (
            yaoshi_screen::render_installer_scene(&prepare, 96, 28),
            96,
            28,
        ),
        (
            yaoshi_screen::render_installer_scene(&prepare, 128, 36),
            128,
            36,
        ),
        (
            yaoshi_screen::dashboard::scene(&yaoshi_test::fixture_dashboard(), (160, 44)),
            160,
            44,
        ),
        (
            yaoshi_screen::dashboard::scene(&yaoshi_test::fixture_dashboard(), (184, 52)),
            184,
            52,
        ),
    ] {
        let cells = yaoshi_screen::buffer_to_fixed_cells(&frame.buffer, width, height);
        let styles = yaoshi_screen::buffer_to_styles(&frame.buffer);
        assert_eq!(cells.lines().count(), height as usize);
        assert!(styles.lines().all(|line| line.contains(" bg=default ")));
    }
}

fn assert_no_banned_rendered_terms(frame: &str) {
    for banned in [
        "write gate",
        "write-gate",
        "source scan",
        "source-scan",
        "writable fd",
        "bounded reader",
        "syncfs",
        "global sync",
        "hidden candidates",
        "candidate status",
        "[FOCUS]",
        "[DANGER]",
        "[DISABLED]",
        "TaskBand",
        "EvidenceBand",
        "VisualizationBand",
        "DetailBand",
        "InstallerStatusGrid",
        "InstallerDecisionSplit",
        "InstallerProgressSplit",
        "InstallerFailureSplit",
        "ConsoleOverviewGrid",
        "ConsoleDetailSplit",
        "ConsoleEventList",
        "Overview",
        "PageLine",
        "ActionDock",
        "KeyHints",
        "history strip",
        "Services  ",
        "nominal",
        "normal",
        "healthy",
        "Yaoshi Console",
    ] {
        assert!(
            !frame
                .to_ascii_lowercase()
                .contains(&banned.to_ascii_lowercase()),
            "rendered frame contains banned term {banned}"
        );
    }
    for glyph in [
        "┌", "┐", "└", "┘", "─", "│", "├", "┤", "┴", "┬", "┼", "╭", "╮", "╰", "╯", "═", "║",
    ] {
        assert!(
            !frame.contains(glyph),
            "rendered frame contains border glyph {glyph}"
        );
    }
}

mod qemu {
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use yaoshi_common::{InstallerMbrLayout, markers};

    const WRITE_TASK_EVIDENCE: &[(&str, &str, &str)] = &[
        (
            "installer-write-start",
            "verify-target",
            "[active]  verify target",
        ),
        (
            "installer-write-copy-image",
            "copy-yaoshi-image",
            "[active]  copy Yaoshi image",
        ),
        (
            "installer-write-finalize",
            "finalize-writes",
            "[active]  finalize writes",
        ),
    ];

    #[test]
    fn flow() {
        run_installer_case("qemu-flow", BuildPolicy::RequireCurrentImage);
    }

    #[derive(Clone, Copy)]
    enum BuildPolicy {
        RequireCurrentImage,
    }

    fn run_installer_case(case_name: &str, build_policy: BuildPolicy) {
        let root = yaoshi_test::repo_root();
        let config = read_qemu_config(&root);
        let _ = case_name;
        let case_dir = root.join(".yaoshi/check/qemu");
        let _ = fs::remove_dir_all(&case_dir);
        fs::create_dir_all(&case_dir).expect("create qemu case dir");
        let evidence = Evidence::new(&case_dir);
        evidence.progress("image-ready", "checking", Some("mode=current-only"));
        let image_ready = ensure_built_artifact(&root, build_policy);
        evidence.image_ready(&image_ready.result);
        let built_installer = image_ready.path.clone();
        assert!(
            built_installer.is_file(),
            "qemu installer requires a generated installer image at {}",
            built_installer.display()
        );

        let mbr = yaoshi_image::parse_mbr(&built_installer).expect("installer MBR parses");
        assert_eq!(
            mbr.len(),
            2,
            "installer image must contain two MBR partitions"
        );
        let payload = &mbr[1];
        assert_eq!(payload.mbr_type, 0x83);
        let payload_offset = payload.start_lba as u64 * yaoshi_common::SECTOR_SIZE;
        let payload_size = payload.sector_count as u64 * yaoshi_common::SECTOR_SIZE;
        let expected_layout = InstallerMbrLayout {
            installed_system_bytes: payload_size,
        };
        assert_eq!(
            payload_offset,
            expected_layout.payload_partition().start_byte
        );
        assert_eq!(payload_size, expected_layout.payload_partition().byte_size);
        let payload_info = yaoshi_payload::validate_payload_region_metadata(
            &built_installer,
            payload_offset,
            payload_size,
        )
        .expect("installer partition 2 metadata parses as YAOSHI_PAYLOAD_V1");
        evidence.write_payload_summary(&payload_info);

        let installer = built_installer.clone();
        let vars = case_dir.join("OVMF_VARS.fd");
        fs::copy(&config.ovmf_vars_template, &vars).expect("copy OVMF vars template");
        fs::set_permissions(&vars, fs::Permissions::from_mode(0o600))
            .expect("make OVMF vars writable");
        evidence.write_firmware(&config, &vars);
        let target = case_dir.join("target.raw");
        File::create(&target)
            .expect("create target disk")
            .set_len(payload_info.target_minimum_bytes + 1024 * 1024 * 1024)
            .expect("size sparse target disk");

        preflight_qemu_host(&case_dir, payload_info.target_minimum_bytes);
        evidence.progress("preflight", "complete", None);
        let ssh_port = free_tcp_port();
        let serial_log = case_dir.join("serial.log");
        let stderr_log = case_dir.join("qemu.stderr.log");
        let qmp_sock = case_dir.join("qmp.sock");
        let qemu_args = vec![
            "-machine".to_string(),
            "q35".to_string(),
            "-accel".to_string(),
            "kvm".to_string(),
            "-cpu".to_string(),
            "host".to_string(),
            "-smp".to_string(),
            "8".to_string(),
            "-m".to_string(),
            "8192M".to_string(),
            "-display".to_string(),
            "vnc=127.0.0.1:0,to=99".to_string(),
            "-vga".to_string(),
            "none".to_string(),
            "-device".to_string(),
            "VGA,xres=2560,yres=1440,vgamem_mb=32".to_string(),
            "-device".to_string(),
            "qemu-xhci,id=xhci".to_string(),
            "-device".to_string(),
            "usb-kbd,bus=xhci.0,id=usb-kbd".to_string(),
            "-device".to_string(),
            "pcie-root-port,id=source-port,bus=pcie.0,chassis=1,slot=1".to_string(),
            "-device".to_string(),
            "pcie-root-port,id=target-port,bus=pcie.0,chassis=2,slot=2".to_string(),
            "-serial".to_string(),
            format!("file:{}", serial_log.display()),
            "-qmp".to_string(),
            format!("unix:{},server=on,wait=off", qmp_sock.display()),
            "-drive".to_string(),
            format!(
                "if=pflash,format=raw,readonly=on,file={}",
                config.ovmf_code.display()
            ),
            "-drive".to_string(),
            format!("if=pflash,format=raw,file={}", vars.display()),
            "-drive".to_string(),
            format!(
                "if=none,id=source,file={},format=raw,readonly=on,cache=unsafe,discard=ignore,detect-zeroes=off,aio=io_uring",
                installer.display()
            ),
            "-device".to_string(),
            "nvme,id=source-nvme,bus=source-port,drive=source,serial=YAOSHI_SOURCE_0,bootindex=1"
                .to_string(),
            "-drive".to_string(),
            format!(
                "if=none,id=target,file={},format=raw,cache=unsafe,discard=unmap,detect-zeroes=unmap,aio=io_uring",
                target.display()
            ),
            "-device".to_string(),
            "nvme,id=target-nvme,bus=target-port,drive=target,serial=YAOSHI_TARGET_0,bootindex=2"
                .to_string(),
            "-netdev".to_string(),
            format!("user,id=net0,hostfwd=tcp:127.0.0.1:{ssh_port}-:22"),
            "-device".to_string(),
            "virtio-net-pci,id=net-virtio,netdev=net0".to_string(),
        ];
        let mut child = Command::new("qemu-system-x86_64")
            .args(&qemu_args)
            .stdout(Stdio::null())
            .stderr(Stdio::from(
                File::create(&stderr_log).expect("create qemu stderr log"),
            ))
            .spawn()
            .expect("spawn qemu-system-x86_64");
        evidence.progress("qemu-start", "complete", None);

        let wanted_reboot = markers::controlled_exit("reboot");
        let boot_started = Instant::now();
        let first_frame_deadline = boot_started + Duration::from_secs(120);
        let media_discovery_deadline = boot_started + Duration::from_secs(240);
        let deadline = boot_started + Duration::from_secs(15 * 60);
        let mut last_report = Instant::now();
        let mut qmp = connect_qmp(&qmp_sock, Duration::from_secs(15));
        let qmp_started = Instant::now();
        let kvm = qmp_command_response(&mut qmp, r#"{"execute":"query-kvm"}"#)
            .expect("query KVM through QMP");
        let compact_kvm = kvm.split_whitespace().collect::<String>();
        assert!(
            compact_kvm.contains(r#""enabled":true"#) && compact_kvm.contains(r#""present":true"#),
            "QMP query-kvm did not report enabled KVM: {kvm}"
        );
        evidence.qmp("query-kvm", "", "enabled", qmp_started);
        let mut review = Review::new(&case_dir);
        let mut captured_preparing = false;
        let mut sent_flash_inputs = false;
        let mut captured_installing = false;
        let mut sent_complete_input = false;
        let mut captured_write_task_frames = Vec::new();
        loop {
            let serial = fs::read_to_string(&serial_log).unwrap_or_default();
            if serial.contains(markers::FIRST_FRAME_FLUSHED) && !captured_preparing {
                thread::sleep(Duration::from_millis(500));
                review.capture_screen_matching(
                    &mut qmp,
                    "installer-prepare-first",
                    "installer-prepare",
                    &serial_log,
                    Some("[pending] installer media 0.000 s / 10.000 s"),
                );
                evidence.progress("installer-first-frame", "complete", None);
                captured_preparing = true;
            }
            if serial.contains(markers::INSTALLATION_MEDIA_DISCOVERED) && !sent_flash_inputs {
                thread::sleep(Duration::from_millis(8_000));
                review.capture_screen_matching(
                    &mut qmp,
                    "installer-target",
                    "installer-target",
                    &serial_log,
                    None,
                );
                evidence.progress("installer-target", "complete", None);
                qmp_send_key(&mut qmp, "ret");
                thread::sleep(Duration::from_millis(700));
                review.capture_screen_matching(
                    &mut qmp,
                    "installer-confirm-safe",
                    "installer-confirm-safe",
                    &serial_log,
                    Some("[ Back ]"),
                );
                qmp_send_key(&mut qmp, "tab");
                thread::sleep(Duration::from_millis(250));
                review.capture_screen_matching(
                    &mut qmp,
                    "installer-confirm-erase",
                    "installer-confirm-erase",
                    &serial_log,
                    Some("[ Erase disk only"),
                );
                qmp_send_key(&mut qmp, "tab");
                thread::sleep(Duration::from_millis(250));
                review.capture_screen_matching(
                    &mut qmp,
                    "installer-confirm-install",
                    "installer-confirm-install",
                    &serial_log,
                    Some("[ Install Yaoshi"),
                );
                qmp_send_key(&mut qmp, "ret");
                sent_flash_inputs = true;
            }
            for (evidence_id, task, required_cell) in WRITE_TASK_EVIDENCE {
                if captured_write_task_frames.contains(evidence_id) {
                    continue;
                }
                let marker = format!(
                    "YAOSHI_MARK v=1 kind=installer event=write_task task={task} state=active"
                );
                if serial.contains(&marker) {
                    review.capture_screen_matching(
                        &mut qmp,
                        evidence_id,
                        "installer-write",
                        &serial_log,
                        Some(required_cell),
                    );
                    captured_write_task_frames.push(*evidence_id);
                }
            }
            if serial.contains(markers::TARGET_WRITE_STARTED) && !captured_installing {
                review.capture_screen_matching(
                    &mut qmp,
                    "installer-write-copy-image",
                    "installer-write",
                    &serial_log,
                    Some("[active]  copy Yaoshi image"),
                );
                evidence.progress(
                    "installer-write",
                    "copy-yaoshi-image",
                    Some("percent=0.000"),
                );
                captured_installing = true;
            }
            if serial.contains(markers::COMPLETE) && !sent_complete_input {
                thread::sleep(Duration::from_millis(500));
                review.capture_screen_matching(
                    &mut qmp,
                    "installer-done",
                    "installer-done",
                    &serial_log,
                    None,
                );
                evidence.progress("installer-done", "complete", None);
                let qmp_started = Instant::now();
                evidence.qmp("device-del", "id=source-nvme", "requested", qmp_started);
                qmp_device_del_and_wait(&mut qmp, "source-nvme", Duration::from_secs(60))
                    .expect("wait for source NVMe removal");
                evidence.qmp("device-deleted", "id=source-nvme", "complete", qmp_started);
                let qmp_started = Instant::now();
                let block_inventory =
                    qmp_command_response(&mut qmp, r#"{"execute":"query-block"}"#)
                        .expect("query block inventory after source detach");
                evidence.qmp("query-block", "", "complete", qmp_started);
                let query_block_return = block_inventory
                    .lines()
                    .find(|line| line.contains(r#""return""#))
                    .unwrap_or(&block_inventory);
                assert!(
                    !query_block_return.contains("source-nvme")
                        && !query_block_return.contains("YAOSHI_SOURCE_0"),
                    "QMP source NVMe device still appears after detach query-block return: {block_inventory}"
                );
                evidence.qmp("source-absent", "", "complete", qmp_started);
                evidence.progress("installer-media-detach", "complete", None);
                qmp_send_key(&mut qmp, "ret");
                let qmp_started = Instant::now();
                qmp_wait_for_event(&mut qmp, "RESET", Duration::from_secs(60))
                    .expect("observe QMP RESET after installer reboot");
                evidence.qmp("reset", "", "observed", qmp_started);
                sent_complete_input = true;
            }
            if flow_markers_complete(&serial, &wanted_reboot) {
                break;
            }
            if let Some(status) = child.try_wait().expect("poll qemu") {
                let latest_serial = fs::read_to_string(&serial_log).unwrap_or_default();
                if status.success() && flow_markers_complete(&latest_serial, &wanted_reboot) {
                    break;
                }
                panic!(
                    "qemu exited before complete flow markers, status={status}, serial:\n{}\nstderr:\n{}",
                    tail(&latest_serial, 120),
                    tail(&fs::read_to_string(&stderr_log).unwrap_or_default(), 120)
                );
            }
            if !captured_preparing && Instant::now() >= first_frame_deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "qemu::flow did not reach the first installer frame within 120s; qemu acceptance tests the current image and treats local startup stalls as failures, serial:\n{}\nstderr:\n{}",
                    tail(&serial, 120),
                    tail(&fs::read_to_string(&stderr_log).unwrap_or_default(), 120)
                );
            }
            if !sent_flash_inputs && Instant::now() >= media_discovery_deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "qemu::flow did not discover installer media within 240s; qemu acceptance tests the current image and treats local boot stalls as failures, serial:\n{}\nstderr:\n{}",
                    tail(&serial, 120),
                    tail(&fs::read_to_string(&stderr_log).unwrap_or_default(), 120)
                );
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "timed out waiting for qemu flow markers, serial:\n{}\nstderr:\n{}",
                    tail(&serial, 120),
                    tail(&fs::read_to_string(&stderr_log).unwrap_or_default(), 120)
                );
            }
            if last_report.elapsed() >= Duration::from_secs(30) {
                eprintln!(
                    "waiting for qemu flow markers; current serial tail:\n{}",
                    tail(&serial, 20)
                );
                last_report = Instant::now();
            }
            thread::sleep(Duration::from_millis(50));
        }
        validate_write_progress_contract(&serial_log, &payload_info);
        let key = root.join("crates/yaoshi-test/fixtures/ssh/root_ed25519");
        wait_for_serial_contains(
            &mut child,
            &serial_log,
            &stderr_log,
            "YAOSHI_MARK v=1 kind=display surface=dashboard",
            Duration::from_secs(10 * 60),
            "dashboard display marker",
        );
        evidence.progress("installed-boot", "complete", None);
        review.capture_screen_matching(&mut qmp, "dashboard-first", "dashboard", &serial_log, None);
        wait_for_ssh_true(
            ssh_port,
            &key,
            r#"/bin/bash -lc 'set -e; root_uuid=5c5c9f71-bc23-4f8d-80b2-6d4bb64a0f33; root_src=$(findmnt -no SOURCE /); root_pk=$(lsblk -no PKNAME "$root_src"); test -n "$root_pk"; test -z "$(systemctl --failed --no-legend)"; test "$(cat /var/lib/yaoshi/root-growth-state 2>/dev/null)" = expanded; grep -q root=PARTUUID=$root_uuid /proc/cmdline; ! grep -qw nomodeset /proc/cmdline; command -v ip >/dev/null; test "$(lsblk -no PARTUUID "$root_src" | tr A-Z a-z)" = "$root_uuid"; test "$(cat /sys/block/$root_pk/queue/logical_block_size)" = 512'"#,
            &mut child,
            &serial_log,
            &stderr_log,
            Duration::from_secs(10 * 60),
        );
        evidence.write("ssh.txt", "root SSH validation command succeeded\n");
        evidence.progress("ssh-ready", "complete", None);
        thread::sleep(Duration::from_secs(2));
        review.capture_screen_matching(&mut qmp, "dashboard-ready", "dashboard", &serial_log, None);
        review.write_required_serial_screen_files(&serial_log);
        evidence.write_screen_validation();
        evidence.progress("dashboard-ready", "complete", None);
        ssh_command(ssh_port, &key, "sync; systemctl poweroff --force --force")
            .expect("request VM poweroff");
        let status = wait_for_exit(&mut child, Duration::from_secs(90));
        assert!(
            status.success(),
            "qemu did not exit cleanly after poweroff: {status}"
        );
        assert_serial_has_no_kernel_panics(&serial_log);
        let installed = yaoshi_image::parse_gpt(&target, None)
            .expect("post-installed target GPT parses after grow-root");
        assert_eq!(
            installed.partitions.len(),
            2,
            "post-installed target GPT has two partitions"
        );
        assert_eq!(
            installed.partitions[0].name,
            yaoshi_common::INSTALLED_ESP_NAME
        );
        assert_eq!(
            installed.partitions[1].name,
            yaoshi_common::INSTALLED_ROOT_NAME
        );
        evidence.write_post_installed_target_validation(&target, &installed.partitions);
        evidence.write(
            "functional.txt",
            "installer flow, source removal, installed boot, SSH, dashboard, and final target validation succeeded\n",
        );
        evidence.progress("target-ready-check", "complete", None);
    }

    fn flow_markers_complete(serial: &str, wanted_reboot: &str) -> bool {
        serial.contains(markers::FIRST_FRAME_FLUSHED)
            && serial.contains(markers::INSTALLATION_MEDIA_DISCOVERED)
            && serial.contains(markers::TARGET_WRITE_STARTED)
            && serial.contains(markers::COMPLETE)
            && serial.contains(wanted_reboot)
    }

    struct QemuConfig {
        ovmf_code: PathBuf,
        ovmf_vars_template: PathBuf,
    }

    fn preflight_qemu_host(evidence: &Path, payload_target_minimum_bytes: u64) {
        let mut report = String::new();
        report.push_str(&format!("os {}\n", std::env::consts::OS));
        report.push_str(&format!("arch {}\n", std::env::consts::ARCH));
        assert_eq!(
            std::env::consts::OS,
            "linux",
            "ENVIRONMENT: QEMU host must be Linux"
        );
        assert_eq!(
            std::env::consts::ARCH,
            "x86_64",
            "ENVIRONMENT: QEMU host must be x86_64"
        );
        let kvm = fs::metadata("/dev/kvm").expect("ENVIRONMENT: /dev/kvm is missing");
        assert!(
            kvm.file_type().is_char_device(),
            "ENVIRONMENT: /dev/kvm is not a character device"
        );
        File::options()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .expect("ENVIRONMENT: current user cannot open /dev/kvm read-write");
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0);
        report.push_str(&format!("logical_cpus {cpus}\n"));
        assert!(
            cpus >= 8,
            "ENVIRONMENT: QEMU flow requires at least 8 logical CPUs"
        );
        let (mem_total, mem_available) = read_meminfo_kib();
        report.push_str(&format!("mem_total_kib {mem_total}\n"));
        report.push_str(&format!("mem_available_kib {mem_available}\n"));
        assert!(
            mem_total >= 16 * 1024 * 1024,
            "ENVIRONMENT: QEMU flow requires at least 16 GiB host RAM"
        );
        assert!(
            mem_available >= 10 * 1024 * 1024,
            "ENVIRONMENT: QEMU flow requires at least 10 GiB available RAM"
        );
        let scratch_available = statvfs_available_bytes(evidence);
        report.push_str(&format!("scratch_available_bytes {scratch_available}\n"));
        assert!(
            scratch_available >= payload_target_minimum_bytes + 4_294_967_296,
            "ENVIRONMENT: QEMU scratch filesystem lacks required free space"
        );
        let _ = report;
        assert!(
            resolve_command("qemu-system-x86_64").is_some(),
            "ENVIRONMENT: qemu-system-x86_64 is missing from PATH"
        );
        assert!(
            resolve_command("ssh").is_some(),
            "ENVIRONMENT: ssh is missing from PATH"
        );
    }

    fn read_meminfo_kib() -> (u64, u64) {
        let text = fs::read_to_string("/proc/meminfo").expect("ENVIRONMENT: read /proc/meminfo");
        let mut total = 0;
        let mut available = 0;
        for line in text.lines() {
            let mut fields = line.split_whitespace();
            match fields.next() {
                Some("MemTotal:") => total = fields.next().unwrap_or("0").parse().unwrap_or(0),
                Some("MemAvailable:") => {
                    available = fields.next().unwrap_or("0").parse().unwrap_or(0)
                }
                _ => {}
            }
        }
        (total, available)
    }

    fn statvfs_available_bytes(path: &Path) -> u64 {
        let raw = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
            .expect("scratch path contains no nul");
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        let rc = unsafe { libc::statvfs(raw.as_ptr(), stat.as_mut_ptr()) };
        assert_eq!(rc, 0, "ENVIRONMENT: statvfs failed for QEMU scratch");
        let stat = unsafe { stat.assume_init() };
        stat.f_bavail.saturating_mul(stat.f_frsize)
    }

    fn ensure_built_artifact(root: &Path, policy: BuildPolicy) -> yaoshi_build::ImageReady {
        let root = root.to_path_buf();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = match policy {
                BuildPolicy::RequireCurrentImage => {
                    yaoshi_build::require_current_installer_image_with_status(&root)
                        .map_err(|e| e.to_string())
                }
            };
            let _ = sender.send(result);
        });
        match receiver.recv_timeout(Duration::from_secs(60)) {
            Ok(Ok(image)) => image,
            Ok(Err(message)) => panic!("{message}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!(
                    "qemu::flow current-image check exceeded 60s; qemu acceptance must test an already current .yaoshi/out/yaoshi.img and must not hide build or resolver stalls"
                )
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("qemu::flow current-image check worker exited without a result")
            }
        }
    }

    fn read_qemu_config(root: &Path) -> QemuConfig {
        let _ = root;
        if let Some(config) = discover_qemu_firmware() {
            return config;
        }
        panic!("ENVIRONMENT: QEMU firmware not found through deterministic search order")
    }

    fn discover_qemu_firmware() -> Option<QemuConfig> {
        let qemu = resolve_command("qemu-system-x86_64")?;
        let canonical = fs::canonicalize(&qemu).unwrap_or(qemu.clone());
        for qemu_path in [qemu, canonical] {
            let Some(prefix) = qemu_path.parent().and_then(Path::parent) else {
                continue;
            };
            let code = first_readable_regular(&[
                prefix.join("share/qemu/edk2-x86_64-code.fd"),
                prefix.join("share/qemu/OVMF_CODE.fd"),
                PathBuf::from("/usr/share/OVMF/OVMF_CODE.fd"),
                PathBuf::from("/usr/share/edk2/x64/OVMF_CODE.fd"),
            ]);
            let vars_template = first_readable_regular(&[
                prefix.join("share/qemu/edk2-i386-vars.fd"),
                prefix.join("share/qemu/OVMF_VARS.fd"),
                PathBuf::from("/usr/share/OVMF/OVMF_VARS.fd"),
                PathBuf::from("/usr/share/edk2/x64/OVMF_VARS.fd"),
            ]);
            if let (Some(ovmf_code), Some(ovmf_vars_template)) = (code, vars_template) {
                return Some(QemuConfig {
                    ovmf_code,
                    ovmf_vars_template,
                });
            }
        }
        None
    }

    fn resolve_command(name: &str) -> Option<PathBuf> {
        let paths = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(name);
            if is_readable_regular(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    fn first_readable_regular(paths: &[PathBuf]) -> Option<PathBuf> {
        paths.iter().find(|path| is_readable_regular(path)).cloned()
    }

    fn is_readable_regular(path: &Path) -> bool {
        path.is_file() && File::open(path).is_ok()
    }

    fn connect_qmp(path: &Path, timeout: Duration) -> UnixStream {
        let deadline = Instant::now() + timeout;
        loop {
            match UnixStream::connect(path) {
                Ok(mut stream) => {
                    let mut greeting = [0u8; 4096];
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let _ = stream.read(&mut greeting);
                    qmp_write(&mut stream, r#"{"execute":"qmp_capabilities"}"#)
                        .expect("negotiate QMP capabilities");
                    return stream;
                }
                Err(_) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => panic!("connect QMP socket {}: {e}", path.display()),
            }
        }
    }

    fn qmp_send_key(stream: &mut UnixStream, key: &str) {
        qmp_hmp_command(stream, &format!("sendkey {key}")).expect("write QMP sendkey command");
    }

    fn qmp_hmp_command(stream: &mut UnixStream, command: &str) -> std::io::Result<()> {
        qmp_write(
            stream,
            &format!(
                r#"{{"execute":"human-monitor-command","arguments":{{"command-line":"{}"}}}}"#,
                json_escape(command)
            ),
        )
    }

    fn qmp_write(stream: &mut UnixStream, message: &str) -> std::io::Result<()> {
        qmp_command_response(stream, message).map(|_| ())
    }

    fn qmp_command_response(stream: &mut UnixStream, message: &str) -> std::io::Result<String> {
        stream
            .write_all(format!("{message}\n").as_bytes())
            .and_then(|_| stream.flush())?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut response = String::new();
        let mut buffer = [0u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "QMP socket closed before response",
                    ));
                }
                Ok(n) => {
                    response.push_str(&String::from_utf8_lossy(&buffer[..n]));
                    if response.contains(r#""return""#) {
                        return Ok(response);
                    }
                    if response.contains(r#""error""#) {
                        return Err(std::io::Error::other(format!(
                            "QMP command failed: {response}"
                        )));
                    }
                }
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) && Instant::now() < deadline =>
                {
                    continue;
                }
                Err(err) => return Err(err),
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("timed out waiting for QMP response to {message}"),
                ));
            }
        }
    }

    fn qmp_device_del_and_wait(
        stream: &mut UnixStream,
        id: &str,
        timeout: Duration,
    ) -> std::io::Result<String> {
        let message = format!(
            r#"{{"execute":"device_del","arguments":{{"id":"{}"}}}}"#,
            json_escape(id)
        );
        stream
            .write_all(format!("{message}\n").as_bytes())
            .and_then(|_| stream.flush())?;
        let deadline = Instant::now() + timeout;
        let mut response = String::new();
        let mut saw_return = false;
        let mut buffer = [0u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "QMP socket closed before source device removal",
                    ));
                }
                Ok(n) => {
                    response.push_str(&String::from_utf8_lossy(&buffer[..n]));
                    let compact = response.split_whitespace().collect::<String>();
                    if response.contains("DEVICE_UNPLUG_GUEST_ERROR") {
                        return Err(std::io::Error::other(response));
                    }
                    if compact.contains(r#""error""#) {
                        return Err(std::io::Error::other(format!(
                            "QMP device_del failed: {response}"
                        )));
                    }
                    saw_return |= compact.contains(r#""return""#);
                    if compact.contains(r#""event":"DEVICE_DELETED""#) {
                        return Ok(response);
                    }
                }
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) && Instant::now() < deadline =>
                {
                    continue;
                }
                Err(err) => {
                    return Err(if saw_return {
                        std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            format!(
                                "timed out waiting for DEVICE_DELETED after device_del: {response}"
                            ),
                        )
                    } else {
                        err
                    });
                }
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("timed out waiting for DEVICE_DELETED after device_del: {response}"),
                ));
            }
        }
    }

    fn qmp_wait_for_event(
        stream: &mut UnixStream,
        event: &str,
        timeout: Duration,
    ) -> std::io::Result<String> {
        let deadline = Instant::now() + timeout;
        let needle = format!(r#""event":"{event}""#);
        let mut response = String::new();
        let mut buffer = [0u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!("QMP socket closed before {event} event"),
                    ));
                }
                Ok(n) => {
                    response.push_str(&String::from_utf8_lossy(&buffer[..n]));
                    let compact = response.split_whitespace().collect::<String>();
                    if compact.contains(&needle) {
                        return Ok(response);
                    }
                    if compact.contains(r#""error""#) {
                        return Err(std::io::Error::other(format!(
                            "QMP error while waiting for {event}: {response}"
                        )));
                    }
                }
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) && Instant::now() < deadline =>
                {
                    continue;
                }
                Err(err) => return Err(err),
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("timed out waiting for QMP {event} event: {response}"),
                ));
            }
        }
    }

    fn json_escape(input: &str) -> String {
        let mut out = String::new();
        for ch in input.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out
    }

    struct Review {
        screens: PathBuf,
    }

    impl Review {
        fn new(case_dir: &Path) -> Self {
            let screens = case_dir.join("screens");
            fs::create_dir_all(&screens).expect("create screen frame evidence dir");
            Self { screens }
        }

        fn capture_screen_matching(
            &mut self,
            _qmp: &mut UnixStream,
            evidence_id: &str,
            source_screen: &str,
            serial_log: &Path,
            required_cell: Option<&str>,
        ) {
            let txt = self.screens.join(format!("{evidence_id}.txt"));
            let deadline = Instant::now() + Duration::from_secs(45);
            let mut last_error = String::new();
            while Instant::now() < deadline {
                let Some(frame) = latest_screen_frame(serial_log, source_screen, required_cell)
                else {
                    last_error = "serial frame was not reviewable yet".to_string();
                    thread::sleep(Duration::from_millis(100));
                    continue;
                };
                fs::write(txt, screen_evidence_text(evidence_id, frame))
                    .expect("write screen text evidence");
                return;
            }
            panic!(
                "QEMU screen evidence for {evidence_id} did not produce serial frame: {last_error}"
            );
        }

        fn write_required_serial_screen_files(&self, serial_log: &Path) {
            let required_screens = [
                (
                    "installer-prepare-first",
                    "installer-prepare",
                    "[pending] installer media 0.000 s / 10.000 s",
                ),
                ("installer-target", "installer-target", "target disks"),
                (
                    "installer-confirm-safe",
                    "installer-confirm-safe",
                    "[ Back ]",
                ),
                (
                    "installer-confirm-erase",
                    "installer-confirm-erase",
                    "[ Erase disk only",
                ),
                (
                    "installer-confirm-install",
                    "installer-confirm-install",
                    "[ Install Yaoshi",
                ),
                (
                    "installer-done",
                    "installer-done",
                    "Yaoshi Installer - Done",
                ),
                ("dashboard-first", "dashboard", "Yaoshi Dashboard v0.0.1"),
                ("dashboard-ready", "dashboard", "Yaoshi Dashboard v0.0.1"),
            ];
            for (evidence_id, source_screen, required_cell) in
                required_screens
                    .into_iter()
                    .chain(
                        WRITE_TASK_EVIDENCE
                            .iter()
                            .map(|(evidence_id, _, required_cell)| {
                                (*evidence_id, "installer-write", *required_cell)
                            }),
                    )
            {
                let txt = self.screens.join(format!("{evidence_id}.txt"));
                if txt.is_file() {
                    continue;
                }
                let frame = latest_screen_frame(serial_log, source_screen, Some(required_cell))
                    .unwrap_or_else(|| {
                        panic!(
                            "missing required serial screen frame {evidence_id} source={source_screen} required={required_cell:?}"
                        )
                    });
                fs::write(txt, screen_evidence_text(evidence_id, frame))
                    .expect("write required serial screen evidence");
            }
        }
    }

    #[derive(Debug, Clone)]
    struct ScreenFrameEvidence {
        screen: String,
        columns: u16,
        rows: u16,
        cells: Vec<String>,
        styles: Vec<String>,
    }

    fn screen_evidence_text(screen_id: &str, frame: ScreenFrameEvidence) -> String {
        let mut text = format!("screen {}\nstatus captured\n", screen_id);
        text.push_str(&format!(
            "frame_status captured\nframe_screen {}\ncolumns {}\nrows {}\n",
            frame.screen, frame.columns, frame.rows
        ));
        text.push_str("cells_begin\n");
        for line in frame.cells {
            text.push_str(&line);
            text.push('\n');
        }
        text.push_str("cells_end\nstyles_begin\n");
        for line in frame.styles {
            text.push_str(&line);
            text.push('\n');
        }
        text.push_str("styles_end\n");
        text
    }

    fn latest_screen_frame(
        serial_log: &Path,
        wanted_screen: &str,
        required_cell: Option<&str>,
    ) -> Option<ScreenFrameEvidence> {
        let serial = fs::read_to_string(serial_log).ok()?;
        let mut latest = None;
        let mut current: Option<ScreenFrameEvidence> = None;
        for line in serial.lines() {
            if let Some(rest) = line.strip_prefix("YAOSHI_SCREEN_FRAME_BEGIN ") {
                if let Some(frame) = current.take()
                    && screen_frame_matches(&frame, wanted_screen, required_cell)
                {
                    latest = Some(frame);
                }
                let screen = field(rest, "screen=")?.to_string();
                let columns = field(rest, "columns=")?.parse().ok()?;
                let rows = field(rest, "rows=")?.parse().ok()?;
                current = Some(ScreenFrameEvidence {
                    screen,
                    columns,
                    rows,
                    cells: Vec::new(),
                    styles: Vec::new(),
                });
                continue;
            }
            if let Some(rest) = line.strip_prefix("YAOSHI_SCREEN_FRAME_ROW ") {
                if let Some(frame) = current.as_mut()
                    && frame.screen == field(rest, "screen=").unwrap_or_default()
                    && let Some((_, text)) = rest.split_once(" text=")
                {
                    frame.cells.push(text.to_string());
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("YAOSHI_SCREEN_FRAME_STYLE ") {
                if let Some(frame) = current.as_mut()
                    && frame.screen == field(rest, "screen=").unwrap_or_default()
                    && let Some((_, text)) = rest.split_once(" text=")
                {
                    frame.styles.push(text.to_string());
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("YAOSHI_SCREEN_FRAME_END ")
                && let Some(frame) = current.take()
                && frame.screen == field(rest, "screen=").unwrap_or_default()
                && screen_frame_matches(&frame, wanted_screen, required_cell)
            {
                latest = Some(frame);
            }
        }
        if let Some(frame) = current.take()
            && screen_frame_matches(&frame, wanted_screen, required_cell)
        {
            latest = Some(frame);
        }
        latest
    }

    fn screen_frame_matches(
        frame: &ScreenFrameEvidence,
        wanted_screen: &str,
        required_cell: Option<&str>,
    ) -> bool {
        frame.screen == wanted_screen
            && frame.cells.len() == frame.rows as usize
            && !frame.styles.is_empty()
            && required_cell
                .is_none_or(|needle| frame.cells.iter().any(|cell| cell.contains(needle)))
    }

    fn validate_write_progress_contract(serial_log: &Path, payload: &yaoshi_payload::PayloadInfo) {
        let serial = fs::read_to_string(serial_log).expect("read serial log for progress contract");
        let mut count = 0u64;
        let mut previous_written = 0u64;
        let mut previous_zero = 0u64;
        let mut final_written = 0u64;
        let mut final_zero = 0u64;
        for line in serial
            .lines()
            .filter(|line| line.contains(" event=write_progress "))
        {
            count += 1;
            let extent = parse_pair(field(line, "extent=").expect("write_progress extent"));
            let written = parse_pair(field(line, "written=").expect("write_progress written"));
            let zeroed = parse_pair(field(line, "zeroed=").expect("write_progress zeroed"));
            assert_eq!(
                extent.0, count,
                "write_progress extent count must be sequential: {line}"
            );
            assert_eq!(
                extent.1, payload.extent_count,
                "write_progress extent total must match payload: {line}"
            );
            assert_eq!(
                written.1, payload.planned_extent_bytes,
                "write_progress written total must match payload: {line}"
            );
            assert_eq!(
                zeroed.1, payload.planned_zero_bytes,
                "write_progress zero total must match payload: {line}"
            );
            assert!(
                written.0 >= previous_written,
                "write_progress written current must be monotonic: {line}"
            );
            assert!(
                written.0 <= written.1,
                "write_progress written current exceeds total: {line}"
            );
            assert!(
                zeroed.0 >= previous_zero,
                "write_progress zero current must be monotonic: {line}"
            );
            assert!(
                zeroed.0 <= zeroed.1,
                "write_progress zero current exceeds total: {line}"
            );
            previous_written = written.0;
            previous_zero = zeroed.0;
            final_written = written.0;
            final_zero = zeroed.0;
        }
        assert!(
            count > 0 && count <= payload.extent_count,
            "write_progress line count must be bounded and nonzero"
        );
        assert_eq!(
            final_written, payload.planned_extent_bytes,
            "final write_progress written current must equal payload planned bytes"
        );
        assert_eq!(
            final_zero, payload.planned_zero_bytes,
            "final write_progress zero current must equal payload zero bytes"
        );
    }

    fn parse_pair(text: &str) -> (u64, u64) {
        let Some((left, right)) = text.split_once('/') else {
            panic!("expected current/total pair, got {text:?}");
        };
        (
            left.parse().expect("parse pair current"),
            right.parse().expect("parse pair total"),
        )
    }

    fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
        let start = line.find(name)? + name.len();
        let rest = &line[start..];
        Some(rest.split_once(' ').map_or(rest, |(value, _)| value))
    }

    struct Evidence {
        root: PathBuf,
        started: Instant,
    }

    impl Evidence {
        fn new(root: &Path) -> Self {
            for rel in [
                "qmp.log",
                "progress.log",
                "qemu.stderr.log",
                "sparse-write.txt",
                "post-installed-target.txt",
                "ssh.txt",
                "functional.txt",
                "screen.txt",
            ] {
                File::create(root.join(rel)).unwrap_or_else(|e| {
                    panic!("create QEMU evidence file {rel}: {e}");
                });
            }
            Self {
                root: root.to_path_buf(),
                started: Instant::now(),
            }
        }

        fn progress(&self, phase: &str, status: &str, extra: Option<&str>) {
            let elapsed_ms = self.started.elapsed().as_millis();
            let line = match extra {
                Some(extra) => {
                    format!(
                        "YAOSHI_QEMU_PROGRESS phase={phase} status={status} {extra} elapsed_ms={elapsed_ms}"
                    )
                }
                None => {
                    format!(
                        "YAOSHI_QEMU_PROGRESS phase={phase} status={status} elapsed_ms={elapsed_ms}"
                    )
                }
            };
            println!("{line}");
            self.append("progress.log", &format!("{line}\n"));
        }

        fn image_ready(&self, result: &yaoshi_build::ImageReadyResult) {
            let elapsed_ms = self.started.elapsed().as_millis();
            match result {
                yaoshi_build::ImageReadyResult::CurrentHit => {
                    self.append(
                        "progress.log",
                        &format!("YAOSHI_IMAGE_READY status=current-hit elapsed_ms={elapsed_ms}\n"),
                    );
                    self.progress("image-ready", "current-hit", None);
                    let elapsed_ms = self.started.elapsed().as_millis();
                    self.append(
                        "progress.log",
                        &format!(
                            "YAOSHI_IMAGE_READY status=complete result=current-hit elapsed_ms={elapsed_ms}\n"
                        ),
                    );
                }
                yaoshi_build::ImageReadyResult::Rebuilt { reason } => {
                    self.append(
                        "progress.log",
                        &format!(
                            "YAOSHI_IMAGE_READY status=current-miss reason={reason} elapsed_ms={elapsed_ms}\n"
                        ),
                    );
                    self.progress("image-ready", "rebuilt", None);
                    let elapsed_ms = self.started.elapsed().as_millis();
                    self.append(
                        "progress.log",
                        &format!(
                            "YAOSHI_IMAGE_READY status=complete result=rebuilt elapsed_ms={elapsed_ms}\n"
                        ),
                    );
                }
            }
        }

        fn qmp(&self, op: &str, pre_status: &str, status: &str, started: Instant) {
            let elapsed_ms = started.elapsed().as_millis();
            let spacer = if pre_status.is_empty() { "" } else { " " };
            self.append(
                "qmp.log",
                &format!(
                    "YAOSHI_QMP v=1 op={op}{spacer}{pre_status} status={status} elapsed_ms={elapsed_ms}\n"
                ),
            );
        }

        fn write_firmware(&self, config: &QemuConfig, vars: &Path) {
            self.append(
                "functional.txt",
                &format!(
                    "OVMF_CODE={}\nOVMF_VARS_TEMPLATE={}\nOVMF_VARS_RUN={}\n",
                    config.ovmf_code.display(),
                    config.ovmf_vars_template.display(),
                    vars.display()
                ),
            );
        }

        fn write_payload_summary(&self, payload: &yaoshi_payload::PayloadInfo) {
            self.append(
                "sparse-write.txt",
                &format!(
                    "extent_count {}\nplanned_extent_bytes {}\nplanned_zero_bytes {}\nblob_area_offset {}\nblob_area_len {}\n",
                    payload.extent_count,
                    payload.planned_extent_bytes,
                    payload.planned_zero_bytes,
                    payload.blob_area_offset,
                    payload.blob_area_len
                ),
            );
        }

        fn write_post_installed_target_validation(
            &self,
            target: &Path,
            parts: &[yaoshi_image::GptPartition],
        ) {
            let size = target.metadata().map(|m| m.len()).unwrap_or(0);
            let mut text = String::new();
            text.push_str(&format!("target_raw_bytes {size}\n"));
            text.push_str(&format!("gpt_partition_count {}\n", parts.len()));
            for part in parts {
                text.push_str(&format!(
                    "partition {} name {} type {} guid {} start_lba {} end_lba {}\n",
                    part.number,
                    part.name,
                    part.type_guid,
                    part.unique_guid,
                    part.start_lba,
                    part.end_lba
                ));
            }
            self.write("post-installed-target.txt", &text);
        }

        fn write_screen_validation(&self) {
            let mut text = String::new();
            for screen in [
                "installer-prepare-first",
                "installer-target",
                "installer-confirm-safe",
                "installer-confirm-erase",
                "installer-confirm-install",
                "installer-write-start",
                "installer-write-copy-image",
                "installer-write-finalize",
                "installer-done",
                "dashboard-first",
                "dashboard-ready",
            ] {
                let txt = self.root.join("screens").join(format!("{screen}.txt"));
                text.push_str(&format!("{screen} txt={}\n", txt.is_file()));
            }
            self.write("screen.txt", &text);
        }

        fn write(&self, rel: &str, text: &str) {
            fs::write(self.root.join(rel), text)
                .unwrap_or_else(|e| panic!("write QEMU evidence {rel}: {e}"));
        }

        fn append(&self, rel: &str, text: &str) {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.root.join(rel))
                .unwrap_or_else(|e| panic!("open QEMU evidence {rel}: {e}"));
            file.write_all(text.as_bytes())
                .unwrap_or_else(|e| panic!("append QEMU evidence {rel}: {e}"));
        }
    }

    fn wait_for_exit(
        child: &mut std::process::Child,
        timeout: Duration,
    ) -> std::process::ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait().expect("poll qemu after reboot") {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return child.wait().expect("wait killed qemu");
            }
            thread::sleep(Duration::from_millis(250));
        }
    }

    fn free_tcp_port() -> u16 {
        TcpListener::bind(("127.0.0.1", 0))
            .expect("bind ephemeral SSH host port")
            .local_addr()
            .expect("read ephemeral SSH host port")
            .port()
    }

    fn wait_for_ssh_true(
        port: u16,
        key: &Path,
        remote_command: &str,
        child: &mut std::process::Child,
        serial_log: &Path,
        stderr_log: &Path,
        timeout: Duration,
    ) {
        let deadline = Instant::now() + timeout;
        loop {
            if ssh_command(port, key, remote_command)
                .map(|status| status.success())
                .unwrap_or(false)
            {
                return;
            }
            if let Some(status) = child.try_wait().expect("poll installed qemu") {
                panic!(
                    "installed qemu exited before SSH checks passed, status={status}, serial:\n{}\nstderr:\n{}",
                    tail(&fs::read_to_string(serial_log).unwrap_or_default(), 120),
                    tail(&fs::read_to_string(stderr_log).unwrap_or_default(), 120)
                );
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "timed out waiting for installed SSH checks, serial:\n{}\nstderr:\n{}",
                    tail(&fs::read_to_string(serial_log).unwrap_or_default(), 120),
                    tail(&fs::read_to_string(stderr_log).unwrap_or_default(), 120)
                );
            }
            thread::sleep(Duration::from_secs(2));
        }
    }

    fn wait_for_serial_contains(
        child: &mut std::process::Child,
        serial_log: &Path,
        stderr_log: &Path,
        needle: &str,
        timeout: Duration,
        label: &str,
    ) {
        let deadline = Instant::now() + timeout;
        loop {
            let serial = fs::read_to_string(serial_log).unwrap_or_default();
            if serial.contains(needle) {
                return;
            }
            if let Some(status) = child.try_wait().expect("poll qemu for serial marker") {
                panic!(
                    "qemu exited before {label}, status={status}, serial:\n{}\nstderr:\n{}",
                    tail(&serial, 120),
                    tail(&fs::read_to_string(stderr_log).unwrap_or_default(), 120)
                );
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "timed out waiting for {label}, serial:\n{}\nstderr:\n{}",
                    tail(&serial, 120),
                    tail(&fs::read_to_string(stderr_log).unwrap_or_default(), 120)
                );
            }
            thread::sleep(Duration::from_millis(250));
        }
    }

    fn assert_serial_has_no_kernel_panics(serial_log: &Path) {
        let serial = fs::read_to_string(serial_log).unwrap_or_default();
        for needle in [
            "Kernel panic",
            "panic - not syncing",
            "Attempted to kill init",
        ] {
            assert!(
                !serial.contains(needle),
                "serial log {} contains kernel panic marker {needle:?}:\n{}",
                serial_log.display(),
                tail(&serial, 120)
            );
        }
    }

    fn ssh_command(
        port: u16,
        key: &Path,
        remote_command: &str,
    ) -> std::io::Result<std::process::ExitStatus> {
        Command::new("ssh")
            .args([
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=3",
                "-i",
            ])
            .arg(key)
            .arg("-p")
            .arg(port.to_string())
            .arg("root@127.0.0.1")
            .arg(remote_command)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    }

    fn tail(text: &str, lines: usize) -> String {
        let collected: Vec<_> = text.lines().rev().take(lines).collect();
        collected.into_iter().rev().collect::<Vec<_>>().join("\n")
    }
}
