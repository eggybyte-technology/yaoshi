#[cfg(test)]
mod renderer_review_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_path_buf()
    }

    #[test]
    fn font_profiles_match_design_order() {
        assert_eq!(
            CONSOLE_FONT_PROFILES.map(|profile| profile.name),
            ["Terminus10x20", "Terminus12x24"]
        );
    }

    #[test]
    fn display_negotiation_selects_12x24_at_1440_or_above() {
        let observations = [
            (
                CONSOLE_FONT_PROFILES[0],
                Ok(DisplayObservation::new(256, 72, Some(2560), Some(1440))),
            ),
            (
                CONSOLE_FONT_PROFILES[1],
                Ok(DisplayObservation::new(213, 60, Some(2560), Some(1440))),
            ),
        ];
        let DisplayNegotiation::Supported(display) =
            negotiate_display_from_observations(Surface::Installer, &observations)
        else {
            panic!("display should be supported");
        };
        assert_eq!(display.font_profile, "Terminus12x24");
        assert_eq!((display.columns, display.rows), (213, 60));
    }

    #[test]
    fn display_negotiation_uses_10x20_below_1440() {
        let observations = [
            (
                CONSOLE_FONT_PROFILES[0],
                Ok(DisplayObservation::new(102, 38, Some(1020), Some(768))),
            ),
            (
                CONSOLE_FONT_PROFILES[1],
                Ok(DisplayObservation::new(85, 32, Some(1020), Some(768))),
            ),
        ];
        let DisplayNegotiation::Supported(display) =
            negotiate_display_from_observations(Surface::Installer, &observations)
        else {
            panic!("display should be supported");
        };
        assert_eq!(display.font_profile, "Terminus10x20");
        assert_eq!((display.columns, display.rows), (102, 38));
    }

    #[test]
    fn display_negotiation_allows_tty_grid_without_framebuffer_pixels() {
        let observations = [
            (
                CONSOLE_FONT_PROFILES[0],
                Ok(DisplayObservation::new(112, 32, None, None)),
            ),
            (
                CONSOLE_FONT_PROFILES[1],
                Ok(DisplayObservation::new(93, 26, None, None)),
            ),
        ];
        let DisplayNegotiation::Supported(display) =
            negotiate_display_from_observations(Surface::Dashboard, &observations)
        else {
            panic!("display should be supported from tty grid alone");
        };
        assert_eq!(display.font_profile, "Terminus10x20");
        assert_eq!((display.columns, display.rows), (112, 32));
        assert_eq!(
            (display.framebuffer_width, display.framebuffer_height),
            (1120, 640)
        );
    }

    #[test]
    fn dashboard_negotiation_allows_current_80x25_console() {
        let DisplayNegotiation::Supported(display) = negotiate_display_from_current_console(
            Surface::Dashboard,
            DisplayObservation::new(80, 25, None, None),
        ) else {
            panic!("dashboard should support the current 80x25 console");
        };
        assert_eq!(display.font_profile, "CurrentConsole");
        assert_eq!((display.columns, display.rows), (80, 25));
    }

    #[test]
    fn current_console_negotiation_uses_grid_not_terminus_cell_floor() {
        let DisplayNegotiation::Supported(display) = negotiate_display_from_current_console(
            Surface::Dashboard,
            DisplayObservation::new(80, 25, Some(640), Some(400)),
        ) else {
            panic!("current console fallback should trust the active grid");
        };
        assert_eq!(display.font_profile, "CurrentConsole");
        assert_eq!((display.columns, display.rows), (80, 25));

        assert!(matches!(
            negotiate_display_for_profile(
                Surface::Dashboard,
                CONSOLE_FONT_PROFILES[0],
                DisplayObservation::new(80, 25, Some(640), Some(400)),
            ),
            DisplayNegotiation::Unsupported(DisplayNegotiationFailure {
                reason: DisplayNegotiationReason::FontApplicationNotEffective,
                ..
            })
        ));
    }

    #[test]
    fn vendored_terminus_fonts_parse_and_match_hashes() {
        let font_root = repo_root().join("crates/yaoshi-screen/fonts/terminus");
        assert!(font_root.join("OFL-1.1.txt").is_file());
        let sums = fs::read_to_string(font_root.join("SHA256SUMS")).unwrap();
        for (file_name, width, height) in [
            ("TerminusRegular10x20.psf", 10, 20),
            ("TerminusBold10x20.psf", 10, 20),
            ("TerminusRegular12x24.psf", 12, 24),
            ("TerminusBold12x24.psf", 12, 24),
        ] {
            let path = font_root.join(file_name);
            let bytes = fs::read(&path).unwrap();
            let parsed = parse_psf2_for_test(&bytes);
            assert_eq!(parsed.width, width);
            assert_eq!(parsed.height, height);
            assert!(parsed.char_count >= 128);
            for byte in 0x20u8..=0x7e {
                let glyph = parsed.glyph(byte);
                assert!(glyph.contains(&0) || byte == b' ');
                if byte != b' ' {
                    assert!(glyph.iter().any(|b| *b != 0), "blank glyph {byte}");
                }
            }
            let digest = Sha256::digest(&bytes);
            let expected = format!("{digest:x}  {file_name}");
            assert!(
                sums.lines().any(|line| line == expected),
                "SHA256SUMS missing {expected}"
            );
        }
    }

    #[test]
    fn dashboard_rows_do_not_duplicate_storage_capacity_or_footer_ssh() {
        let frame = dashboard::scene(&fixture_dashboard(), (184, 52));
        assert!(!frame.cells_text.contains("Root FS"));
        assert!(frame.cells_text.contains("System"));
        assert!(frame.cells_text.contains("Compute"));
        assert!(frame.cells_text.contains("Memory"));
        assert!(frame.cells_text.contains("Storage"));
        assert!(frame.cells_text.contains("Network"));
        assert!(
            frame
                .cells_text
                .contains("Access    ssh root@192.0.2.10:22 ready - tty2 running")
        );
        assert!(!frame.cells_text.contains("SSH       root@"));
        assert!(!frame.cells_text.contains("keys 1"));
        assert!(!frame.cells_text.contains("FooterLine"));
        assert!(!frame.cells_text.contains("Read-only - tty2 root shell"));
        assert!(
            frame
                .cells_text
                .contains("Signals   thermal unavailable - kernel none")
        );
        assert!(
            !frame
                .cells_text
                .contains("unavailable - unavailable unavailable")
        );
        assert!(!frame.cells_text.contains("Events"));
        assert!(!frame.cells_text.contains("History"));
    }

    #[test]
    fn dashboard_boot_row_shows_prepare_state() {
        let frame = dashboard::scene(&fixture_dashboard(), (184, 52));
        assert!(
            frame
                .cells_text
                .contains("Boot      kernel 6.12.0-yaoshi - prepare applied - growth expanded")
        );
        assert!(
            frame
                .cells_text
                .contains("Prepare applied  Root expanded  SSH ready  Net ready  Health ok")
        );
    }

    #[test]
    fn dashboard_inventory_is_bounded_at_supported_floor() {
        for (cpus, disks, interfaces) in [
            (1, 1, 1),
            (256, 1, 1),
            (8, 128, 1),
            (8, 1, 64),
            (256, 128, 64),
        ] {
            let frame = dashboard::scene(
                &fixture_dashboard_inventory(cpus, disks, interfaces),
                (96, 28),
            );
            assert_dashboard_frame_shape(&frame, 96, 28);
            assert!(frame.cells_text.contains("CPU       Yaoshi CPU"));
            assert!(frame.cells_text.contains(&format!("{cpus} CPUs")));
            assert!(frame.cells_text.contains("max cpu"));
            assert!(frame.cells_text.contains(&format!(
                "Disks     {disks} disks - root /dev/vda 5.000 GiB virtio"
            )));
            assert!(frame.cells_text.contains(&format!(
                "NICs      {interfaces} ifaces - route eth0 - access eth0 - attention 0"
            )));
            if cpus > 1 {
                assert!(!frame.cells_text.contains("cpu255"));
            }
            if disks > 1 {
                assert!(!frame.cells_text.contains("/dev/vd127"));
            }
            if interfaces > 1 {
                assert!(!frame.cells_text.contains("eth63   "));
            }
            assert!(
                frame
                    .cells_text
                    .contains("Prepare applied  Root expanded  SSH ready  Net ready  Health ok")
            );
        }
    }

    #[test]
    fn dashboard_extra_rows_follow_bounded_allocation_order() {
        let frame = dashboard::scene(&fixture_dashboard_inventory(256, 128, 64), (160, 44));
        let region = |name: &str| {
            frame
                .region_tree
                .iter()
                .find(|region| region.name == name)
                .unwrap()
                .rect
        };
        assert_eq!(region("Storage").height, 12);
        assert_eq!(region("Network").height, 13);
        assert_eq!(region("Compute").height, 7);
        assert!(frame.cells_text.contains("... "));
        assert!(frame.cells_text.contains("more CPU cores not shown"));
        assert!(frame.cells_text.contains("more disks not shown"));
        assert!(frame.cells_text.contains("more interfaces not shown"));
        assert_dashboard_frame_shape(&frame, 160, 44);
    }

    #[test]
    fn dashboard_floor_allocates_one_storage_and_network_unit() {
        let frame = dashboard::scene(&fixture_dashboard_inventory(8, 4, 4), (96, 28));
        assert!(frame.cells_text.contains("* /dev/vda 5.000 GiB"));
        assert!(frame.cells_text.contains("  sn unavailable - fs "));
        assert!(frame.cells_text.contains("eth0   route+access physical up"));
        assert!(frame.cells_text.contains("       mac unavailable - rx "));
        assert!(!frame.cells_text.contains("cpu1    "));
        assert_dashboard_frame_shape(&frame, 96, 28);
    }

    #[test]
    fn dashboard_machine_identity_uses_real_board_when_product_is_placeholder() {
        let mut snapshot = fixture_dashboard();
        snapshot.system_vendor = "ASUSTeK COMPUTER INC.".to_string();
        snapshot.product_name = "System Product Name".to_string();
        snapshot.board_vendor = "ASUSTeK COMPUTER INC.".to_string();
        snapshot.board_name = "PRIME B650M-A WIFI II".to_string();
        let frame = dashboard::scene(&snapshot, (160, 44));
        assert!(
            frame
                .cells_text
                .contains("Machine   ASUSTeK COMPUTER INC. PRIME B650M-A WIFI II - fw")
        );
        assert!(!frame.cells_text.contains("System Product Name"));
        assert!(!frame.cells_text.contains(" - board PRIME B650M-A WIFI II"));
    }

    #[test]
    fn dashboard_warning_and_critical_inventory_rows_are_prioritized() {
        let mut snapshot = fixture_dashboard_inventory(8, 4, 3);
        snapshot.core_rows[1].used_percent = "96.000%".to_string();
        snapshot.core_rows[2].used_percent = "86.000%".to_string();
        snapshot.disk_rows[1].fs = "9.500 GiB/10.000 GiB (95.000%)".to_string();
        snapshot.disk_rows[2].fs = "8.500 GiB/10.000 GiB (85.000%)".to_string();
        snapshot.network_error_state = "critical".to_string();
        snapshot.network_rows[1].alert_state = "critical".to_string();
        snapshot.network_rows[2].alert_state = "warning".to_string();
        snapshot.interface_attention_count = "2".to_string();
        let frame = dashboard::scene(&snapshot, (160, 44));
        let cpu1 = frame.cells_text.find("cpu1").unwrap();
        let cpu2 = frame.cells_text.find("cpu2").unwrap();
        assert!(cpu1 < cpu2, "critical core must sort before warning core");
        let root = frame.cells_text.find("* /dev/vda").unwrap();
        let disk1 = frame.cells_text.find("  /dev/vd001").unwrap();
        let disk2 = frame.cells_text.find("  /dev/vd002").unwrap();
        assert!(root < disk1);
        assert!(disk1 < disk2, "critical disk must sort before warning disk");
        assert!(frame.cells_text.contains("attention 2"));
    }

    fn assert_dashboard_frame_shape(frame: &RenderedFrame, columns: u16, rows: u16) {
        let cells = buffer_to_fixed_cells(&frame.buffer, columns, rows);
        let lines = cells.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), rows as usize);
        for line in lines {
            assert_eq!(line.len(), columns as usize);
            assert!(line.bytes().all(|byte| (0x20..=0x7e).contains(&byte)));
        }
        for region in &frame.region_tree {
            assert!(region.rect.x + region.rect.width <= columns);
            assert!(region.rect.y + region.rect.height <= rows);
        }
    }

    #[test]
    fn stopped_scene_renders_required_write_state_row() {
        let no_writes = render_installer_scene(
            &InstallerScene::Stopped(StoppedModel {
                reason: FailureReason::PayloadValidationFailed,
                failed_step_name: "payload validation".to_string(),
                affected_disk: None,
                failure_write_state: FailureWriteState::NoTargetWrites,
                auto_poweroff_seconds: None,
            }),
            128,
            36,
        );
        assert!(
            no_writes
                .cells_text
                .contains("state     no target writes started")
        );

        let write_started = render_installer_scene(
            &InstallerScene::Stopped(StoppedModel {
                reason: FailureReason::TargetWriteFailed,
                failed_step_name: "copy Yaoshi image".to_string(),
                affected_disk: Some(PathBuf::from("/dev/vda")),
                failure_write_state: FailureWriteState::TargetWriteStarted,
                auto_poweroff_seconds: None,
            }),
            128,
            36,
        );
        assert!(
            write_started
                .cells_text
                .contains("state     target write started")
        );

        let auto_poweroff = render_installer_scene(
            &InstallerScene::Stopped(StoppedModel {
                reason: FailureReason::TargetWriteFailed,
                failed_step_name: "copy Yaoshi image".to_string(),
                affected_disk: Some(PathBuf::from("/dev/vda")),
                failure_write_state: FailureWriteState::TargetWriteStarted,
                auto_poweroff_seconds: Some(15),
            }),
            128,
            36,
        );
        assert!(
            auto_poweroff
                .cells_text
                .contains("poweroff  automatic in 15 s; Enter powers off now")
        );
    }

    #[test]
    fn installer_phase_rows_show_required_operational_context() {
        let display = NegotiatedDisplay::fixture(Surface::Installer, 128, 36);
        let prepare = render_installer_scene(
            &InstallerScene::Prepare(PrepareModel {
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
            }),
            128,
            36,
        );
        for required in [
            "status    preparing installer",
            "current   starting input and storage drivers",
            "[done]    console ready",
            "[active]  input and storage drivers 0 / 0",
            "[pending] installer media 0.000 s / 10.000 s",
            "[pending] payload validation",
            "[pending] target disk scan",
        ] {
            assert!(prepare.cells_text.contains(required), "missing {required}");
        }

        let loading = render_installer_scene(
            &InstallerScene::Prepare(PrepareModel {
                display: NegotiatedDisplay::fixture(Surface::Installer, 128, 36),
                media_check_elapsed: Duration::ZERO,
                modules_state: "loading 3 / 16".to_string(),
                media_state: "not-started".to_string(),
                payload_state: "not opened".to_string(),
                disk_inspection_state: "not inspected".to_string(),
                target_minimum_bytes: None,
                payload_planned_extent_bytes: None,
                payload_container_bytes: None,
                payload_zero_extent_bytes: None,
            }),
            128,
            36,
        );
        assert!(
            loading
                .cells_text
                .contains("current   loading input and storage support")
        );
        assert!(
            loading
                .cells_text
                .contains("[active]  input and storage drivers 3 / 16")
        );
        assert!(!loading.cells_text.contains("xhci"));
        assert!(!loading.cells_text.contains(".ko"));

        let target = render_installer_scene(
            &InstallerScene::Target(TargetModel {
                candidates: vec![fixture_disk("vda", 4 << 30)],
                selected_visible: 0,
                focus: TargetFocus::ConfirmTarget,
                candidate_total_count: 1,
                selectable_count: 1,
                installer_media_count: 1,
                installed_target_count: 0,
                blocked_by_installed_target_count: 0,
                too_small_count: 0,
                unsupported_sector_size_count: 0,
                no_stable_id_count: 0,
                read_error_count: 0,
                target_minimum_bytes: 2 << 30,
                payload_planned_extent_bytes: 512 << 20,
                payload_container_bytes: 256 << 20,
            }),
            128,
            36,
        );
        assert!(target.cells_text.contains("> /dev/vda"));
        assert_eq!(target.cells_text.matches("image").count(), 1);
        assert!(target.cells_text.contains("selected"));
        assert!(target.cells_text.contains("disk      /dev/vda"));
        assert!(
            target
                .cells_text
                .contains("usage     50.000% before first boot growth")
        );
        assert!(
            target
                .cells_text
                .contains("identity  /dev/disk/by-id/virtio-vda-stable")
        );
        let write = render_installer_scene(
            &InstallerScene::Write(WriteModel {
                operation: TargetOperation::Install,
                target_dev_path: PathBuf::from("/dev/vda"),
                task: WriteTask::CopyYaoshiImage,
                head_scrub_written_bytes: 16 << 20,
                target_head_scrub_bytes: 16 << 20,
                tail_scrub_written_bytes: 16 << 20,
                target_tail_scrub_bytes: 16 << 20,
                planned_written_bytes: 256 << 20,
                planned_total_bytes: 512 << 20,
                source_read_bytes: 128 << 20,
                payload_source_bytes: 256 << 20,
                zero_written_bytes: 64 << 20,
                payload_zero_extent_bytes: (2 << 30) - (512 << 20),
                target_image_bytes: 2 << 30,
                current_rate_bps: None,
                average_rate_bps: None,
                eta: None,
            }),
            128,
            36,
        );
        assert!(write.cells_text.contains("[active]  copy Yaoshi image"));
        assert!(!write.cells_text.contains("source    "));
        assert!(write.cells_text.contains("target    /dev/vda"));
        assert!(write.cells_text.contains("image     "));

        let done = render_installer_scene(
            &InstallerScene::Done(DoneModel {
                operation: TargetOperation::Install,
                target_dev_path: PathBuf::from("/dev/vda"),
                target_stable_id: Some("/dev/disk/by-id/virtio-vda".to_string()),
                payload_planned_extent_bytes: 512 << 20,
                payload_zero_extent_bytes: (2 << 30) - (512 << 20),
                focus: DoneFocus::Reboot,
            }),
            128,
            36,
        );
        assert!(done.cells_text.contains("  identity     virtio-vda"));
    }

    #[test]
    fn dashboard_metric_state_words_fit_at_supported_floor() {
        let frame = dashboard::scene(&fixture_dashboard(), (160, 44));
        for prefix in [
            "Usage     ",
            "RAM       ",
            "Swap      ",
            "Root      ",
            "cpu0",
        ] {
            let row = frame
                .cells_text
                .lines()
                .find(|line| line.contains(prefix))
                .unwrap_or_else(|| panic!("missing dashboard metric row {prefix}"));
            assert!(
                !row.contains(" nominal") && !row.contains(" normal") && !row.contains(" healthy"),
                "dashboard metric row contains a below-warning state word at 160x44: {row:?}"
            );
            assert!(
                !row.trim_end().ends_with("..."),
                "dashboard metric row was truncated at 160x44: {row:?}"
            );
        }
    }

    #[test]
    fn style_artifacts_preserve_semantic_names_with_equal_rgb_values() {
        let display = NegotiatedDisplay::fixture(Surface::Installer, 112, 32);
        let frame = render_installer_scene(
            &InstallerScene::Prepare(PrepareModel {
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
            }),
            112,
            32,
        );
        let styles = buffer_to_styles(&frame.buffer);
        assert!(
            styles.contains("style=Plain fg=white bg=default modifier=none"),
            "plain content must not collapse into Canvas style runs:\n{styles}"
        );
        assert!(
            styles.contains("style=Canvas fg=default bg=default modifier=none"),
            "canvas cells must still be represented explicitly:\n{styles}"
        );
    }

    #[test]
    fn installer_action_rows_style_only_the_focused_enabled_action() {
        let target = render_installer_scene(
            &InstallerScene::Target(TargetModel {
                candidates: vec![fixture_disk("vda", 4 << 30)],
                selected_visible: 0,
                focus: TargetFocus::ConfirmTarget,
                candidate_total_count: 1,
                selectable_count: 1,
                installer_media_count: 1,
                installed_target_count: 0,
                blocked_by_installed_target_count: 0,
                too_small_count: 0,
                unsupported_sector_size_count: 0,
                no_stable_id_count: 0,
                read_error_count: 0,
                target_minimum_bytes: 2 << 30,
                payload_planned_extent_bytes: 512 << 20,
                payload_container_bytes: 256 << 20,
            }),
            96,
            28,
        );
        assert_text_style(&target, "[ Confirm target ]", SemanticStyle::ActionFocus);
        assert_text_style(&target, "Refresh", SemanticStyle::Action);
        assert_text_style(&target, "Power off", SemanticStyle::Action);

        let zero_disk_target = render_installer_scene(
            &InstallerScene::Target(TargetModel {
                candidates: Vec::new(),
                selected_visible: 0,
                focus: TargetFocus::ConfirmTarget,
                candidate_total_count: 0,
                selectable_count: 0,
                installer_media_count: 1,
                installed_target_count: 0,
                blocked_by_installed_target_count: 0,
                too_small_count: 0,
                unsupported_sector_size_count: 0,
                no_stable_id_count: 0,
                read_error_count: 0,
                target_minimum_bytes: 2 << 30,
                payload_planned_extent_bytes: 512 << 20,
                payload_container_bytes: 256 << 20,
            }),
            96,
            28,
        );
        assert_text_style(&zero_disk_target, "Confirm target", SemanticStyle::Disabled);
        assert_text_style(&zero_disk_target, "[ Refresh ]", SemanticStyle::ActionFocus);
        assert_text_style(&zero_disk_target, "Power off", SemanticStyle::Action);

        let install = render_installer_scene(
            &InstallerScene::Install(InstallModel {
                target: fixture_disk("vda", 4 << 30),
                target_minimum_bytes: 2 << 30,
                payload_planned_extent_bytes: 512 << 20,
                payload_container_bytes: 256 << 20,
                payload_zero_extent_bytes: (2 << 30) - (512 << 20),
                focus: InstallFocus::Destructive,
            }),
            128,
            36,
        );
        assert_text_style(&install, "Back", SemanticStyle::Action);
        assert_text_style(&install, "Erase disk only /dev/vda", SemanticStyle::Action);
        assert_text_style(
            &install,
            "[ Install Yaoshi to /dev/vda ]",
            SemanticStyle::DestructiveFocus,
        );
        assert_text_style(&install, "Power off", SemanticStyle::Action);

        let erase = render_installer_scene(
            &InstallerScene::Install(InstallModel {
                target: fixture_disk("vda", 4 << 30),
                target_minimum_bytes: 2 << 30,
                payload_planned_extent_bytes: 512 << 20,
                payload_container_bytes: 256 << 20,
                payload_zero_extent_bytes: (2 << 30) - (512 << 20),
                focus: InstallFocus::Erase,
            }),
            128,
            36,
        );
        assert_text_style(
            &erase,
            "[ Erase disk only /dev/vda ]",
            SemanticStyle::DestructiveFocus,
        );

        let done = render_installer_scene(
            &InstallerScene::Done(DoneModel {
                operation: TargetOperation::Install,
                target_dev_path: PathBuf::from("/dev/vda"),
                target_stable_id: Some("/dev/disk/by-id/virtio-vda".to_string()),
                payload_planned_extent_bytes: 512 << 20,
                payload_zero_extent_bytes: (2 << 30) - (512 << 20),
                focus: DoneFocus::Reboot,
            }),
            96,
            28,
        );
        assert_text_style(&done, "[ Reboot ]", SemanticStyle::ActionFocus);
        assert_text_style(&done, "Power off", SemanticStyle::Action);

        let erase_done = render_installer_scene(
            &InstallerScene::Done(DoneModel {
                operation: TargetOperation::Erase,
                target_dev_path: PathBuf::from("/dev/vda"),
                target_stable_id: Some("/dev/disk/by-id/virtio-vda".to_string()),
                payload_planned_extent_bytes: 32 << 20,
                payload_zero_extent_bytes: 32 << 20,
                focus: DoneFocus::ChooseTarget,
            }),
            96,
            28,
        );
        assert!(
            erase_done
                .cells_text
                .contains("Yaoshi Installer - Erase Result")
        );
        assert_text_style(&erase_done, "[ Choose target ]", SemanticStyle::ActionFocus);
        assert_text_style(&erase_done, "Reboot", SemanticStyle::Action);
        assert_text_style(&erase_done, "Power off", SemanticStyle::Action);
    }

    #[test]
    fn writes_deterministic_ui_review_artifacts() {
        let root = repo_root().join("target/yaoshi-test/ui-review");
        let _ = fs::remove_dir_all(&root);

        let installer_display = NegotiatedDisplay::fixture(Surface::Installer, 112, 32);
        let installer_scenes = vec![
            (
                "prepare",
                InstallerScene::Prepare(PrepareModel {
                    display: installer_display,
                    media_check_elapsed: Duration::ZERO,
                    modules_state: "not-started".to_string(),
                    media_state: "not-started".to_string(),
                    payload_state: "not opened".to_string(),
                    disk_inspection_state: "not inspected".to_string(),
                    target_minimum_bytes: None,
                    payload_planned_extent_bytes: None,
                    payload_container_bytes: None,
                    payload_zero_extent_bytes: None,
                }),
            ),
            (
                "target",
                InstallerScene::Target(TargetModel {
                    candidates: vec![fixture_disk("vda", 4 << 30)],
                    selected_visible: 0,
                    focus: TargetFocus::ConfirmTarget,
                    candidate_total_count: 1,
                    selectable_count: 1,
                    installer_media_count: 1,
                    installed_target_count: 0,
                    blocked_by_installed_target_count: 0,
                    too_small_count: 0,
                    unsupported_sector_size_count: 0,
                    no_stable_id_count: 0,
                    read_error_count: 0,
                    target_minimum_bytes: 2 << 30,
                    payload_planned_extent_bytes: 512 << 20,
                    payload_container_bytes: 256 << 20,
                }),
            ),
            (
                "install",
                InstallerScene::Install(InstallModel {
                    target: fixture_disk("vda", 4 << 30),
                    target_minimum_bytes: 2 << 30,
                    payload_planned_extent_bytes: 512 << 20,
                    payload_container_bytes: 256 << 20,
                    payload_zero_extent_bytes: (2 << 30) - (512 << 20),
                    focus: InstallFocus::Back,
                }),
            ),
            (
                "write",
                InstallerScene::Write(WriteModel {
                    operation: TargetOperation::Install,
                    target_dev_path: PathBuf::from("/dev/vda"),
                    task: WriteTask::CopyYaoshiImage,
                    head_scrub_written_bytes: 16 << 20,
                    target_head_scrub_bytes: 16 << 20,
                    tail_scrub_written_bytes: 16 << 20,
                    target_tail_scrub_bytes: 16 << 20,
                    planned_written_bytes: 256 << 20,
                    planned_total_bytes: 512 << 20,
                    source_read_bytes: 128 << 20,
                    payload_source_bytes: 256 << 20,
                    zero_written_bytes: 64 << 20,
                    payload_zero_extent_bytes: (2 << 30) - (512 << 20),
                    target_image_bytes: 2 << 30,
                    current_rate_bps: Some(88 << 20),
                    average_rate_bps: Some(64 << 20),
                    eta: Some(Duration::from_secs(23)),
                }),
            ),
            (
                "done",
                InstallerScene::Done(DoneModel {
                    operation: TargetOperation::Install,
                    target_dev_path: PathBuf::from("/dev/vda"),
                    target_stable_id: Some("/dev/disk/by-id/virtio-vda".to_string()),
                    payload_planned_extent_bytes: 512 << 20,
                    payload_zero_extent_bytes: (2 << 30) - (512 << 20),
                    focus: DoneFocus::Reboot,
                }),
            ),
            (
                "erase-done",
                InstallerScene::Done(DoneModel {
                    operation: TargetOperation::Erase,
                    target_dev_path: PathBuf::from("/dev/vdb"),
                    target_stable_id: Some("/dev/disk/by-id/virtio-vdb".to_string()),
                    payload_planned_extent_bytes: 32 << 20,
                    payload_zero_extent_bytes: 32 << 20,
                    focus: DoneFocus::ChooseTarget,
                }),
            ),
            (
                "stopped",
                InstallerScene::Stopped(StoppedModel {
                    reason: FailureReason::TargetWriteFailed,
                    failed_step_name: "copy Yaoshi image".to_string(),
                    affected_disk: Some(PathBuf::from("/dev/vda")),
                    failure_write_state: FailureWriteState::TargetWriteStarted,
                    auto_poweroff_seconds: None,
                }),
            ),
        ];

        for (columns, rows) in [(96, 28), (128, 36)] {
            for (id, scene) in &installer_scenes {
                let frame = render_installer_scene(scene, columns, rows);
                write_review_artifact(
                    &root.join(format!("installer/{columns:03}x{rows:03}")),
                    id,
                    &frame,
                    columns,
                    rows,
                );
            }
        }

        for (columns, rows) in [(160, 44), (184, 52)] {
            let frame = dashboard::scene(&fixture_dashboard(), (columns, rows));
            write_review_artifact(
                &root.join(format!("dashboard/{columns:03}x{rows:03}")),
                "dashboard",
                &frame,
                columns,
                rows,
            );
        }
    }

    fn write_review_artifact(dir: &Path, id: &str, frame: &RenderedFrame, columns: u16, rows: u16) {
        fs::create_dir_all(dir).unwrap();
        let cells = buffer_to_fixed_cells(&frame.buffer, columns, rows);
        assert_cells_shape(&cells, columns, rows);
        let styles = buffer_to_styles(&frame.buffer);
        assert!(styles.lines().all(|line| line.starts_with("row=")));
        fs::write(dir.join(format!("{id}.cells.txt")), cells).unwrap();
        fs::write(dir.join(format!("{id}.styles.txt")), styles).unwrap();
    }

    fn assert_cells_shape(cells: &str, columns: u16, rows: u16) {
        let lines = cells.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), rows as usize);
        for line in lines {
            assert_eq!(line.len(), columns as usize);
            assert!(line.bytes().all(|byte| (0x20..=0x7e).contains(&byte)));
        }
    }

    struct TestPsf<'a> {
        width: u16,
        height: u16,
        char_count: usize,
        char_size: usize,
        data: &'a [u8],
    }

    impl<'a> TestPsf<'a> {
        fn glyph(&self, byte: u8) -> &'a [u8] {
            let start = byte as usize * self.char_size;
            &self.data[start..start + self.char_size]
        }
    }

    fn parse_psf2_for_test(bytes: &[u8]) -> TestPsf<'_> {
        assert!(bytes.len() >= 32);
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            0x864a_b572
        );
        let header_size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let char_count = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
        let char_size = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
        let height = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as u16;
        let width = u32::from_le_bytes(bytes[28..32].try_into().unwrap()) as u16;
        let end = header_size + char_count * char_size;
        assert!(bytes.len() >= end);
        TestPsf {
            width,
            height,
            char_count,
            char_size,
            data: &bytes[header_size..end],
        }
    }

    fn assert_text_style(frame: &RenderedFrame, needle: &str, expected: SemanticStyle) {
        let mut row_start = 0usize;
        for (row, line) in frame.cells_text.lines().enumerate() {
            if let Some(col) = line.find(needle) {
                for offset in 0..needle.len() {
                    let style = frame
                        .semantic_spans
                        .iter()
                        .find(|span| {
                            span.y == row as u16
                                && span.x <= (col + offset) as u16
                                && (col + offset) as u16
                                    <= span.x.saturating_add(span.width).saturating_sub(1)
                        })
                        .map(|span| span.semantic_style)
                        .unwrap_or(SemanticStyle::Canvas);
                    assert_eq!(
                        style,
                        expected,
                        "unexpected style for {needle:?} at row {row} col {} in frame:\n{}",
                        col + offset,
                        frame.cells_text
                    );
                }
                return;
            }
            row_start += line.len() + 1;
        }
        let _ = row_start;
        panic!("missing {needle:?} in frame:\n{}", frame.cells_text);
    }
}
