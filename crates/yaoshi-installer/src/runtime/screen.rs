fn enter_raw_mode(tty: &File) -> Result<Termios, String> {
    let original = tcgetattr(tty).map_err(|e| e.to_string())?;
    let mut raw = original.clone();
    cfmakeraw(&mut raw);
    tcsetattr(tty, SetArg::TCSANOW, &raw).map_err(|e| e.to_string())?;
    Ok(original)
}

fn read_input(tty: &mut File) -> Option<yaoshi_screen::Input> {
    if RESIZE_PENDING.swap(false, Ordering::SeqCst) {
        let (columns, rows) = terminal_size(tty);
        return Some(yaoshi_screen::Input::Resize { columns, rows });
    }
    let mut byte = [0u8; 1];
    if let Err(err) = tty.read_exact(&mut byte) {
        if err.kind() == ErrorKind::Interrupted || RESIZE_PENDING.swap(false, Ordering::SeqCst) {
            let (columns, rows) = terminal_size(tty);
            return Some(yaoshi_screen::Input::Resize { columns, rows });
        }
        return None;
    }
    match byte[0] {
        b'\r' | b'\n' => Some(yaoshi_screen::Input::Enter),
        b'\t' => Some(yaoshi_screen::Input::Tab),
        0x0c => None,
        0x7f | 0x08 => None,
        0x1b => read_escape_input(tty),
        b if b.is_ascii_graphic() || b == b' ' => None,
        _ => None,
    }
}

fn read_escape_input(tty: &mut File) -> Option<yaoshi_screen::Input> {
    if !input_ready(tty, 30) {
        return None;
    }
    let mut seq = [0u8; 2];
    if tty.read_exact(&mut seq[..1]).is_err() {
        return None;
    }
    if seq[0] != b'[' {
        return None;
    }
    if tty.read_exact(&mut seq[1..2]).is_err() {
        return None;
    }
    match seq[1] {
        b'A' => Some(yaoshi_screen::Input::ArrowUp),
        b'B' => Some(yaoshi_screen::Input::ArrowDown),
        _ => None,
    }
}

fn input_ready(tty: &File, timeout_ms: u16) -> bool {
    if RESIZE_PENDING.load(Ordering::SeqCst) {
        return true;
    }
    let mut fds = [PollFd::new(tty.as_fd(), PollFlags::POLLIN)];
    poll(&mut fds, timeout_ms)
        .map(|n| n > 0 || RESIZE_PENDING.load(Ordering::SeqCst))
        .unwrap_or_else(|_| RESIZE_PENDING.load(Ordering::SeqCst))
}

#[expect(
    clippy::too_many_arguments,
    reason = "installer prepare page is an adapter from runtime probe state to screen model"
)]
fn prepare_page(
    display: &yaoshi_screen::NegotiatedDisplay,
    modules_state: &str,
    media_state: &str,
    payload_state: &str,
    disk_inspection_state: &str,
    elapsed: Duration,
    target_minimum_bytes: Option<u64>,
    payload_planned_extent_bytes: Option<u64>,
    payload_container_bytes: Option<u64>,
    payload_zero_extent_bytes: Option<u64>,
    width: u16,
    height: u16,
) -> yaoshi_screen::RenderedFrame {
    yaoshi_screen::render_installer_scene(
        &yaoshi_screen::InstallerScene::Prepare(yaoshi_screen::PrepareModel {
            display: display.clone(),
            media_check_elapsed: elapsed,
            modules_state: modules_state.to_string(),
            media_state: media_state.to_string(),
            payload_state: payload_state.to_string(),
            disk_inspection_state: disk_inspection_state.to_string(),
            target_minimum_bytes,
            payload_planned_extent_bytes,
            payload_container_bytes,
            payload_zero_extent_bytes,
        }),
        width,
        height,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "installer target page is an adapter from disk inventory state to screen model"
)]
fn target_page(
    candidates: &[TargetDiskCandidate],
    selected: usize,
    target_minimum_bytes: u64,
    payload_planned_extent_bytes: u64,
    payload_container_bytes: u64,
    focus: yaoshi_screen::TargetFocus,
    width: u16,
    height: u16,
) -> yaoshi_screen::RenderedFrame {
    let (
        candidate_total_count,
        selectable_count,
        installer_media_count,
        installed_target_count,
        blocked_by_installed_target_count,
        too_small_count,
        unsupported_sector_size_count,
        no_stable_id_count,
        read_error_count,
    ) = yaoshi_screen::candidate_counts(candidates);
    yaoshi_screen::render_installer_scene(
        &yaoshi_screen::InstallerScene::Target(yaoshi_screen::TargetModel {
            candidates: candidates.to_vec(),
            selected_visible: selected,
            focus,
            candidate_total_count,
            selectable_count,
            installer_media_count,
            installed_target_count,
            blocked_by_installed_target_count,
            too_small_count,
            unsupported_sector_size_count,
            no_stable_id_count,
            read_error_count,
            target_minimum_bytes,
            payload_planned_extent_bytes,
            payload_container_bytes,
        }),
        width,
        height,
    )
}

fn write_page(
    model: &yaoshi_screen::WriteModel,
    width: u16,
    height: u16,
) -> yaoshi_screen::RenderedFrame {
    yaoshi_screen::render_installer_scene(
        &yaoshi_screen::InstallerScene::Write(model.clone()),
        width,
        height,
    )
}

fn done_page(
    model: &yaoshi_screen::DoneModel,
    width: u16,
    height: u16,
) -> yaoshi_screen::RenderedFrame {
    yaoshi_screen::render_installer_scene(
        &yaoshi_screen::InstallerScene::Done(model.clone()),
        width,
        height,
    )
}

fn exit_page(action: &str, width: u16, height: u16) -> yaoshi_screen::RenderedFrame {
    let scene = if action == "reboot" {
        yaoshi_screen::InstallerScene::Reboot
    } else {
        yaoshi_screen::InstallerScene::Poweroff
    };
    yaoshi_screen::render_installer_scene(&scene, width, height)
}

fn failure_page(
    reason: &str,
    affected_disk: Option<PathBuf>,
    writes_started: bool,
    auto_poweroff_seconds: Option<u64>,
    width: u16,
    height: u16,
) -> yaoshi_screen::RenderedFrame {
    yaoshi_screen::render_installer_scene(
        &yaoshi_screen::InstallerScene::Stopped(yaoshi_screen::StoppedModel {
            reason: screen_failure_reason(reason),
            failed_step_name: failed_step_name(reason).to_string(),
            affected_disk,
            failure_write_state: if writes_started {
                yaoshi_screen::FailureWriteState::TargetWriteStarted
            } else {
                yaoshi_screen::FailureWriteState::NoTargetWrites
            },
            auto_poweroff_seconds,
        }),
        width,
        height,
    )
}

fn screen_failure_reason(reason: &str) -> yaoshi_screen::FailureReason {
    match reason {
        "installation-media-not-found" => yaoshi_screen::FailureReason::InstallationMediaNotFound,
        "installation-media-ambiguous" => yaoshi_screen::FailureReason::InstallationMediaAmbiguous,
        "payload-validation-failed" => yaoshi_screen::FailureReason::PayloadValidationFailed,
        "disk-inspection-failed" => yaoshi_screen::FailureReason::DiskInspectionFailed,
        "selected-disk-changed" => yaoshi_screen::FailureReason::SelectedDiskChanged,
        "target-open-failed" => yaoshi_screen::FailureReason::TargetOpenFailed,
        "target-write-failed" => yaoshi_screen::FailureReason::TargetWriteFailed,
        "module-load-failed" => yaoshi_screen::FailureReason::ModuleLoadFailed,
        _ => yaoshi_screen::FailureReason::InternalError,
    }
}

fn failed_step_name(reason: &str) -> &'static str {
    match reason {
        "installation-media-not-found" | "installation-media-ambiguous" => "media-discovery",
        "payload-validation-failed" => "payload-validation",
        "disk-inspection-failed" => "disk-inspection",
        "selected-disk-changed" => "target-revalidation",
        "target-open-failed" => "target-open",
        "target-write-failed" => "payload-write",
        "module-load-failed" => "module-loading",
        _ => "internal",
    }
}

fn render(tty: &mut File, page: &str) -> Result<(), String> {
    yaoshi_screen::terminal::render_text_to_tty(tty, page).map_err(|e| e.to_string())
}

fn render_page<F>(tty: &mut File, build: F) -> Result<(), String>
where
    F: FnOnce(u16, u16) -> yaoshi_screen::RenderedFrame,
{
    let (width, height) = terminal_size(tty);
    yaoshi_screen::terminal::render_frame_to_tty(tty, &build(width, height))
        .map_err(|e| e.to_string())
}

fn render_page_audit<F>(
    tty: &mut File,
    serial: &mut Option<File>,
    screen_id: &str,
    build: F,
) -> Result<(), String>
where
    F: FnOnce(u16, u16) -> yaoshi_screen::RenderedFrame,
{
    let (width, height) = terminal_size(tty);
    let frame = build(width, height);
    yaoshi_screen::terminal::render_frame_to_tty(tty, &frame).map_err(|e| e.to_string())?;
    emit_screen_frame(serial, screen_id, width, height, &frame);
    Ok(())
}

fn negotiate_supported_display(
    tty: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
) -> Result<yaoshi_screen::NegotiatedDisplay, String> {
    match yaoshi_screen::terminal::negotiate_display_for_tty(tty, yaoshi_screen::Surface::Installer)
    {
        yaoshi_screen::DisplayNegotiation::Supported(display) => Ok(display),
        yaoshi_screen::DisplayNegotiation::Unsupported(failure) => {
            wait_unsupported_display_exit(tty, serial, original_termios, &failure);
        }
    }
}

fn wait_unsupported_display_exit(
    tty: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
    failure: &yaoshi_screen::DisplayNegotiationFailure,
) -> ! {
    let deadline = Instant::now() + BLOCKING_STOP_POWEROFF_TIMEOUT;
    loop {
        let remaining = remaining_seconds(deadline);
        render(
            tty,
            &yaoshi_screen::unsupported_display_page_with_poweroff(failure, Some(remaining)),
        )
        .ok();
        let next_tick = (Instant::now() + Duration::from_secs(1)).min(deadline);
        while Instant::now() < next_tick {
            if input_ready(tty, 100)
                && let Some(yaoshi_screen::Input::Enter) = read_input(tty)
            {
                render_page(tty, |width, height| exit_page("poweroff", width, height)).ok();
                pause_for_exit_scene_review();
                controlled_exit(serial, "poweroff", tty, original_termios);
            }
        }
        if Instant::now() >= deadline {
            render_page(tty, |width, height| exit_page("poweroff", width, height)).ok();
            pause_for_exit_scene_review();
            controlled_exit(serial, "poweroff", tty, original_termios);
        }
    }
}

fn terminal_size(tty: &File) -> (u16, u16) {
    let mut winsize = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let rc = unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCGWINSZ, &mut winsize) };
    if rc == 0 && winsize.ws_col >= 1 && winsize.ws_row >= 1 {
        (winsize.ws_col, winsize.ws_row)
    } else {
        (0, 0)
    }
}
