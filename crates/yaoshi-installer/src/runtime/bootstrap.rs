fn main() {
    install_panic_handler();
    if let Err(err) = run() {
        let _ = write_console(&format!("Yaoshi installer startup failure: {err}\n"));
        fatal_startup_poweroff(&err);
    }
}

fn run() -> Result<(), String> {
    if getpid() != Pid::from_raw(1) {
        return Err("not running as PID 1".to_string());
    }
    let mut phase = InstallerPhase::Start;
    let _ = &phase;
    let _console = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/console")
        .map_err(|e| format!("open /dev/console: {e}"))?;
    mount_runtime_fs()?;
    fs::create_dir_all("/run/yaoshi").map_err(|e| format!("create /run/yaoshi: {e}"))?;
    let mut tty1 = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty1")
        .map_err(|e| format!("open /dev/tty1: {e}"))?;
    let original_termios = enter_raw_mode(&tty1)?;
    install_resize_handler()?;
    let mut serial = open_serial();
    if serial.is_none() {
        fs::write("/run/yaoshi/serial-unavailable", "ttyS0 unavailable\n")
            .map_err(|e| format!("record serial unavailable: {e}"))?;
    }
    let display = negotiate_supported_display(&mut tty1, &mut serial, Some(&original_termios))?;
    mark(&mut serial, &display.serial_marker());
    render_page_audit(
        &mut tty1,
        &mut serial,
        "installer-prepare",
        |width, height| {
            prepare_page(
                &display,
                "not-started",
                "not-started",
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
        },
    )?;
    mark(&mut serial, markers::FIRST_FRAME_FLUSHED);
    mark(&mut serial, markers::MODULE_LOADING_STARTED);
    if load_modules(&mut tty1, &mut serial, Some(&original_termios), &display).is_err() {
        phase = InstallerPhase::Failure(FailureReason::ModuleLoadFailed);
        let _ = &phase;
        let reason = FailureReason::ModuleLoadFailed.as_str();
        render_page(&mut tty1, |width, height| {
            failure_page(
                reason,
                None,
                false,
                Some(BLOCKING_STOP_POWEROFF_TIMEOUT.as_secs()),
                width,
                height,
            )
        })?;
        wait_failure_exit(
            &mut tty1,
            &mut serial,
            Some(&original_termios),
            reason,
            None,
            false,
        );
        return Ok(());
    }
    mark(&mut serial, markers::MODULE_LOADING_COMPLETE);
    phase = InstallerPhase::DiscoverSource;
    let _ = &phase;
    render_page_audit(
        &mut tty1,
        &mut serial,
        "installer-prepare",
        |width, height| {
            prepare_page(
                &display,
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
        },
    )?;
    let source = match discover_source(&mut tty1, &mut serial, Some(&original_termios), &display) {
        Ok(source) => source,
        Err(reason) => {
            let failure_phase = InstallerPhase::Failure(match reason.as_str() {
                "installation-media-not-found" => FailureReason::SourceNotFound,
                "installation-media-ambiguous" => FailureReason::SourceAmbiguous,
                "disk-inspection-failed" => FailureReason::DiskInspectionFailed,
                _ => FailureReason::InternalError,
            });
            let _ = &failure_phase;
            render_page(&mut tty1, |width, height| {
                failure_page(
                    &reason,
                    None,
                    false,
                    Some(BLOCKING_STOP_POWEROFF_TIMEOUT.as_secs()),
                    width,
                    height,
                )
            })?;
            wait_failure_exit(
                &mut tty1,
                &mut serial,
                Some(&original_termios),
                &reason,
                None,
                false,
            );
            return Ok(());
        }
    };
    phase = InstallerPhase::InspectDisks;
    let _ = &phase;
    select_loop(&source, &mut tty1, &mut serial, Some(&original_termios))?;
    Ok(())
}

fn install_panic_handler() {
    std::panic::set_hook(Box::new(|info| {
        let _ = write_console(&format!("Yaoshi installer panic: {info}\n"));
    }));
}

fn fatal_startup_poweroff(reason: &str) -> ! {
    let _ = write_console(&format!(
        "Yaoshi installer will power off automatically in {} s: {reason}\n",
        BLOCKING_STOP_POWEROFF_TIMEOUT.as_secs()
    ));
    thread::sleep(BLOCKING_STOP_POWEROFF_TIMEOUT);
    sync();
    let result = reboot(RebootMode::RB_POWER_OFF);
    let line = match result {
        Ok(_) => "Yaoshi installer halted: machine did not exit.\n".to_string(),
        Err(err) => format!("Yaoshi installer halted: machine did not exit: {err}\n"),
    };
    let _ = write_console(&line);
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn install_resize_handler() -> Result<(), String> {
    unsafe extern "C" fn handle_sigwinch(_: libc::c_int) {
        RESIZE_PENDING.store(true, Ordering::SeqCst);
    }

    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handle_sigwinch as *const () as usize;
    action.sa_flags = 0;
    let rc = unsafe {
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGWINCH, &action, std::ptr::null_mut())
    };
    if rc == 0 {
        Ok(())
    } else {
        Err("install SIGWINCH handler".to_string())
    }
}

fn mount_runtime_fs() -> Result<(), String> {
    mount(
        Some("devtmpfs"),
        "/dev",
        Some("devtmpfs"),
        MsFlags::empty(),
        None::<&str>,
    )
    .map_err(|e| format!("mount devtmpfs: {e}"))?;
    mount(
        Some("proc"),
        "/proc",
        Some("proc"),
        MsFlags::empty(),
        None::<&str>,
    )
    .map_err(|e| format!("mount proc: {e}"))?;
    mount(
        Some("sysfs"),
        "/sys",
        Some("sysfs"),
        MsFlags::empty(),
        None::<&str>,
    )
    .map_err(|e| format!("mount sysfs: {e}"))?;
    mount(
        Some("tmpfs"),
        "/run",
        Some("tmpfs"),
        MsFlags::empty(),
        None::<&str>,
    )
    .map_err(|e| format!("mount run tmpfs: {e}"))?;
    Ok(())
}

fn load_modules(
    tty1: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
    display: &yaoshi_screen::NegotiatedDisplay,
) -> Result<(), String> {
    let text =
        fs::read_to_string("/YAOSHI-MODULES").map_err(|e| format!("read /YAOSHI-MODULES: {e}"))?;
    let modules = text
        .lines()
        .map(str::trim)
        .filter(|module| !module.is_empty())
        .collect::<Vec<_>>();
    if modules.is_empty() {
        return Err("module list is empty".to_string());
    }
    let module_count = modules.len();
    for (index, module) in modules.iter().enumerate() {
        if module.contains("..") || module.starts_with('/') {
            return Err(format!("invalid module path: {module}"));
        }
        render_module_loading_progress(tty1, display, index, module_count)?;
        mark(serial, &markers::module_loading_module_started(module));
        match load_module_with_timeout(tty1, serial, original_termios, module, MODULE_LOAD_TIMEOUT)
        {
            Ok(()) => {
                mark(serial, &markers::module_loading_module_complete(module));
                render_module_loading_progress(tty1, display, index + 1, module_count)?;
            }
            Err(err) => {
                mark(
                    serial,
                    &markers::module_loading_module_failed(module, &marker_reason(&err)),
                );
                return Err(err);
            }
        }
    }
    Ok(())
}

fn load_module_with_timeout(
    tty1: &mut File,
    serial: &mut Option<File>,
    original_termios: Option<&Termios>,
    module: &str,
    timeout: Duration,
) -> Result<(), String> {
    let module = module.to_string();
    let (tx, rx) = mpsc::channel();
    let worker_module = module.clone();
    thread::spawn(move || {
        let result = load_one_module(&worker_module);
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + timeout;
    loop {
        handle_preparing_input(tty1, serial, original_termios)?;
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(format!("load module {module}: timed out after 30s"));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(format!("load module {module}: loader thread exited"));
            }
        }
        handle_preparing_input(tty1, serial, original_termios)?;
    }
}

fn open_serial() -> Option<File> {
    OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open("/dev/ttyS0")
        .ok()
}

fn load_one_module(module: &str) -> Result<(), String> {
    let file =
        File::open(format!("/{module}")).map_err(|e| format!("open module {module}: {e}"))?;
    let rc = unsafe { libc::syscall(libc::SYS_finit_module, file.as_raw_fd(), c"".as_ptr(), 0) };
    if rc == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EEXIST) {
        return Ok(());
    }
    Err(format!("load module {module}: {err}"))
}

fn render_module_loading_progress(
    tty1: &mut File,
    display: &yaoshi_screen::NegotiatedDisplay,
    loaded: usize,
    module_count: usize,
) -> Result<(), String> {
    let modules_state = if loaded >= module_count {
        format!("loaded {loaded} / {module_count}")
    } else {
        format!("loading {loaded} / {module_count}")
    };
    render_page(tty1, |width, height| {
        prepare_page(
            display,
            &modules_state,
            "not-started",
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
    })
}

fn marker_reason(reason: &str) -> String {
    reason
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
