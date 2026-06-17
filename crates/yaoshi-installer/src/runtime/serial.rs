fn mark(serial: &mut Option<File>, line: &str) {
    let Some(mut file) = serial.take() else {
        return;
    };
    if write_serial_line(&mut file, line, SERIAL_MARK_TIMEOUT).is_ok() && file.flush().is_ok() {
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
    let begin = markers::screen_frame_begin(screen_id, columns, rows);
    let mut ok = write_serial_line(&mut file, &begin, SERIAL_FRAME_LINE_TIMEOUT).is_ok();
    for (row, line) in frame.cells_text.lines().enumerate() {
        if !ok {
            break;
        }
        let row = markers::screen_frame_row(screen_id, row, line);
        ok = write_serial_line(&mut file, &row, SERIAL_FRAME_LINE_TIMEOUT).is_ok();
    }
    if ok {
        for (row, line) in yaoshi_screen::buffer_to_styles(&frame.buffer)
            .lines()
            .enumerate()
        {
            let style = markers::screen_frame_style(screen_id, row, line);
            ok = write_serial_line(&mut file, &style, SERIAL_FRAME_LINE_TIMEOUT).is_ok();
            if !ok {
                break;
            }
        }
    }
    if ok {
        ok = write_serial_line(
            &mut file,
            &markers::screen_frame_end(screen_id),
            SERIAL_FRAME_LINE_TIMEOUT,
        )
        .and_then(|_| file.flush())
        .is_ok();
    }
    let _ = ok;
    *serial = Some(file);
}

fn write_serial_line(file: &mut File, line: &str, timeout: Duration) -> std::io::Result<()> {
    write_serial_bytes(file, line.as_bytes(), timeout)?;
    write_serial_bytes(file, b"\n", timeout)
}

fn write_serial_bytes(file: &mut File, mut bytes: &[u8], timeout: Duration) -> std::io::Result<()> {
    let deadline = Instant::now() + timeout;
    while !bytes.is_empty() {
        match file.write(bytes) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    ErrorKind::WriteZero,
                    "serial write returned zero",
                ));
            }
            Ok(written) => bytes = &bytes[written..],
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
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
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout_ms = remaining.as_millis().clamp(1, 25) as u16;
        let mut fds = [PollFd::new(file.as_fd(), PollFlags::POLLOUT)];
        if poll(&mut fds, timeout_ms).map(|n| n > 0).unwrap_or(false) {
            return true;
        }
    }
    false
}

fn write_console(message: &str) -> std::io::Result<()> {
    OpenOptions::new()
        .write(true)
        .open("/dev/console")
        .and_then(|mut f| f.write_all(message.as_bytes()))
}

fn read_u64(path: impl AsRef<Path>) -> Result<u64, std::io::Error> {
    read_string(&path)?.parse().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("parse {} as u64: {e}", path.as_ref().display()),
        )
    })
}

fn read_string(path: impl AsRef<Path>) -> Result<String, std::io::Error> {
    Ok(fs::read_to_string(path)?.trim().to_string())
}
