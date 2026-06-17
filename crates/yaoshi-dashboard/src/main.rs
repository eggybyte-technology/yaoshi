mod sampler;

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use sampler::Sampler;
use yaoshi_common::markers;
use yaoshi_screen::{NegotiatedDisplay, Surface};

const FULL_REDRAW_INTERVAL: Duration = Duration::from_secs(30);

fn main() {
    let mut sampler = Sampler::default();
    let mut terminal = match yaoshi_screen::terminal::TerminalSession::enter_tty(
        Path::new("/dev/tty1"),
        Surface::Dashboard,
        true,
    ) {
        Ok(terminal) => terminal,
        Err(err) => {
            eprintln!("yaoshi-dashboard: terminal initialization failed: {err}");
            std::process::exit(1);
        }
    };
    let mut serial = open_serial();
    mark_display(&mut serial, terminal.display());
    let mut startup_full_redraws = 5u8;
    let mut last_full_redraw = Instant::now();
    loop {
        let (width, height) = terminal.size();
        let display = terminal.display().clone();
        let frame = yaoshi_screen::dashboard::scene(&sampler.sample(display), (width, height));
        if startup_full_redraws > 0 || last_full_redraw.elapsed() >= FULL_REDRAW_INTERVAL {
            terminal.force_full_redraw();
            startup_full_redraws = startup_full_redraws.saturating_sub(1);
            last_full_redraw = Instant::now();
        }
        if terminal.render_frame(&frame).is_err() {
            terminal.force_full_redraw();
            thread::sleep(Duration::from_millis(1000));
            let _ = terminal.refresh_display();
            continue;
        }
        emit_screen_frame(&mut serial, "dashboard", width, height, &frame);
        thread::sleep(Duration::from_millis(1000));
        let _ = terminal.refresh_display();
    }
}

fn open_serial() -> Option<File> {
    OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open("/dev/ttyS0")
        .ok()
}

fn mark_display(serial: &mut Option<File>, display: &NegotiatedDisplay) {
    let Some(mut file) = serial.take() else {
        return;
    };
    if write_serial_line(&mut file, &display.serial_marker(), Duration::from_secs(1)).is_ok()
        && file.flush().is_ok()
    {
        *serial = Some(file);
    }
}

fn emit_screen_frame(
    serial: &mut Option<File>,
    screen_id: &str,
    columns: u16,
    rows: u16,
    frame: &yaoshi_screen::RenderedFrame,
) {
    let Some(mut file) = serial.take() else {
        return;
    };
    let (serial_columns, serial_rows) = serial_frame_rows(frame).unwrap_or_else(|| {
        (
            columns,
            frame
                .cells_text
                .lines()
                .map(|line| line.trim_end_matches(' ').to_string())
                .take(rows as usize)
                .collect::<Vec<_>>(),
        )
    });
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut ok = write_serial_line_until(
        &mut file,
        &markers::screen_frame_begin(screen_id, serial_columns, serial_rows.len() as u16),
        deadline,
    )
    .is_ok();
    for (row, line) in serial_rows.iter().enumerate() {
        if !ok {
            break;
        }
        ok = write_serial_line_until(
            &mut file,
            &markers::screen_frame_row(screen_id, row, line),
            deadline,
        )
        .is_ok();
    }
    if ok {
        let style = format!(
            "row=0 col=0 len={} style=Plain fg=white bg=default modifier=none",
            serial_columns
        );
        ok = write_serial_line_until(
            &mut file,
            &markers::screen_frame_style(screen_id, 0, &style),
            deadline,
        )
        .is_ok();
    }
    if ok {
        ok = write_serial_line_until(&mut file, &markers::screen_frame_end(screen_id), deadline)
            .is_ok();
    } else {
        let _ = file.write_all(b"\n");
    }
    let _ = ok.then(|| file.flush()).transpose();
    *serial = Some(file);
}

fn serial_frame_rows(frame: &yaoshi_screen::RenderedFrame) -> Option<(u16, Vec<String>)> {
    let min_x = frame.region_tree.iter().map(|region| region.rect.x).min()?;
    let min_y = frame.region_tree.iter().map(|region| region.rect.y).min()?;
    let max_x = frame
        .region_tree
        .iter()
        .map(|region| region.rect.x.saturating_add(region.rect.width))
        .max()?;
    let max_y = frame
        .region_tree
        .iter()
        .map(|region| region.rect.y.saturating_add(region.rect.height))
        .max()?;
    let width = max_x.checked_sub(min_x)?;
    let lines = frame.cells_text.lines().collect::<Vec<_>>();
    let mut rows = Vec::new();
    for y in min_y..max_y {
        let line = lines.get(y as usize)?;
        let start = min_x as usize;
        let end = max_x as usize;
        let slice = line.get(start..end)?;
        rows.push(slice.trim_end_matches(' ').to_string());
    }
    Some((width, rows))
}

fn write_serial_line(file: &mut File, line: &str, timeout: Duration) -> std::io::Result<()> {
    let deadline = Instant::now() + timeout;
    write_serial_line_until(file, line, deadline)
}

fn write_serial_line_until(file: &mut File, line: &str, deadline: Instant) -> std::io::Result<()> {
    write_serial_bytes_until(file, line.as_bytes(), deadline)?;
    write_serial_bytes_until(file, b"\n", deadline)
}

fn write_serial_bytes_until(
    file: &mut File,
    mut bytes: &[u8],
    deadline: Instant,
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        match file.write(bytes) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    ErrorKind::WriteZero,
                    "serial write returned zero",
                ));
            }
            Ok(n) => bytes = &bytes[n..],
            Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                if Instant::now() >= deadline || !wait_serial_writable(file, deadline) {
                    return Err(err);
                }
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn wait_serial_writable(file: &File, deadline: Instant) -> bool {
    let now = Instant::now();
    if now >= deadline {
        return false;
    }
    let timeout = deadline
        .saturating_duration_since(now)
        .min(Duration::from_millis(25));
    let mut pollfd = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLOUT,
        revents: 0,
    };
    unsafe { libc::poll(&mut pollfd, 1, timeout.as_millis() as i32) > 0 }
}
