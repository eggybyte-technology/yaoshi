fn prepare_write_target(
    source: &Source,
    target: &TargetDiskCandidate,
) -> Result<OpenedTarget, String> {
    let disks = list_whole_disks().map_err(|_| "selected-disk-changed".to_string())?;
    let conflicting_installed_target = disks.iter().any(|disk| {
        !same_kernel_disk(disk, &source.disk)
            && is_yaoshi_installed_target_with_timeout(disk, DISK_CLASSIFY_TIMEOUT)
    });
    let reread = disks
        .into_iter()
        .find(|d| same_kernel_disk(d, &target.disk))
        .ok_or_else(|| "selected-disk-changed".to_string())?;
    if reread.logical_block_size != 512
        || reread.byte_size < source.payload_target_minimum_bytes
        || same_kernel_disk(&reread, &source.disk)
        || reread.stable_disk_id != target.disk.stable_disk_id
        || conflicting_installed_target
    {
        return Err("selected-disk-changed".to_string());
    }
    let existing = classify_existing_partition_table_with_timeout(&reread, DISK_CLASSIFY_TIMEOUT)
        .map_err(|_| "selected-disk-changed".to_string())?;
    yaoshi_payload::validate_payload_region_metadata(
        &source.disk.dev_path,
        source.payload_offset,
        source.payload_size,
    )
    .map_err(|_| "payload-validation-failed".to_string())?;
    let mut target_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&reread.dev_path)
        .map_err(|_| "target-open-failed".to_string())?;
    target_file
        .seek(SeekFrom::Start(0))
        .map_err(|_| "target-open-failed".to_string())?;
    Ok(OpenedTarget {
        target: TargetDiskCandidate {
            disk: reread,
            status: CandidateStatus::Selectable,
            existing,
        },
        file: target_file,
    })
}

fn prepare_erase_target(target: &TargetDiskCandidate) -> Result<OpenedTarget, String> {
    let reread = list_whole_disks()
        .map_err(|_| "selected-disk-changed".to_string())?
        .into_iter()
        .find(|d| same_kernel_disk(d, &target.disk))
        .ok_or_else(|| "selected-disk-changed".to_string())?;
    if reread.logical_block_size != 512 || reread.stable_disk_id != target.disk.stable_disk_id {
        return Err("selected-disk-changed".to_string());
    }
    let existing = classify_existing_partition_table_with_timeout(&reread, DISK_CLASSIFY_TIMEOUT)
        .map_err(|_| "selected-disk-changed".to_string())?;
    let mut target_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&reread.dev_path)
        .map_err(|_| "target-open-failed".to_string())?;
    target_file
        .seek(SeekFrom::Start(0))
        .map_err(|_| "target-open-failed".to_string())?;
    Ok(OpenedTarget {
        target: TargetDiskCandidate {
            disk: reread,
            status: target.status,
            existing,
        },
        file: target_file,
    })
}

fn initial_install_progress_model(
    target_dev_path: &Path,
    source: &Source,
) -> yaoshi_screen::WriteModel {
    yaoshi_screen::WriteModel {
        operation: yaoshi_screen::TargetOperation::Install,
        target_dev_path: target_dev_path.to_path_buf(),
        task: WriteTask::VerifyTarget,
        head_scrub_written_bytes: 0,
        target_head_scrub_bytes: TARGET_HEAD_SCRUB_BYTES,
        tail_scrub_written_bytes: 0,
        target_tail_scrub_bytes: TARGET_TAIL_SCRUB_BYTES,
        planned_written_bytes: 0,
        planned_total_bytes: source.payload_planned_extent_bytes,
        source_read_bytes: 0,
        payload_source_bytes: source.payload_size,
        zero_written_bytes: 0,
        payload_zero_extent_bytes: source.payload_zero_extent_bytes,
        target_image_bytes: source.payload_target_image_bytes,
        current_rate_bps: None,
        average_rate_bps: None,
        eta: None,
    }
}

fn initial_erase_progress_model(
    target_dev_path: &Path,
    target: &KernelDiskRef,
) -> yaoshi_screen::WriteModel {
    let (head, tail) = erase_scrub_lengths(target.byte_size);
    yaoshi_screen::WriteModel {
        operation: yaoshi_screen::TargetOperation::Erase,
        target_dev_path: target_dev_path.to_path_buf(),
        task: WriteTask::VerifyTarget,
        head_scrub_written_bytes: 0,
        target_head_scrub_bytes: head,
        tail_scrub_written_bytes: 0,
        target_tail_scrub_bytes: tail,
        planned_written_bytes: 0,
        planned_total_bytes: head.saturating_add(tail),
        source_read_bytes: 0,
        payload_source_bytes: 0,
        zero_written_bytes: 0,
        payload_zero_extent_bytes: head.saturating_add(tail),
        target_image_bytes: target.byte_size,
        current_rate_bps: None,
        average_rate_bps: None,
        eta: None,
    }
}

struct WriteProgressState {
    operation: yaoshi_screen::TargetOperation,
    target_dev_path: PathBuf,
    task: WriteTask,
    head_scrub_written_bytes: u64,
    target_head_scrub_bytes: u64,
    tail_scrub_written_bytes: u64,
    target_tail_scrub_bytes: u64,
    planned_written_bytes: u64,
    planned_total_bytes: u64,
    source_read_bytes: u64,
    payload_source_bytes: u64,
    zero_written_bytes: u64,
    payload_zero_extent_bytes: u64,
    target_image_bytes: u64,
    current_rate_bps: Option<u64>,
    started: Instant,
}

impl WriteProgressState {
    fn install(target_dev_path: PathBuf, target_size: u64, source: &Source) -> Self {
        Self {
            operation: yaoshi_screen::TargetOperation::Install,
            target_dev_path,
            task: WriteTask::VerifyTarget,
            head_scrub_written_bytes: 0,
            target_head_scrub_bytes: TARGET_HEAD_SCRUB_BYTES.min(target_size),
            tail_scrub_written_bytes: 0,
            target_tail_scrub_bytes: TARGET_TAIL_SCRUB_BYTES.min(target_size),
            planned_written_bytes: 0,
            planned_total_bytes: source.payload_planned_extent_bytes,
            source_read_bytes: 0,
            payload_source_bytes: source.payload_size,
            zero_written_bytes: 0,
            payload_zero_extent_bytes: source.payload_zero_extent_bytes,
            target_image_bytes: source.payload_target_image_bytes,
            current_rate_bps: None,
            started: Instant::now(),
        }
    }

    fn erase(target_dev_path: PathBuf, target_size: u64) -> Self {
        let (head_len, tail_len) = erase_scrub_lengths(target_size);
        let total = head_len.saturating_add(tail_len);
        Self {
            operation: yaoshi_screen::TargetOperation::Erase,
            target_dev_path,
            task: WriteTask::VerifyTarget,
            head_scrub_written_bytes: 0,
            target_head_scrub_bytes: head_len,
            tail_scrub_written_bytes: 0,
            target_tail_scrub_bytes: tail_len,
            planned_written_bytes: 0,
            planned_total_bytes: total,
            source_read_bytes: 0,
            payload_source_bytes: 0,
            zero_written_bytes: 0,
            payload_zero_extent_bytes: total,
            target_image_bytes: target_size,
            current_rate_bps: None,
            started: Instant::now(),
        }
    }

    fn model(&self) -> yaoshi_screen::WriteModel {
        let elapsed = self.started.elapsed();
        let average_rate_bps = if self.planned_written_bytes > 0 && elapsed.as_secs_f64() > 0.0 {
            Some((self.planned_written_bytes as f64 / elapsed.as_secs_f64()) as u64)
        } else {
            None
        };
        let eta = average_rate_bps.filter(|rate| *rate > 0).map(|rate| {
            Duration::from_secs_f64(
                self.planned_total_bytes
                    .saturating_sub(self.planned_written_bytes) as f64
                    / rate as f64,
            )
        });
        yaoshi_screen::WriteModel {
            operation: self.operation,
            target_dev_path: self.target_dev_path.clone(),
            task: self.task,
            head_scrub_written_bytes: self.head_scrub_written_bytes,
            target_head_scrub_bytes: self.target_head_scrub_bytes,
            tail_scrub_written_bytes: self.tail_scrub_written_bytes,
            target_tail_scrub_bytes: self.target_tail_scrub_bytes,
            planned_written_bytes: self.planned_written_bytes,
            planned_total_bytes: self.planned_total_bytes,
            source_read_bytes: self.source_read_bytes,
            payload_source_bytes: self.payload_source_bytes,
            zero_written_bytes: self.zero_written_bytes,
            payload_zero_extent_bytes: self.payload_zero_extent_bytes,
            target_image_bytes: self.target_image_bytes,
            current_rate_bps: self.current_rate_bps,
            average_rate_bps,
            eta,
        }
    }
}

fn render_write_progress(
    tty: &mut File,
    serial: &mut Option<File>,
    state: &WriteProgressState,
) -> Result<(), String> {
    let model = state.model();
    render_page_audit(tty, serial, "installer-write", |width, height| {
        write_page(&model, width, height)
    })
}

fn render_target_write_progress(
    tty: &mut File,
    serial: &mut Option<File>,
    state: &WriteProgressState,
) -> Result<(), String> {
    render_write_progress(tty, serial, state).map_err(|_| "target-write-failed".to_string())
}

fn mark_write_task(serial: &mut Option<File>, task: WriteTask, status: &str) {
    mark(serial, &markers::write_task(task, status));
}

fn write_target(
    source: &Source,
    target: OpenedTarget,
    tty: &mut File,
    serial: &mut Option<File>,
) -> Result<WriteResult, String> {
    if source.payload_extent_count == 0 || source.payload_planned_extent_bytes == 0 {
        return Err("payload-validation-failed".to_string());
    }

    let target_dev_path = target.target.disk.dev_path.clone();
    let target_stable_id = target
        .target
        .disk
        .stable_disk_id
        .as_ref()
        .map(|path| path.display().to_string());
    let target_size = target.target.disk.byte_size;
    let mut target_file = target.file;
    let mut progress = WriteProgressState::install(target_dev_path.clone(), target_size, source);

    scrub_install_target(&mut target_file, &mut progress, tty, serial)?;
    copy_payload_to_target(source, &mut target_file, &mut progress, tty, serial)?;
    finalize_target_writes(target_file, &mut progress, tty, serial)?;

    Ok(WriteResult {
        target_dev_path,
        target_stable_id,
        bytes_written: progress.planned_written_bytes,
    })
}

fn scrub_install_target(
    target_file: &mut File,
    progress: &mut WriteProgressState,
    tty: &mut File,
    serial: &mut Option<File>,
) -> Result<(), String> {
    render_target_write_progress(tty, serial, progress)?;
    mark_write_task(serial, WriteTask::VerifyTarget, "done");
    mark(serial, markers::TARGET_WRITE_STARTED);

    progress.task = WriteTask::PrepareDiskBeginning;
    mark_write_task(serial, progress.task, "active");
    render_target_write_progress(tty, serial, progress)?;
    write_zero_range(target_file, 0, progress.target_head_scrub_bytes)
        .map_err(|_| "target-write-failed".to_string())?;
    progress.head_scrub_written_bytes = progress.target_head_scrub_bytes;
    render_target_write_progress(tty, serial, progress)?;
    mark_write_task(serial, progress.task, "done");

    progress.task = WriteTask::PrepareDiskEnd;
    mark_write_task(serial, progress.task, "active");
    render_target_write_progress(tty, serial, progress)?;
    let tail_offset = progress
        .target_image_bytes
        .saturating_sub(progress.target_tail_scrub_bytes);
    write_zero_range(target_file, tail_offset, progress.target_tail_scrub_bytes)
        .map_err(|_| "target-write-failed".to_string())?;
    progress.tail_scrub_written_bytes = progress.target_tail_scrub_bytes;
    render_target_write_progress(tty, serial, progress)?;
    mark_write_task(serial, progress.task, "done");

    Ok(())
}

fn copy_payload_to_target(
    source: &Source,
    target_file: &mut File,
    progress: &mut WriteProgressState,
    tty: &mut File,
    serial: &mut Option<File>,
) -> Result<(), String> {
    progress.task = WriteTask::CopyYaoshiImage;
    progress.started = Instant::now();
    progress.current_rate_bps = None;
    mark_write_task(serial, progress.task, "active");

    let mut last_render = Instant::now();
    let mut last_copied = 0u64;
    let mut last_rate = None;
    let mut processed_extents = 0u64;
    render_target_write_progress(tty, serial, progress)?;

    let mut payload_progress = |p: yaoshi_payload::PayloadWriteProgress| {
        progress.planned_written_bytes = p.planned_written_bytes;
        progress.source_read_bytes = p.source_read_bytes;
        progress.payload_source_bytes = p.payload_source_bytes;
        progress.zero_written_bytes = p.zero_written_bytes;
        processed_extents = processed_extents.saturating_add(1);
        mark(
            serial,
            &markers::write_progress(markers::WriteProgress {
                processed_extent_count: processed_extents,
                payload_extent_count: source.payload_extent_count,
                planned_written_bytes: p.planned_written_bytes,
                payload_planned_extent_bytes: p.planned_total_bytes,
                source_read_bytes: p.source_read_bytes,
                payload_source_bytes: p.payload_source_bytes,
                zero_written_bytes: p.zero_written_bytes,
                payload_zero_extent_bytes: progress.payload_zero_extent_bytes,
            }),
        );

        let now = Instant::now();
        if now.duration_since(last_render) >= Duration::from_secs(1)
            || progress.planned_written_bytes == progress.planned_total_bytes
        {
            let sample_elapsed = now.duration_since(last_render).as_secs_f64();
            let current_rate = if sample_elapsed > 0.0 {
                Some(((progress.planned_written_bytes - last_copied) as f64 / sample_elapsed) as u64)
            } else {
                None
            };
            if let Some(rate) = current_rate {
                last_rate = Some(rate);
            }
            progress.current_rate_bps = current_rate;
            let _ = render_write_progress(tty, serial, progress);
            last_render = now;
            last_copied = progress.planned_written_bytes;
        }
    };

    yaoshi_payload::write_payload_region_to_target(
        &source.disk.dev_path,
        source.payload_offset,
        source.payload_size,
        target_file,
        &mut payload_progress,
    )
    .map_err(|_| "payload-validation-failed".to_string())?;

    if progress.planned_written_bytes != progress.planned_total_bytes {
        return Err("target-write-failed".to_string());
    }
    progress.current_rate_bps = last_rate;
    render_target_write_progress(tty, serial, progress)?;
    mark_write_task(serial, progress.task, "done");
    Ok(())
}

fn finalize_target_writes(
    target_file: File,
    progress: &mut WriteProgressState,
    tty: &mut File,
    serial: &mut Option<File>,
) -> Result<(), String> {
    progress.task = WriteTask::FinalizeWrites;
    mark_write_task(serial, progress.task, "active");
    render_target_write_progress(tty, serial, progress)?;
    target_file
        .sync_all()
        .map_err(|_| "target-write-failed".to_string())?;
    render_target_write_progress(tty, serial, progress)?;
    nix::unistd::syncfs(&target_file).map_err(|_| "target-write-failed".to_string())?;
    render_target_write_progress(tty, serial, progress)?;
    sync();
    mark_write_task(serial, progress.task, "done");
    drop(target_file);
    render_target_write_progress(tty, serial, progress)
}

fn erase_target(
    mut target: OpenedTarget,
    tty: &mut File,
    serial: &mut Option<File>,
) -> Result<WriteResult, String> {
    let target_dev_path = target.target.disk.dev_path.clone();
    let target_stable_id = target
        .target
        .disk
        .stable_disk_id
        .as_ref()
        .map(|path| path.display().to_string());
    let target_size = target.target.disk.byte_size;
    let mut progress = WriteProgressState::erase(target_dev_path.clone(), target_size);
    let total = progress.planned_total_bytes;

    mark_write_task(serial, WriteTask::VerifyTarget, "done");
    mark(serial, markers::TARGET_WRITE_STARTED);

    progress.task = WriteTask::PrepareDiskBeginning;
    mark_write_task(serial, progress.task, "active");
    render_target_write_progress(tty, serial, &progress)?;
    write_zero_range(&mut target.file, 0, progress.target_head_scrub_bytes)
        .map_err(|_| "target-write-failed".to_string())?;
    progress.head_scrub_written_bytes = progress.target_head_scrub_bytes;
    progress.planned_written_bytes = progress.head_scrub_written_bytes;
    progress.zero_written_bytes = progress.head_scrub_written_bytes;
    render_target_write_progress(tty, serial, &progress)?;
    mark_write_task(serial, progress.task, "done");

    progress.task = WriteTask::PrepareDiskEnd;
    mark_write_task(serial, progress.task, "active");
    let tail_offset = target_size.saturating_sub(progress.target_tail_scrub_bytes);
    write_zero_range(&mut target.file, tail_offset, progress.target_tail_scrub_bytes)
        .map_err(|_| "target-write-failed".to_string())?;
    progress.tail_scrub_written_bytes = progress.target_tail_scrub_bytes;
    progress.planned_written_bytes = total;
    progress.zero_written_bytes = total;
    render_target_write_progress(tty, serial, &progress)?;
    mark_write_task(serial, progress.task, "done");
    mark_write_task(serial, WriteTask::CopyYaoshiImage, "done");

    progress.task = WriteTask::FinalizeWrites;
    mark_write_task(serial, progress.task, "active");
    target
        .file
        .sync_all()
        .map_err(|_| "target-write-failed".to_string())?;
    nix::unistd::syncfs(&target.file).map_err(|_| "target-write-failed".to_string())?;
    sync();
    mark_write_task(serial, progress.task, "done");
    drop(target.file);
    Ok(WriteResult {
        target_dev_path,
        target_stable_id,
        bytes_written: total,
    })
}

fn write_zero_range(file: &mut File, offset: u64, len: u64) -> Result<(), String> {
    if !offset.is_multiple_of(SECTOR_SIZE) || !len.is_multiple_of(SECTOR_SIZE) {
        return Err("zero range is not 512-byte aligned".to_string());
    }
    const BLKZEROOUT: libc::Ioctl = 0x127f;
    let mut range = [offset, len];
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), BLKZEROOUT, range.as_mut_ptr()) };
    if rc == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(code) if code == libc::ENOTTY || code == libc::EOPNOTSUPP || code == libc::EINVAL => {}
        _ => return Err(err.to_string()),
    }
    let mut remaining = len;
    let zeros = vec![0u8; 1024 * 1024];
    let mut cursor = offset;
    while remaining > 0 {
        let n = remaining.min(zeros.len() as u64) as usize;
        file.write_all_at(&zeros[..n], cursor)
            .map_err(|e| e.to_string())?;
        cursor += n as u64;
        remaining -= n as u64;
    }
    Ok(())
}

fn erase_scrub_lengths(target_size: u64) -> (u64, u64) {
    let head = TARGET_HEAD_SCRUB_BYTES.min(target_size);
    let remaining = target_size.saturating_sub(head);
    let tail = TARGET_TAIL_SCRUB_BYTES.min(remaining);
    (head, tail)
}

enum CompleteAction {
    ChooseTarget,
}

fn wait_complete_exit(
    tty: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
    mut complete: yaoshi_screen::DoneModel,
) -> CompleteAction {
    loop {
        match read_input(tty) {
            Some(yaoshi_screen::Input::Tab) => {
                complete.focus = next_done_focus(complete.operation, complete.focus);
                render_page(tty, |width, height| done_page(&complete, width, height)).ok();
            }
            Some(yaoshi_screen::Input::Enter) => {
                let action = match complete.focus {
                    yaoshi_screen::DoneFocus::ChooseTarget
                        if complete.operation == yaoshi_screen::TargetOperation::Erase =>
                    {
                        return CompleteAction::ChooseTarget;
                    }
                    yaoshi_screen::DoneFocus::ChooseTarget => "reboot",
                    yaoshi_screen::DoneFocus::Reboot => "reboot",
                    yaoshi_screen::DoneFocus::PowerOff => "poweroff",
                };
                render_page(tty, |width, height| exit_page(action, width, height)).ok();
                pause_for_exit_scene_review();
                controlled_exit(serial, action, tty, original_termios);
            }
            Some(yaoshi_screen::Input::Resize { .. }) => {
                render_page(tty, |width, height| done_page(&complete, width, height)).ok();
            }
            _ => {}
        }
    }
}

fn next_done_focus(
    operation: yaoshi_screen::TargetOperation,
    focus: yaoshi_screen::DoneFocus,
) -> yaoshi_screen::DoneFocus {
    match operation {
        yaoshi_screen::TargetOperation::Install => match focus {
            yaoshi_screen::DoneFocus::PowerOff => yaoshi_screen::DoneFocus::Reboot,
            _ => yaoshi_screen::DoneFocus::PowerOff,
        },
        yaoshi_screen::TargetOperation::Erase => match focus {
            yaoshi_screen::DoneFocus::ChooseTarget => yaoshi_screen::DoneFocus::Reboot,
            yaoshi_screen::DoneFocus::Reboot => yaoshi_screen::DoneFocus::PowerOff,
            yaoshi_screen::DoneFocus::PowerOff => yaoshi_screen::DoneFocus::ChooseTarget,
        },
    }
}

fn wait_failure_exit(
    tty: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
    reason: &str,
    affected_disk: Option<PathBuf>,
    writes_started: bool,
) {
    let deadline = Instant::now() + BLOCKING_STOP_POWEROFF_TIMEOUT;
    loop {
        let remaining = remaining_seconds(deadline);
        render_page(tty, |width, height| {
            failure_page(
                reason,
                affected_disk.clone(),
                writes_started,
                Some(remaining),
                width,
                height,
            )
        })
        .ok();
        let next_tick = (Instant::now() + Duration::from_secs(1)).min(deadline);
        while Instant::now() < next_tick {
            if input_ready(tty, 100) {
                match read_input(tty) {
                    Some(yaoshi_screen::Input::Enter) => {
                        render_page(tty, |width, height| exit_page("poweroff", width, height)).ok();
                        pause_for_exit_scene_review();
                        controlled_exit(serial, "poweroff", tty, original_termios);
                    }
                    Some(yaoshi_screen::Input::Resize { .. }) => break,
                    _ => {}
                }
            }
        }
        if Instant::now() >= deadline {
            render_page(tty, |width, height| exit_page("poweroff", width, height)).ok();
            pause_for_exit_scene_review();
            controlled_exit(serial, "poweroff", tty, original_termios);
        }
    }
}

fn remaining_seconds(deadline: Instant) -> u64 {
    let remaining = deadline.saturating_duration_since(Instant::now());
    remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
}

fn controlled_exit(
    serial: &mut Option<File>,
    action: &str,
    tty: &mut File,
    original_termios: Option<&Termios>,
) -> ! {
    if let Some(termios) = original_termios {
        let _ = tcsetattr(&mut *tty, SetArg::TCSANOW, termios);
    }
    let _ = yaoshi_screen::terminal::restore_tty_for_exit(tty);
    sync();
    mark(serial, &markers::controlled_exit(action));
    let result = match action {
        "reboot" => reboot(RebootMode::RB_AUTOBOOT),
        _ => reboot(RebootMode::RB_POWER_OFF),
    };
    let line = match result {
        Ok(_) => "Yaoshi installer halted: machine did not exit.\n".to_string(),
        Err(err) => format!("Yaoshi installer halted: machine did not exit: {err}\n"),
    };
    let _ = tty.write_all(line.as_bytes());
    let _ = tty.flush();
    let _ = write_console(&line);
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn pause_for_exit_scene_review() {
    thread::sleep(Duration::from_secs(1));
}
