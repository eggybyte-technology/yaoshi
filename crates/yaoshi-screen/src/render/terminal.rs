pub mod terminal {
    use super::*;

    const KDFONTOP: c_ulong = 0x4B72;
    const KD_FONT_OP_SET_TALL: u32 = 4;
    const TIOCGWINSZ: c_ulong = 0x5413;
    const FBIOGET_VSCREENINFO: c_ulong = 0x4600;
    const VT_ACTIVATE: c_ulong = 0x5606;

    #[repr(C)]
    struct ConsoleFontOp {
        op: u32,
        flags: u32,
        width: u32,
        height: u32,
        charcount: u32,
        data: *mut c_void,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct WinSize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    unsafe extern "C" {
        fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    }

    pub struct TerminalSession {
        tty: File,
        surface: Surface,
        raw_active: bool,
        display: NegotiatedDisplay,
        previous_frame: Option<RenderedFrame>,
    }

    impl TerminalSession {
        pub fn enter_tty(path: &Path, surface: Surface, raw: bool) -> io::Result<Self> {
            let mut tty = OpenOptions::new().read(true).write(true).open(path)?;
            let _ = activate_virtual_terminal(&tty, path);
            if raw {
                enable_raw_mode()?;
            }
            match negotiate_display_for_tty(&tty, surface) {
                DisplayNegotiation::Supported(display) => {
                    let mut session = Self {
                        tty,
                        surface,
                        raw_active: raw,
                        display,
                        previous_frame: None,
                    };
                    prepare_tty(&mut session.tty)?;
                    Ok(session)
                }
                DisplayNegotiation::Unsupported(failure) => {
                    let _ = prepare_tty(&mut tty);
                    let _ = render_text_to_tty(&mut tty, &unsupported_display_page(&failure));
                    if raw {
                        let _ = disable_raw_mode();
                    }
                    Err(io::Error::other(format!(
                        "display unsupported: {}",
                        failure.reason.as_str()
                    )))
                }
            }
        }

        pub fn tty_mut(&mut self) -> &mut File {
            &mut self.tty
        }

        pub fn size(&self) -> (u16, u16) {
            (self.display.columns, self.display.rows)
        }

        pub fn display(&self) -> &NegotiatedDisplay {
            &self.display
        }

        pub fn refresh_display(&mut self) -> DisplayNegotiation {
            let profile = CONSOLE_FONT_PROFILES
                .iter()
                .copied()
                .find(|profile| profile.name == self.display.font_profile)
                .unwrap_or(CURRENT_CONSOLE_FONT_PROFILE);
            let negotiation = read_display_observation(&self.tty)
                .map(|observed| negotiate_display_for_profile(self.surface, profile, observed))
                .unwrap_or_else(|_| {
                    DisplayNegotiation::Unsupported(DisplayNegotiationFailure {
                        surface: self.surface,
                        columns: self.display.columns,
                        rows: self.display.rows,
                        font_profile: Some(profile.name.to_string()),
                        reason: DisplayNegotiationReason::TerminalSizeUnreadable,
                        attempts: vec![format!("{} terminal grid unreadable", profile.name)],
                    })
                });
            if let DisplayNegotiation::Supported(next_display) = &negotiation
                && &self.display != next_display
            {
                self.previous_frame = None;
                self.display = next_display.clone();
            }
            negotiation
        }

        pub fn render_text(&mut self, text: &str) -> io::Result<()> {
            self.previous_frame = None;
            render_text_to_tty(&mut self.tty, text)
        }

        pub fn render_frame(&mut self, frame: &RenderedFrame) -> io::Result<()> {
            present_frame_to_tty(&mut self.tty, self.previous_frame.as_ref(), frame)?;
            self.previous_frame = Some(frame.clone());
            Ok(())
        }

        pub fn force_full_redraw(&mut self) {
            self.previous_frame = None;
        }

        pub fn exit_restore(&mut self) -> io::Result<()> {
            restore_tty_for_exit(&mut self.tty)?;
            if self.raw_active {
                disable_raw_mode()?;
                self.raw_active = false;
            }
            Ok(())
        }
    }

    impl Drop for TerminalSession {
        fn drop(&mut self) {
            let _ = self.exit_restore();
        }
    }

    pub fn prepare_tty(tty: &mut File) -> io::Result<()> {
        tty.execute(Hide)?;
        tty.execute(DisableLineWrap)?;
        tty.execute(Clear(ClearType::All))?;
        tty.execute(MoveTo(0, 0))?;
        tty.flush()
    }

    pub fn render_text_to_tty(tty: &mut File, text: &str) -> io::Result<()> {
        tty.execute(Clear(ClearType::All))?;
        tty.execute(MoveTo(0, 0))?;
        let output = text.trim_end_matches('\n').replace('\n', "\r\n");
        tty.write_all(output.as_bytes())?;
        tty.flush()
    }

    pub fn render_frame_to_tty(tty: &mut File, frame: &RenderedFrame) -> io::Result<()> {
        render_full_frame_to_tty(tty, frame)
    }

    pub fn present_frame_to_tty(
        tty: &mut File,
        previous: Option<&RenderedFrame>,
        next: &RenderedFrame,
    ) -> io::Result<()> {
        let full = previous.is_none_or(|previous| previous.buffer.area != next.buffer.area);
        if full {
            render_full_frame_to_tty(tty, next)
        } else {
            render_diff_frame_to_tty(tty, previous.expect("previous frame exists"), next)
        }
    }

    fn render_full_frame_to_tty(tty: &mut File, frame: &RenderedFrame) -> io::Result<()> {
        tty.queue(SetAttribute(Attribute::Reset))?;
        tty.queue(ResetColor)?;
        tty.queue(Clear(ClearType::All))?;
        tty.queue(MoveTo(0, 0))?;
        let area = frame.buffer.area;
        let mut current_style = None;
        for y in area.y..area.y + area.height {
            tty.queue(MoveTo(0, y))?;
            for x in area.x..area.x + area.width {
                let cell = &frame.buffer[(x, y)];
                let semantic_style = super::style_from_cell(cell.style());
                if current_style != Some(semantic_style) {
                    queue_semantic_style(tty, semantic_style)?;
                    current_style = Some(semantic_style);
                }
                tty.queue(Print(cell.symbol()))?;
            }
        }
        tty.queue(ResetColor)?;
        tty.queue(SetAttribute(Attribute::Reset))?;
        tty.queue(MoveTo(0, 0))?;
        tty.flush()
    }

    fn render_diff_frame_to_tty(
        tty: &mut File,
        previous: &RenderedFrame,
        next: &RenderedFrame,
    ) -> io::Result<()> {
        tty.queue(Hide)?;
        let area = next.buffer.area;
        let mut current_style = None;
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                let next_cell = &next.buffer[(x, y)];
                let previous_cell = &previous.buffer[(x, y)];
                if next_cell.symbol() == previous_cell.symbol()
                    && next_cell.style() == previous_cell.style()
                {
                    continue;
                }
                tty.queue(MoveTo(x, y))?;
                let semantic_style = super::style_from_cell(next_cell.style());
                if current_style != Some(semantic_style) {
                    queue_semantic_style(tty, semantic_style)?;
                    current_style = Some(semantic_style);
                }
                tty.queue(Print(next_cell.symbol()))?;
            }
        }
        tty.flush()
    }

    fn queue_semantic_style(tty: &mut File, style: SemanticStyle) -> io::Result<()> {
        let (fg, _bg, modifier) = semantic_style_definition(style);
        tty.queue(SetAttribute(Attribute::Reset))?;
        tty.queue(SetForegroundColor(crossterm_color(fg)))?;
        if modifier.contains(Modifier::BOLD) {
            tty.queue(SetAttribute(Attribute::Bold))?;
        } else if modifier.contains(Modifier::DIM) {
            tty.queue(SetAttribute(Attribute::Dim))?;
        }
        Ok(())
    }

    fn crossterm_color(color: Color) -> CrosstermColor {
        match color {
            Color::Rgb(r, g, b) => CrosstermColor::Rgb { r, g, b },
            Color::Black => CrosstermColor::Black,
            Color::Red => CrosstermColor::DarkRed,
            Color::Green => CrosstermColor::DarkGreen,
            Color::Yellow => CrosstermColor::DarkYellow,
            Color::Blue => CrosstermColor::DarkBlue,
            Color::Magenta => CrosstermColor::DarkMagenta,
            Color::Cyan => CrosstermColor::DarkCyan,
            Color::Gray => CrosstermColor::Grey,
            Color::DarkGray => CrosstermColor::DarkGrey,
            Color::LightRed => CrosstermColor::Red,
            Color::LightGreen => CrosstermColor::Green,
            Color::LightYellow => CrosstermColor::Yellow,
            Color::LightBlue => CrosstermColor::Blue,
            Color::LightMagenta => CrosstermColor::Magenta,
            Color::LightCyan => CrosstermColor::Cyan,
            Color::White => CrosstermColor::White,
            _ => CrosstermColor::Reset,
        }
    }

    pub fn restore_tty_for_exit(tty: &mut File) -> io::Result<()> {
        tty.execute(Show)?;
        tty.execute(EnableLineWrap)?;
        tty.execute(Clear(ClearType::All))?;
        tty.execute(MoveTo(0, 0))?;
        tty.flush()
    }

    pub fn negotiate_display_for_tty(tty: &File, surface: Surface) -> DisplayNegotiation {
        let mut observations = Vec::new();
        let candidate_order = framebuffer_virtual_size()
            .map(|(_, height)| candidate_profiles_for_framebuffer_height(height))
            .unwrap_or_else(|_| candidate_profiles_for_framebuffer_height(0));
        for profile in candidate_order {
            let observed = set_vendored_terminus_font(tty, profile)
                .and_then(|_| read_display_observation(tty))
                .map_err(|_| "font-or-display-read-failed");
            observations.push((profile, observed));
        }
        let negotiated = negotiate_display_from_observations(surface, &observations);
        match negotiated {
            DisplayNegotiation::Supported(display) => {
                let Some(profile) = CONSOLE_FONT_PROFILES
                    .iter()
                    .copied()
                    .find(|profile| profile.name == display.font_profile)
                else {
                    return DisplayNegotiation::Unsupported(DisplayNegotiationFailure {
                        surface,
                        columns: display.columns,
                        rows: display.rows,
                        font_profile: Some(display.font_profile),
                        reason: DisplayNegotiationReason::ConsoleFontUnavailable,
                        attempts: vec!["selected profile unavailable".to_string()],
                    });
                };
                match set_vendored_terminus_font(tty, profile).and_then(|_| {
                    read_display_observation(tty).and_then(|observed| {
                        match negotiate_display_for_profile(surface, profile, observed) {
                            DisplayNegotiation::Supported(display) => Ok(display),
                            DisplayNegotiation::Unsupported(_) => {
                                Err(io::Error::other("selected profile ineffective"))
                            }
                        }
                    })
                }) {
                    Ok(display) => DisplayNegotiation::Supported(display),
                    Err(_) => DisplayNegotiation::Unsupported(DisplayNegotiationFailure {
                        surface,
                        columns: display.columns,
                        rows: display.rows,
                        font_profile: Some(profile.name.to_string()),
                        reason: DisplayNegotiationReason::FontApplicationNotEffective,
                        attempts: vec![format!("{} reapply failed", profile.name)],
                    }),
                }
            }
            unsupported => match read_display_observation(tty)
                .map(|observed| negotiate_display_from_current_console(surface, observed))
            {
                Ok(DisplayNegotiation::Supported(display)) => {
                    DisplayNegotiation::Supported(display)
                }
                _ => unsupported,
            },
        }
    }

    fn read_display_observation(tty: &File) -> io::Result<DisplayObservation> {
        let size = read_terminal_size(tty)?;
        let framebuffer = framebuffer_virtual_size().ok();
        let framebuffer_width = framebuffer
            .map(|(width, _)| width)
            .or_else(|| nonzero_u16(size.ws_xpixel).map(u32::from));
        let framebuffer_height = framebuffer
            .map(|(_, height)| height)
            .or_else(|| nonzero_u16(size.ws_ypixel).map(u32::from));
        Ok(DisplayObservation {
            columns: size.ws_col,
            rows: size.ws_row,
            framebuffer_width,
            framebuffer_height,
        })
    }

    fn read_terminal_size(tty: &File) -> io::Result<WinSize> {
        let mut size = WinSize::default();
        let rc = unsafe { ioctl(tty.as_raw_fd(), TIOCGWINSZ, &mut size) };
        if rc != 0 || size.ws_col == 0 || size.ws_row == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(size)
    }

    fn nonzero_u16(value: u16) -> Option<u16> {
        if value == 0 { None } else { Some(value) }
    }

    fn activate_virtual_terminal(tty: &File, path: &Path) -> io::Result<()> {
        let number = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("tty"))
            .and_then(|raw| raw.parse::<c_int>().ok())
            .filter(|number| *number > 0)
            .ok_or_else(|| io::Error::other("not a virtual terminal path"))?;
        let rc = unsafe { ioctl(tty.as_raw_fd(), VT_ACTIVATE, number) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn framebuffer_virtual_size() -> io::Result<(u32, u32)> {
        for path in ["/dev/fb0", "/dev/fb/0"] {
            if let Ok(file) = File::open(path) {
                let mut raw = [0u32; 40];
                let rc = unsafe { ioctl(file.as_raw_fd(), FBIOGET_VSCREENINFO, raw.as_mut_ptr()) };
                if rc == 0 && raw[2] > 0 && raw[3] > 0 {
                    return Ok((raw[2], raw[3]));
                }
            }
        }
        Err(io::Error::other("framebuffer virtual size unavailable"))
    }

    struct ParsedPsfFont<'a> {
        width: u16,
        height: u16,
        charcount: u16,
        data: &'a [u8],
    }

    fn set_vendored_terminus_font(tty: &File, profile: ConsoleFontProfile) -> io::Result<()> {
        let font = parse_vendored_psf(profile)?;
        let mut op = ConsoleFontOp {
            op: KD_FONT_OP_SET_TALL,
            flags: 0,
            width: u32::from(font.width),
            height: u32::from(font.height),
            charcount: u32::from(font.charcount),
            data: font.data.as_ptr().cast::<c_void>() as *mut c_void,
        };
        let rc = unsafe { ioctl(tty.as_raw_fd(), KDFONTOP, &mut op) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn parse_vendored_psf(profile: ConsoleFontProfile) -> io::Result<ParsedPsfFont<'static>> {
        let bytes = match profile.regular_file {
            "TerminusRegular10x20.psf" => {
                include_bytes!("../../fonts/terminus/TerminusRegular10x20.psf").as_slice()
            }
            "TerminusRegular12x24.psf" => {
                include_bytes!("../../fonts/terminus/TerminusRegular12x24.psf").as_slice()
            }
            _ => return Err(io::Error::other("unknown Terminus profile")),
        };
        parse_psf2(bytes).and_then(|font| {
            if font.width == profile.cell_width && font.height == profile.cell_height {
                Ok(font)
            } else {
                Err(io::Error::other("Terminus profile dimensions mismatch"))
            }
        })
    }

    fn parse_psf2(bytes: &'static [u8]) -> io::Result<ParsedPsfFont<'static>> {
        if bytes.len() < 32 || u32::from_le_bytes(bytes[0..4].try_into().unwrap()) != 0x864a_b572 {
            return Err(io::Error::other("invalid PSF2 magic"));
        }
        let header_size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let length = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
        let charsize = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
        let height = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as u16;
        let width = u32::from_le_bytes(bytes[28..32].try_into().unwrap()) as u16;
        let data_len = length
            .checked_mul(charsize)
            .ok_or_else(|| io::Error::other("PSF glyph data length overflow"))?;
        let end = header_size
            .checked_add(data_len)
            .ok_or_else(|| io::Error::other("PSF glyph data end overflow"))?;
        if bytes.len() < end || length == 0 || length > u16::MAX as usize {
            return Err(io::Error::other("invalid PSF glyph data"));
        }
        Ok(ParsedPsfFont {
            width,
            height,
            charcount: length as u16,
            data: &bytes[header_size..end],
        })
    }
}
