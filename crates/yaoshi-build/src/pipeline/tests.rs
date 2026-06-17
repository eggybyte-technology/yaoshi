#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn build_source_text() -> String {
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
        visit(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
        out
    }

    #[test]
    fn build_phase_order_matches_design() {
        let names = BUILD_PHASE_ORDER
            .iter()
            .map(|phase| phase.name())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "bootstrap-instance-files",
                "load-config",
                "resolve-current-image",
                "resolve-build-intents",
                "resolve-cache-index-graph",
                "prepare-work-root-on-first-producer-miss",
                "resolve-foundation-branch",
                "resolve-root-branch",
                "resolve-runtime-binaries",
                "resolve-installed-runtime-branch",
                "resolve-installer-envelope-branch",
                "resolve-final-installer-image",
                "publish-current-image",
                "print-summary",
            ]
        );
    }

    #[test]
    fn installer_module_seeds_load_usb_keyboard_before_storage() {
        assert_eq!(
            INSTALLER_MODULE_SEEDS
                .iter()
                .take(2)
                .map(|seed| seed.name)
                .collect::<Vec<_>>(),
            vec!["xhci_pci", "xhci_hcd"]
        );
        let first_storage = INSTALLER_MODULE_SEEDS
            .iter()
            .position(|seed| seed.name == "usb_storage")
            .expect("usb_storage seed exists");
        let usbhid = INSTALLER_MODULE_SEEDS
            .iter()
            .position(|seed| seed.name == "usbhid")
            .expect("usbhid seed exists");
        let last_optional_host_controller = INSTALLER_MODULE_SEEDS
            .iter()
            .rposition(|seed| !seed.required)
            .expect("optional host controller seeds exist");
        assert!(last_optional_host_controller < usbhid);
        assert!(usbhid < first_storage);
    }

    #[test]
    fn hostname_validation_matches_label_rules() {
        assert!(validate_hostname("yaoshi-1").is_ok());
        assert!(validate_hostname("-bad").is_err());
        assert!(validate_hostname("bad-").is_err());
        assert!(validate_hostname("").is_err());
    }

    #[test]
    fn source_url_validation_accepts_http_and_rejects_query_or_fragment() {
        assert!(validate_http_url("https://mirrors.aliyun.com/debian", "sources.debian").is_ok());
        assert!(validate_http_url("https://example.invalid/debian", "sources.debian").is_ok());
        assert!(validate_http_url("ftp://example.invalid/debian", "sources.debian").is_err());
        assert!(validate_http_url("https://example.invalid/debian?q=1", "sources.debian").is_err());
        assert!(
            validate_http_url("https://example.invalid/debian#main", "sources.debian").is_err()
        );
    }

    #[test]
    fn installed_package_validation_deduplicates_in_order() {
        assert_eq!(
            validate_installed_packages(vec![
                "curl".to_string(),
                "git".to_string(),
                "curl".to_string(),
                "iproute2".to_string(),
            ])
            .unwrap(),
            vec!["curl", "git", "iproute2"]
        );
        assert!(validate_installed_packages(vec!["Bad".to_string()]).is_err());
        assert!(validate_installed_packages(vec!["a_b".to_string()]).is_err());
        assert!(validate_installed_packages(vec![format!("a{}", "b".repeat(128))]).is_err());
    }

    #[test]
    fn configured_script_validation_checks_suffix_size_and_nul() {
        let base = std::env::temp_dir().join(format!(
            "yaoshi-build-script-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        let ok = base.join("ok.sh");
        fs::write(&ok, b"#!/bin/bash\n").unwrap();
        assert_eq!(
            read_configured_script(&base, Some(Path::new("ok.sh")), "scripts.build_system")
                .unwrap(),
            b"#!/bin/bash\n"
        );
        fs::write(base.join("bad.txt"), b"#!/bin/bash\n").unwrap();
        assert!(
            read_configured_script(&base, Some(Path::new("bad.txt")), "scripts.build_system")
                .is_err()
        );
        fs::write(base.join("nul.sh"), b"a\0b").unwrap();
        assert!(
            read_configured_script(&base, Some(Path::new("nul.sh")), "scripts.build_system")
                .is_err()
        );
        fs::write(base.join("large.sh"), vec![b'x'; 1_048_577]).unwrap();
        assert!(
            read_configured_script(&base, Some(Path::new("large.sh")), "scripts.build_system")
                .is_err()
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn config_schema_rejects_unknown_tables_and_fields() {
        let unknown_root = r#"
[build]
enabled = true

[root]
ssh_public_key_files = ["root.pub"]
"#;
        assert!(toml::from_str::<RawConfig>(unknown_root).is_err());
        let unknown_field = r#"
[root]
ssh_public_key_files = ["root.pub"]
extra = true
"#;
        assert!(toml::from_str::<RawConfig>(unknown_field).is_err());
    }

    #[test]
    fn bootstrap_creates_missing_instance_files_from_askama_without_root_key() {
        let base = std::env::temp_dir().join(format!(
            "yaoshi-bootstrap-create-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        bootstrap_instance_files(&base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join(CONFIG_PATH)).unwrap(),
            templates::render_config_yaoshi_toml().unwrap()
        );
        assert_eq!(
            fs::read_to_string(base.join(".yaoshi/scripts/build-system.sh")).unwrap(),
            templates::render_config_build_system_sh().unwrap()
        );
        assert_eq!(
            fs::read_to_string(base.join(".yaoshi/scripts/first-boot.sh")).unwrap(),
            templates::render_config_first_boot_sh().unwrap()
        );
        assert!(!base.join("keys/root.pub").exists());
        assert_eq!(load_config(&base).unwrap_err().kind(), ExitKind::Config);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn bootstrap_never_overwrites_existing_instance_files() {
        let base = std::env::temp_dir().join(format!(
            "yaoshi-bootstrap-preserve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(base.join(".yaoshi/scripts")).unwrap();
        fs::write(base.join(CONFIG_PATH), "existing config").unwrap();
        fs::write(
            base.join(".yaoshi/scripts/build-system.sh"),
            "existing build",
        )
        .unwrap();
        fs::write(base.join(".yaoshi/scripts/first-boot.sh"), "existing boot").unwrap();
        bootstrap_instance_files(&base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join(CONFIG_PATH)).unwrap(),
            "existing config"
        );
        assert_eq!(
            fs::read_to_string(base.join(".yaoshi/scripts/build-system.sh")).unwrap(),
            "existing build"
        );
        assert_eq!(
            fs::read_to_string(base.join(".yaoshi/scripts/first-boot.sh")).unwrap(),
            "existing boot"
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn default_build_system_script_has_no_login_banner() {
        let script = templates::render_config_build_system_sh().unwrap();

        assert!(script.contains("/opt/yaoshi-local/README"));
        assert!(!script.contains("welcome.sh"));
        assert!(!script.contains("YAOSHI_WELCOME"));
        assert!(!script.contains("YAOSHI_WELCOME_SHOWN"));
        assert!(!script.contains("ROOT CONSOLE ONLINE"));
        assert!(!script.contains("/etc/profile.d/yaoshi-local.sh"));
        assert!(!script.contains("/root/.bashrc"));
        assert!(!script.contains("yaoe"));
        assert!(!script.contains("yaoshi-update"));
    }

    #[test]
    fn bootstrap_rejects_non_regular_instance_file_path() {
        let base = std::env::temp_dir().join(format!(
            "yaoshi-bootstrap-nonregular-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(base.join(CONFIG_PATH)).unwrap();
        assert_eq!(
            bootstrap_instance_files(&base).unwrap_err().kind(),
            ExitKind::Config
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn find_command_skips_non_executable_path_entry() {
        let base = std::env::temp_dir().join(format!(
            "yaoshi-build-find-command-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = base.join("first");
        let second = base.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let non_exec = first.join("cargo");
        let exec = second.join("cargo");
        fs::write(&non_exec, b"not executable").unwrap();
        fs::write(&exec, b"executable").unwrap();
        fs::set_permissions(&non_exec, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&exec, fs::Permissions::from_mode(0o755)).unwrap();
        let paths = std::env::join_paths([&first, &second]).unwrap();
        assert_eq!(find_command_in_path("cargo", &paths).unwrap(), exec);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn installed_overlay_unit_text_matches_design_labels() {
        let dashboard = templates::render_overlay_yaoshi_dashboard_service().unwrap();
        let prepare = templates::render_overlay_yaoshi_prepare_service().unwrap();
        let first_boot = templates::render_overlay_yaoshi_first_boot_service().unwrap();
        let root_shell = templates::render_overlay_yaoshi_root_shell_service().unwrap();
        let ssh_order = templates::render_overlay_ssh_order_conf().unwrap();
        let sshd = templates::render_overlay_sshd_config().unwrap();
        let esp_prepare = templates::render_esp_prepare().unwrap();
        let network = templates::render_overlay_network_dhcp().unwrap();
        let prepare_launcher = templates::render_overlay_prepare_launcher_sh().unwrap();
        assert!(esp_prepare.starts_with("#!/bin/bash\n"));
        assert!(esp_prepare.ends_with("exit 0\n"));
        assert!(prepare_launcher.contains("LABEL=YAOSHI_ESP /boot"));
        assert!(!esp_prepare.contains("LABEL=YAOSHI_ESP /boot"));
        assert!(prepare_launcher.contains("/run/yaoshi-prepare/PREPARE"));
        assert!(prepare_launcher.contains("YAOSHI_PREPARE_HOSTNAME="));
        assert!(esp_prepare.contains("YAOSHI_PREPARE_HOSTNAME"));
        assert!(esp_prepare.contains("127.0.1.1 ${host}"));
        assert!(esp_prepare.contains("/bin/hostname \"${host}\""));
        assert!(
            dashboard.contains(
                "After=local-fs.target yaoshi-prepare.service yaoshi-root-shell.service\n"
            )
        );
        assert!(!dashboard.contains("Requires=yaoshi-prepare.service\n"));
        assert!(dashboard.contains("StandardError=journal\n"));
        assert!(dashboard.contains("TTYVTDisallocate=no\n"));
        assert!(
            prepare.contains(
                "Before=yaoshi-first-boot.service yaoshi-dashboard.service yaoshi-root-shell.service ssh.service\n"
            )
        );
        assert!(first_boot.contains("TimeoutStartSec=75\n"));
        assert!(root_shell.contains("ExecStart=-/bin/bash --login\n"));
        assert!(root_shell.contains("Before=yaoshi-dashboard.service\n"));
        assert!(root_shell.contains("TimeoutStopSec=2\n"));
        assert!(root_shell.contains("StandardError=tty\n"));
        assert!(ssh_order.contains("After=yaoshi-prepare.service yaoshi-first-boot.service\n"));
        assert!(sshd.contains("PasswordAuthentication no\n"));
        assert!(prepare_launcher.contains("/run/yaoshi-prepare/PREPARE"));
        assert!(esp_prepare.contains("YAOSHI_PREPARE_AUTHKEYS"));
        assert_eq!(
            network,
            "[Match]\nName=*\nType=ether\n\n[Network]\nDHCP=yes\nIPv6AcceptRA=yes\n\n[DHCPv4]\nClientIdentifier=mac"
        );
    }

    #[test]
    fn package_manifest_validation_requires_sorted_tabular_installed_rows() {
        let base = std::env::temp_dir().join(format!(
            "yaoshi-build-package-manifest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        let good = base.join("good.manifest");
        let bad = base.join("bad.manifest");
        fs::write(
            &good,
            b"bash 5.2.37-2 amd64 installed\nca-certificates 20250419 all installed\ndebian-archive-keyring 2025.1 all installed\ne2fsprogs 1.47.2-3 amd64 installed\ninitramfs-tools 0.148.4 all installed\niproute2 6.15.0-1 amd64 installed\nlinux-image-amd64 6.12.90-2 amd64 installed\nopenssh-client 1:10.0p1-7 amd64 installed\nopenssh-server 1:10.0p1-7 amd64 installed\nprocps 2:4.0.4-9 amd64 installed\nsudo 1.9.16p2-3 amd64 installed\nsystemd 257.13-1 amd64 installed\nsystemd-boot-efi-amd64-signed 257.13-1 amd64 installed\nsystemd-repart 257.13-1 amd64 installed\nsystemd-resolved 257.13-1 amd64 installed\nsystemd-sysv 257.13-1 amd64 installed\nudev 257.13-1 amd64 installed\n",
        )
        .unwrap();
        fs::write(&bad, b"bash 5.2.37-2 amd64\n").unwrap();
        let required = [
            DEBIAN_PACKAGES,
            &["iproute2", "openssh-client", "procps", "sudo"],
        ]
        .concat()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        assert!(package_manifest_is_valid(&good, &required).unwrap());
        assert!(!package_manifest_is_valid(&bad, &required).unwrap());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn root_overlay_tar_contains_dashboard_symlink_without_authorized_keys_file() {
        let base = std::env::temp_dir().join(format!(
            "yaoshi-overlay-tar-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        render_installed_overlay_tree(&base).unwrap();
        assert!(base.join("usr/lib/yaoshi/prepare-launcher").is_file());
        assert!(base.join("usr/bin/yaoshi-dashboard").is_symlink());
        assert!(base.join("etc/resolv.conf").is_symlink());
        assert!(!base.join("root/.ssh/authorized_keys").exists());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn current_image_fingerprint_includes_overlay_source_bytes() {
        let digest = root_overlay_source_bytes().unwrap();
        assert!(digest.contains("ExecStart=/usr/lib/yaoshi/prepare-launcher"));
        assert!(digest.contains("Description=Prepare Yaoshi installed runtime"));
        assert!(digest.contains("PermitRootLogin yes"));
        assert!(digest.contains("etc/resolv.conf -> /run/systemd/resolve/stub-resolv.conf"));
        assert!(digest.contains("multi-user.target.wants/systemd-resolved.service"));
        assert!(!digest.contains("yaoshi-grow-root.service"));
        assert!(!digest.contains("yaoshi-apply-config.service"));
    }

    #[test]
    fn current_image_stamp_uses_five_design_fingerprints() {
        let fingerprints = CurrentImageFingerprints {
            foundation: "0".repeat(64),
            installed_root: "1".repeat(64),
            installed_runtime: "2".repeat(64),
            installer_envelope: "3".repeat(64),
            published_image: "4".repeat(64),
        };
        let text = fingerprints.stamp_text();
        assert_eq!(
            text,
            format!(
                "foundation-fingerprint={}\ninstalled-root-fingerprint={}\ninstalled-runtime-fingerprint={}\ninstaller-envelope-fingerprint={}\npublished-image-fingerprint={}\n",
                fingerprints.foundation,
                fingerprints.installed_root,
                fingerprints.installed_runtime,
                fingerprints.installer_envelope,
                fingerprints.published_image
            )
        );
        assert_eq!(parse_current_image_stamp(&text), Some(fingerprints));
        assert!(parse_current_image_stamp("artifact-input-fingerprint=0000\n").is_none());
        assert!(parse_current_image_stamp(text.trim_end_matches('\n')).is_none());
    }

    #[test]
    fn final_installer_output_is_composite_manifest_not_raw_file_phase() {
        let base = std::env::temp_dir().join(format!(
            "yaoshi-final-composite-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        let boot = base.join("installer-boot.fat32");
        let payload = base.join("installed-system.ypayload");
        File::create(&boot)
            .unwrap()
            .set_len(INSTALLER_BOOT_SIZE_BYTES)
            .unwrap();
        {
            let mut file = File::create(&payload).unwrap();
            file.write_all(PAYLOAD_MAGIC).unwrap();
            file.set_len(4096).unwrap();
        }
        let layout = InstallerMbrLayout {
            installed_system_bytes: 4096,
        };
        let payload_digest = sha256_file_hex(&payload).unwrap();
        let (_logical, manifest) =
            final_installer_composite_manifest(&boot, &payload, &payload_digest, &layout).unwrap();
        let value: Value = serde_json::from_slice(&manifest).unwrap();
        assert_eq!(value["grammar"], COMPOSITE_FILE_GRAMMAR);
        assert_eq!(value["kind"], "composite-file");
        assert_eq!(
            value["segments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|segment| segment["kind"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["inline-bytes", "zero", "file-ref", "file-ref", "zero"]
        );

        let source = build_source_text();
        assert!(source.contains("pack-final-installer-mbr-composite"));
        assert!(!source.contains(&["pack-final-installer", "-mbr-image"].concat()));
        assert!(!source.contains(&["pack-installer-base-initramfs", "-newc\","].concat()));
        assert!(!source.contains(&["pack-installer-app-initramfs", "-newc\","].concat()));
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn debian_package_root_key_excludes_post_foundation_inputs() {
        let source = build_source_text();
        assert!(source.contains("\"phase\": \"resolve-debian-package-root\""));
        assert!(source.contains("\"boot_export_contract\""));
        assert!(!source.contains("\"root_overlay_tar_digest\""));
        assert!(!source.contains("\"installed_root_size_bytes\": installed_root_size_bytes"));
        assert!(!source.contains(&["render_installed_root", "_overlay_tar"].concat()));
    }
}
