fn select_loop(
    source: &Source,
    tty1: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
) -> Result<(), String> {
    loop {
        render_page_audit(tty1, serial, "installer-prepare", |width, height| {
            prepare_page(
                &yaoshi_screen::NegotiatedDisplay::fixture(
                    yaoshi_screen::Surface::Installer,
                    width,
                    height,
                ),
                "loaded",
                "media-found",
                "valid",
                "inspecting",
                Duration::ZERO,
                Some(source.payload_target_minimum_bytes),
                Some(source.payload_planned_extent_bytes),
                Some(source.payload_size),
                Some(source.payload_zero_extent_bytes),
                width,
                height,
            )
        })?;
        let candidates = match inspect_disks(source) {
            Ok(candidates) => candidates,
            Err(_) => {
                render_page(tty1, |width, height| {
                    failure_page(
                        FailureReason::DiskInspectionFailed.as_str(),
                        None,
                        false,
                        Some(BLOCKING_STOP_POWEROFF_TIMEOUT.as_secs()),
                        width,
                        height,
                    )
                })?;
                wait_failure_exit(
                    tty1,
                    serial,
                    original_termios,
                    FailureReason::DiskInspectionFailed.as_str(),
                    None,
                    false,
                );
                return Ok(());
            }
        };
        let mut selected = 0usize;
        let mut focus = if target_action_disks(&candidates).is_empty() {
            yaoshi_screen::TargetFocus::Refresh
        } else {
            yaoshi_screen::TargetFocus::ConfirmTarget
        };
        loop {
            render_page_audit(tty1, serial, "installer-target", |width, height| {
                target_page(
                    &candidates,
                    selected,
                    source.payload_target_minimum_bytes,
                    source.payload_planned_extent_bytes,
                    source.payload_size,
                    focus,
                    width,
                    height,
                )
            })?;
            match read_input(tty1) {
                Some(yaoshi_screen::Input::ArrowUp) => {
                    selected = selected.saturating_sub(1);
                }
                Some(yaoshi_screen::Input::ArrowDown) => {
                    if selected + 1 < target_action_disks(&candidates).len() {
                        selected += 1;
                    }
                }
                Some(yaoshi_screen::Input::Tab) => {
                    focus = next_choose_focus(focus, !target_action_disks(&candidates).is_empty())
                }
                Some(yaoshi_screen::Input::Enter) => match focus {
                    yaoshi_screen::TargetFocus::ConfirmTarget => {
                        let selectable = target_action_disks(&candidates);
                        if let Some(target) = selectable.get(selected) {
                            match confirm_loop(
                                source,
                                (*target).clone(),
                                tty1,
                                serial,
                                original_termios,
                            )? {
                                ConfirmResult::Back => {}
                                ConfirmResult::ChooseTarget => break,
                                ConfirmResult::Complete => return Ok(()),
                            }
                        }
                    }
                    yaoshi_screen::TargetFocus::Refresh => break,
                    yaoshi_screen::TargetFocus::PowerOff => {
                        render_page(tty1, |width, height| exit_page("poweroff", width, height))?;
                        pause_for_exit_scene_review();
                        controlled_exit(serial, "poweroff", tty1, original_termios);
                    }
                },
                Some(yaoshi_screen::Input::Resize { .. }) => {}
                _ => {}
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmResult {
    Back,
    ChooseTarget,
    Complete,
}

fn confirm_loop(
    source: &Source,
    target: TargetDiskCandidate,
    tty1: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
) -> Result<ConfirmResult, String> {
    let mut focus = yaoshi_screen::InstallFocus::Back;
    loop {
        let screen_id = match focus {
            yaoshi_screen::InstallFocus::Back => "installer-confirm-safe",
            yaoshi_screen::InstallFocus::Erase => "installer-confirm-erase",
            yaoshi_screen::InstallFocus::Destructive => "installer-confirm-install",
            yaoshi_screen::InstallFocus::PowerOff => "installer-confirm-poweroff",
        };
        render_page_audit(tty1, serial, screen_id, |width, height| {
            yaoshi_screen::render_installer_scene(
                &yaoshi_screen::InstallerScene::Install(yaoshi_screen::InstallModel {
                    target: target.clone(),
                    target_minimum_bytes: source.payload_target_minimum_bytes,
                    payload_planned_extent_bytes: source.payload_planned_extent_bytes,
                    payload_container_bytes: source.payload_size,
                    payload_zero_extent_bytes: source.payload_zero_extent_bytes,
                    focus,
                }),
                width,
                height,
            )
        })?;
        match read_input(tty1) {
            Some(yaoshi_screen::Input::Tab) => {
                focus = next_install_focus(focus, target.status == CandidateStatus::Selectable);
            }
            Some(yaoshi_screen::Input::Enter) => match focus {
                yaoshi_screen::InstallFocus::Back => {
                    return Ok(ConfirmResult::Back);
                }
                yaoshi_screen::InstallFocus::Erase => {
                    mark(
                        serial,
                        &markers::write_task(WriteTask::VerifyTarget, "active"),
                    );
                    render_page_audit(tty1, serial, "installer-write", |width, height| {
                        write_page(
                            &initial_erase_progress_model(&target.disk.dev_path, &target.disk),
                            width,
                            height,
                        )
                    })?;
                    let opened = match prepare_erase_target(&target) {
                        Ok(opened) => opened,
                        Err(err) => {
                            render_page(tty1, |width, height| {
                                failure_page(
                                    &err,
                                    Some(target.disk.dev_path.clone()),
                                    false,
                                    Some(BLOCKING_STOP_POWEROFF_TIMEOUT.as_secs()),
                                    width,
                                    height,
                                )
                            })?;
                            wait_failure_exit(
                                tty1,
                                serial,
                                original_termios,
                                &err,
                                Some(target.disk.dev_path.clone()),
                                false,
                            );
                            return Ok(ConfirmResult::Complete);
                        }
                    };
                    match erase_target(opened, tty1, serial) {
                        Ok(result) => {
                            mark(serial, markers::COMPLETE);
                            let complete = yaoshi_screen::DoneModel {
                                operation: yaoshi_screen::TargetOperation::Erase,
                                target_dev_path: result.target_dev_path,
                                target_stable_id: result.target_stable_id,
                                payload_planned_extent_bytes: result.bytes_written,
                                payload_zero_extent_bytes: result.bytes_written,
                                focus: yaoshi_screen::DoneFocus::ChooseTarget,
                            };
                            render_page_audit(
                                tty1,
                                serial,
                                "installer-erase-done",
                                |width, height| done_page(&complete, width, height),
                            )?;
                            match wait_complete_exit(tty1, serial, original_termios, complete) {
                                CompleteAction::ChooseTarget => {
                                    return Ok(ConfirmResult::ChooseTarget);
                                }
                            }
                        }
                        Err(err) => {
                            render_page(tty1, |width, height| {
                                failure_page(
                                    &err,
                                    Some(target.disk.dev_path.clone()),
                                    true,
                                    Some(BLOCKING_STOP_POWEROFF_TIMEOUT.as_secs()),
                                    width,
                                    height,
                                )
                            })?;
                            wait_failure_exit(
                                tty1,
                                serial,
                                original_termios,
                                &err,
                                Some(target.disk.dev_path.clone()),
                                true,
                            );
                            return Ok(ConfirmResult::Complete);
                        }
                    }
                }
                yaoshi_screen::InstallFocus::Destructive => {
                    if target.status != CandidateStatus::Selectable {
                        continue;
                    }
                    mark(
                        serial,
                        &markers::write_task(WriteTask::VerifyTarget, "active"),
                    );
                    render_page_audit(tty1, serial, "installer-write", |width, height| {
                        write_page(
                            &initial_install_progress_model(&target.disk.dev_path, source),
                            width,
                            height,
                        )
                    })?;
                    let opened = match prepare_write_target(source, &target) {
                        Ok(opened) => opened,
                        Err(err) => {
                            render_page(tty1, |width, height| {
                                failure_page(
                                    &err,
                                    Some(target.disk.dev_path.clone()),
                                    false,
                                    Some(BLOCKING_STOP_POWEROFF_TIMEOUT.as_secs()),
                                    width,
                                    height,
                                )
                            })?;
                            wait_failure_exit(
                                tty1,
                                serial,
                                original_termios,
                                &err,
                                Some(target.disk.dev_path.clone()),
                                false,
                            );
                            return Ok(ConfirmResult::Complete);
                        }
                    };
                    match write_target(source, opened, tty1, serial) {
                        Ok(result) => {
                            mark(serial, markers::COMPLETE);
                            let complete = yaoshi_screen::DoneModel {
                                operation: yaoshi_screen::TargetOperation::Install,
                                target_dev_path: result.target_dev_path,
                                target_stable_id: result.target_stable_id,
                                payload_planned_extent_bytes: result.bytes_written,
                                payload_zero_extent_bytes: source.payload_zero_extent_bytes,
                                focus: yaoshi_screen::DoneFocus::Reboot,
                            };
                            render_page_audit(tty1, serial, "installer-done", |width, height| {
                                done_page(&complete, width, height)
                            })?;
                            wait_complete_exit(tty1, serial, original_termios, complete);
                            return Ok(ConfirmResult::Complete);
                        }
                        Err(err) => {
                            render_page(tty1, |width, height| {
                                failure_page(
                                    &err,
                                    Some(target.disk.dev_path.clone()),
                                    true,
                                    Some(BLOCKING_STOP_POWEROFF_TIMEOUT.as_secs()),
                                    width,
                                    height,
                                )
                            })?;
                            wait_failure_exit(
                                tty1,
                                serial,
                                original_termios,
                                &err,
                                Some(target.disk.dev_path.clone()),
                                true,
                            );
                            return Ok(ConfirmResult::Complete);
                        }
                    }
                }
                yaoshi_screen::InstallFocus::PowerOff => {
                    render_page(tty1, |width, height| exit_page("poweroff", width, height))?;
                    pause_for_exit_scene_review();
                    controlled_exit(serial, "poweroff", tty1, original_termios);
                }
            },
            Some(yaoshi_screen::Input::Resize { .. }) => {}
            _ => {}
        }
    }
}

fn target_action_disks(candidates: &[TargetDiskCandidate]) -> Vec<&TargetDiskCandidate> {
    candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.status,
                CandidateStatus::Selectable
                    | CandidateStatus::InstalledTarget
                    | CandidateStatus::BlockedByInstalledTarget
            )
        })
        .collect()
}

fn next_install_focus(
    focus: yaoshi_screen::InstallFocus,
    install_enabled: bool,
) -> yaoshi_screen::InstallFocus {
    match focus {
        yaoshi_screen::InstallFocus::Back => yaoshi_screen::InstallFocus::Erase,
        yaoshi_screen::InstallFocus::Erase if install_enabled => {
            yaoshi_screen::InstallFocus::Destructive
        }
        yaoshi_screen::InstallFocus::Erase => yaoshi_screen::InstallFocus::PowerOff,
        yaoshi_screen::InstallFocus::Destructive => yaoshi_screen::InstallFocus::PowerOff,
        yaoshi_screen::InstallFocus::PowerOff => yaoshi_screen::InstallFocus::Back,
    }
}

fn next_choose_focus(
    focus: yaoshi_screen::TargetFocus,
    has_disks: bool,
) -> yaoshi_screen::TargetFocus {
    if has_disks {
        match focus {
            yaoshi_screen::TargetFocus::ConfirmTarget => yaoshi_screen::TargetFocus::Refresh,
            yaoshi_screen::TargetFocus::Refresh => yaoshi_screen::TargetFocus::PowerOff,
            yaoshi_screen::TargetFocus::PowerOff => yaoshi_screen::TargetFocus::ConfirmTarget,
        }
    } else {
        match focus {
            yaoshi_screen::TargetFocus::PowerOff => yaoshi_screen::TargetFocus::Refresh,
            _ => yaoshi_screen::TargetFocus::PowerOff,
        }
    }
}

#[derive(Debug, Clone)]
struct Source {
    disk: KernelDiskRef,
    payload_offset: u64,
    payload_size: u64,
    payload_target_image_bytes: u64,
    payload_target_minimum_bytes: u64,
    payload_planned_extent_bytes: u64,
    payload_zero_extent_bytes: u64,
    payload_extent_count: u64,
}

fn discover_source(
    tty1: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
    display: &yaoshi_screen::NegotiatedDisplay,
) -> Result<Source, String> {
    let started = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        render_preparing_progress(tty1, started, "scanning", "not opened", display)?;
        handle_preparing_input(tty1, serial, original_termios)?;
        let mut found = Vec::new();
        for disk in list_whole_disks()
            .map_err(|_| FailureReason::DiskInspectionFailed.as_str().to_string())?
        {
            handle_preparing_input(tty1, serial, original_termios)?;
            if disk.logical_block_size != 512 {
                render_preparing_progress(tty1, started, "checking", "not opened", display)?;
                continue;
            }
            render_preparing_progress(tty1, started, "checking", "not opened", display)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            if let Some(source) = probe_source_with_timeout(
                disk.clone(),
                remaining.min(SOURCE_PROBE_TIMEOUT),
                tty1,
                serial,
                original_termios,
            )? {
                found.push(source);
                render_preparing_progress(tty1, started, "media-found", "validating", display)?;
            }
        }
        match found.len() {
            1 => {
                mark(serial, markers::INSTALLATION_MEDIA_DISCOVERED);
                return Ok(found.remove(0));
            }
            n if n > 1 => return Err("installation-media-ambiguous".to_string()),
            _ if Instant::now() >= deadline => {
                return Err("installation-media-not-found".to_string());
            }
            _ => sleep_with_preparing_input(tty1, serial, original_termios)?,
        }
    }
}

fn probe_source_with_timeout(
    disk: KernelDiskRef,
    timeout: Duration,
    tty1: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
) -> Result<Option<Source>, String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = probe_source_disk(&disk);
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + timeout;
    loop {
        handle_preparing_input(tty1, serial, original_termios)?;
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => return Ok(result),
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(mpsc::RecvTimeoutError::Timeout) => return Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
        }
    }
}

fn probe_source_disk(disk: &KernelDiskRef) -> Option<Source> {
    let parts = yaoshi_image::parse_mbr_with_len(&disk.dev_path, disk.byte_size).ok()?;
    source_from_mbr(disk, &parts)
}

fn render_preparing_progress(
    tty1: &mut File,
    started: Instant,
    media_state: &str,
    payload_state: &str,
    display: &yaoshi_screen::NegotiatedDisplay,
) -> Result<(), String> {
    let elapsed = started.elapsed();
    render_page(tty1, |width, height| {
        prepare_page(
            display,
            "loaded",
            media_state,
            payload_state,
            "not inspected",
            elapsed,
            None,
            None,
            None,
            None,
            width,
            height,
        )
    })
}

fn sleep_with_preparing_input(
    tty1: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
) -> Result<(), String> {
    let until = Instant::now() + Duration::from_millis(200);
    while Instant::now() < until {
        handle_preparing_input(tty1, serial, original_termios)?;
        thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}

fn handle_preparing_input(
    tty1: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
) -> Result<(), String> {
    if !input_ready(tty1, 0) {
        return Ok(());
    }
    match read_input(tty1) {
        Some(yaoshi_screen::Input::Enter) => {
            render_page(tty1, |width, height| exit_page("poweroff", width, height))?;
            pause_for_exit_scene_review();
            controlled_exit(serial, "poweroff", tty1, original_termios);
        }
        Some(yaoshi_screen::Input::Resize { .. }) => {
            render_page(tty1, |width, height| {
                prepare_page(
                    &yaoshi_screen::NegotiatedDisplay::fixture(
                        yaoshi_screen::Surface::Installer,
                        width,
                        height,
                    ),
                    "loaded",
                    "scanning",
                    "not opened",
                    "not inspected",
                    Duration::ZERO,
                    None,
                    None,
                    None,
                    None,
                    width,
                    height,
                )
            })?;
        }
        _ => {}
    }
    Ok(())
}

fn source_from_mbr(disk: &KernelDiskRef, parts: &[yaoshi_image::MbrPartition]) -> Option<Source> {
    if parts.len() != 2 || parts[0].mbr_type != 0xEF || parts[1].mbr_type != 0x83 {
        return None;
    }
    let boot_start = parts[0].start_lba as u64 * SECTOR_SIZE;
    let boot_size = parts[0].sector_count as u64 * SECTOR_SIZE;
    if boot_start != LEADING_GAP_BYTES || boot_size != INSTALLER_BOOT_SIZE_BYTES {
        return None;
    }
    let payload_offset = parts[1].start_lba as u64 * SECTOR_SIZE;
    let payload_size = parts[1].sector_count as u64 * SECTOR_SIZE;
    if payload_offset != LEADING_GAP_BYTES + INSTALLER_BOOT_SIZE_BYTES || payload_size == 0 {
        return None;
    }
    let info = yaoshi_payload::validate_payload_region_metadata(
        &disk.dev_path,
        payload_offset,
        payload_size,
    )
    .ok()?;
    Some(Source {
        disk: disk.clone(),
        payload_offset,
        payload_size,
        payload_target_image_bytes: info.target_image_bytes,
        payload_target_minimum_bytes: info.target_minimum_bytes,
        payload_planned_extent_bytes: info.planned_extent_bytes,
        payload_zero_extent_bytes: info.planned_zero_bytes,
        payload_extent_count: info.extent_count,
    })
}

fn inspect_disks(source: &Source) -> Result<Vec<TargetDiskCandidate>, DiskInspectError> {
    let records = list_whole_disk_records()?;
    let installed_target_attached = records.iter().any(|record| {
        !record.read_error
            && !same_kernel_disk(&record.disk, &source.disk)
            && is_yaoshi_installed_target_with_timeout(&record.disk, DISK_CLASSIFY_TIMEOUT)
    });
    let mut disks = records
        .into_iter()
        .map(|record| {
            let disk = record.disk;
            let mut existing = ExistingPartitionTable::Unrecognized;
            let status = if record.read_error {
                CandidateStatus::ReadError
            } else if same_kernel_disk(&disk, &source.disk) {
                existing =
                    classify_existing_partition_table_with_timeout(&disk, DISK_CLASSIFY_TIMEOUT)
                        .unwrap_or(ExistingPartitionTable::Unrecognized);
                CandidateStatus::InstallerMedia
            } else if is_yaoshi_installed_target_with_timeout(&disk, DISK_CLASSIFY_TIMEOUT) {
                existing = ExistingPartitionTable::Gpt;
                CandidateStatus::InstalledTarget
            } else if installed_target_attached {
                CandidateStatus::BlockedByInstalledTarget
            } else if disk.logical_block_size != 512 {
                CandidateStatus::UnsupportedSectorSize
            } else if disk.byte_size < source.payload_target_minimum_bytes {
                CandidateStatus::TooSmall
            } else if disk.stable_disk_id.is_none() {
                CandidateStatus::NoStableId
            } else {
                match classify_existing_partition_table_with_timeout(&disk, DISK_CLASSIFY_TIMEOUT) {
                    Ok(classified) => {
                        existing = classified;
                        CandidateStatus::Selectable
                    }
                    Err(()) => CandidateStatus::ReadError,
                }
            };
            TargetDiskCandidate {
                disk,
                status,
                existing,
            }
        })
        .collect::<Vec<_>>();
    disks.sort_by(|a, b| a.disk.kernel_name.cmp(&b.disk.kernel_name));
    Ok(disks)
}

fn list_whole_disks() -> Result<Vec<KernelDiskRef>, DiskInspectError> {
    Ok(list_whole_disk_records()?
        .into_iter()
        .filter_map(|record| (!record.read_error).then_some(record.disk))
        .collect())
}

fn list_whole_disk_records() -> Result<Vec<DiskRecord>, DiskInspectError> {
    let entries = fs::read_dir("/sys/block")
        .map_err(|e| DiskInspectError(format!("read /sys/block: {e}")))?;
    let mut disks = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| DiskInspectError(format!("read /sys/block entry: {e}")))?;
        let kernel_name = entry.file_name().to_string_lossy().to_string();
        if is_excluded_disk(&kernel_name) {
            continue;
        }
        let sysfs_path = PathBuf::from("/sys/block").join(&kernel_name);
        let logical_block_size = read_u64(sysfs_path.join("queue/logical_block_size"));
        let sectors = read_u64(sysfs_path.join("size"));
        let major_minor = read_string(sysfs_path.join("dev"));
        let read_error = logical_block_size.is_err() || sectors.is_err() || major_minor.is_err();
        let byte_size = sectors
            .ok()
            .and_then(|sectors| sectors.checked_mul(512))
            .unwrap_or(0);
        let model = read_string(format!("/sys/block/{kernel_name}/device/model"))
            .or_else(|_| read_string(format!("/sys/block/{kernel_name}/model")))
            .ok();
        let serial = read_string(format!("/sys/block/{kernel_name}/device/serial"))
            .or_else(|_| read_string(format!("/sys/block/{kernel_name}/serial")))
            .ok();
        disks.push(DiskRecord {
            disk: KernelDiskRef {
                sysfs_path,
                major_minor: major_minor.unwrap_or_else(|_| "unavailable".to_string()),
                kernel_name: kernel_name.clone(),
                dev_path: PathBuf::from("/dev").join(&kernel_name),
                logical_block_size: logical_block_size.unwrap_or(0),
                byte_size,
                stable_disk_id: stable_disk_id_for(&kernel_name),
                model,
                serial,
            },
            read_error,
        });
    }
    disks.sort_by(|a, b| a.disk.kernel_name.cmp(&b.disk.kernel_name));
    Ok(disks)
}

fn stable_disk_id_for(kernel_name: &str) -> Option<PathBuf> {
    for field in ["wwid", "eui", "uuid", "serial"] {
        let path = format!("/sys/block/{kernel_name}/device/{field}");
        let Ok(value) = read_string(path) else {
            continue;
        };
        if value.trim().is_empty() {
            continue;
        }
        let sanitized = yaoshi_screen::AsciiCellString::new(&value).into_string();
        if !sanitized.is_empty() {
            return Some(PathBuf::from(format!("sysfs:{sanitized}")));
        }
    }
    if let Ok(value) = read_string(format!("/sys/block/{kernel_name}/serial")) {
        let sanitized = yaoshi_screen::AsciiCellString::new(&value).into_string();
        if !sanitized.is_empty() {
            return Some(PathBuf::from(format!("sysfs:{sanitized}")));
        }
    }
    None
}

fn classify_existing_partition_table(disk: &KernelDiskRef) -> Result<ExistingPartitionTable, ()> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(&disk.dev_path)
        .map_err(|_| ())?;
    let mut sector = [0u8; 512];
    file.read_exact(&mut sector).map_err(|_| ())?;
    if sector[510] == 0x55 && sector[511] == 0xaa {
        let is_protective = sector[446 + 4] == 0xee;
        if is_protective {
            return if yaoshi_image::parse_gpt_physical(&disk.dev_path, disk.byte_size).is_ok() {
                Ok(ExistingPartitionTable::Gpt)
            } else {
                Ok(ExistingPartitionTable::Unrecognized)
            };
        }
        return if mbr_has_partition_entry(&sector) {
            Ok(ExistingPartitionTable::Mbr)
        } else {
            Ok(ExistingPartitionTable::None)
        };
    }
    if sector.iter().all(|byte| *byte == 0) {
        Ok(ExistingPartitionTable::None)
    } else {
        Ok(ExistingPartitionTable::Unrecognized)
    }
}

fn classify_existing_partition_table_with_timeout(
    disk: &KernelDiskRef,
    timeout: Duration,
) -> Result<ExistingPartitionTable, ()> {
    let disk = disk.clone();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = classify_existing_partition_table(&disk);
        let _ = tx.send(result);
    });
    rx.recv_timeout(timeout).unwrap_or(Err(()))
}

fn is_yaoshi_installed_target(disk: &KernelDiskRef) -> bool {
    let Ok(gpt) = yaoshi_image::parse_gpt_physical(&disk.dev_path, disk.byte_size) else {
        return false;
    };
    let layout = InstalledGptLayout::fixed(INSTALLED_ROOT_MINIMUM_BYTES);
    if gpt.disk_guid != layout.disk_guid {
        return false;
    }
    let has_esp = gpt.partitions.iter().any(|partition| {
        partition.number == 1
            && partition.unique_guid == layout.esp_guid
            && partition.name == INSTALLED_ESP_NAME
    });
    let has_root = gpt.partitions.iter().any(|partition| {
        partition.number == 2
            && partition.unique_guid == layout.root_guid
            && partition.name == INSTALLED_ROOT_NAME
    });
    has_esp && has_root
}

fn is_yaoshi_installed_target_with_timeout(disk: &KernelDiskRef, timeout: Duration) -> bool {
    let disk = disk.clone();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = is_yaoshi_installed_target(&disk);
        let _ = tx.send(result);
    });
    rx.recv_timeout(timeout).unwrap_or(false)
}

fn mbr_has_partition_entry(sector: &[u8; 512]) -> bool {
    sector[446..510]
        .chunks_exact(16)
        .any(|entry| entry.iter().any(|byte| *byte != 0))
}

fn same_kernel_disk(a: &KernelDiskRef, b: &KernelDiskRef) -> bool {
    a.sysfs_path == b.sysfs_path && a.major_minor == b.major_minor && a.kernel_name == b.kernel_name
}

fn is_excluded_disk(name: &str) -> bool {
    name.starts_with("ram")
        || name.starts_with("loop")
        || name.starts_with("dm-")
        || name.starts_with("md")
        || name.starts_with("zram")
        || name.starts_with("sr")
        || name.starts_with("fd")
}
