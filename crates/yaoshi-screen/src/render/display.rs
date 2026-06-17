#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Installer,
    Dashboard,
}

impl Surface {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Installer => "installer",
            Self::Dashboard => "dashboard",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Installer => "Yaoshi Installer",
            Self::Dashboard => "Yaoshi Dashboard",
        }
    }

    pub const fn envelope(self) -> DisplayEnvelope {
        match self {
            Self::Installer => DisplayEnvelope {
                minimum_columns: 96,
                minimum_rows: 28,
                maximum_columns: 160,
                maximum_rows: 44,
            },
            Self::Dashboard => DisplayEnvelope {
                minimum_columns: 80,
                minimum_rows: 24,
                maximum_columns: 160,
                maximum_rows: 44,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayEnvelope {
    pub minimum_columns: u16,
    pub minimum_rows: u16,
    pub maximum_columns: u16,
    pub maximum_rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeClass {
    Unsupported,
    Supported,
}

pub type TerminalSizeClass = SizeClass;

pub fn size_class_for_surface(surface: Surface, width: u16, height: u16) -> SizeClass {
    let env = surface.envelope();
    if width >= env.minimum_columns && height >= env.minimum_rows {
        SizeClass::Supported
    } else {
        SizeClass::Unsupported
    }
}

pub fn size_class(width: u16, height: u16) -> SizeClass {
    size_class_for_surface(Surface::Dashboard, width, height)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsoleFontProfile {
    pub name: &'static str,
    pub cell_width: u16,
    pub cell_height: u16,
    pub regular_file: &'static str,
    pub bold_file: &'static str,
}

impl ConsoleFontProfile {
    pub const fn new(
        name: &'static str,
        cell_width: u16,
        cell_height: u16,
        regular_file: &'static str,
        bold_file: &'static str,
    ) -> Self {
        Self {
            name,
            cell_width,
            cell_height,
            regular_file,
            bold_file,
        }
    }

    pub const fn is_current_console(self) -> bool {
        self.regular_file.is_empty() && self.bold_file.is_empty()
    }
}

pub const CONSOLE_FONT_PROFILES: [ConsoleFontProfile; 2] = [
    ConsoleFontProfile::new(
        "Terminus10x20",
        10,
        20,
        "TerminusRegular10x20.psf",
        "TerminusBold10x20.psf",
    ),
    ConsoleFontProfile::new(
        "Terminus12x24",
        12,
        24,
        "TerminusRegular12x24.psf",
        "TerminusBold12x24.psf",
    ),
];

pub const CURRENT_CONSOLE_FONT_PROFILE: ConsoleFontProfile =
    ConsoleFontProfile::new("CurrentConsole", 8, 16, "", "");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiatedDisplay {
    pub surface: Surface,
    pub font_profile: String,
    pub columns: u16,
    pub rows: u16,
    pub framebuffer_width: u32,
    pub framebuffer_height: u32,
    pub cell_width: u16,
    pub cell_height: u16,
    pub root_columns: u16,
    pub root_rows: u16,
}

impl NegotiatedDisplay {
    pub fn new(
        surface: Surface,
        profile: ConsoleFontProfile,
        columns: u16,
        rows: u16,
        framebuffer_width: u32,
        framebuffer_height: u32,
    ) -> Self {
        let env = surface.envelope();
        let cell_width = if columns == 0 {
            0
        } else {
            (framebuffer_width / u32::from(columns)) as u16
        };
        let cell_height = if rows == 0 {
            0
        } else {
            (framebuffer_height / u32::from(rows)) as u16
        };
        Self {
            surface,
            font_profile: profile.name.to_string(),
            columns,
            rows,
            framebuffer_width,
            framebuffer_height,
            cell_width,
            cell_height,
            root_columns: columns.min(env.maximum_columns),
            root_rows: rows.min(env.maximum_rows),
        }
    }

    pub fn fixture(surface: Surface, columns: u16, rows: u16) -> Self {
        let profile = CONSOLE_FONT_PROFILES[0];
        Self::new(
            surface,
            profile,
            columns,
            rows,
            u32::from(columns) * u32::from(profile.cell_width),
            u32::from(rows) * u32::from(profile.cell_height),
        )
    }

    pub fn serial_marker(&self) -> String {
        format!(
            "YAOSHI_MARK v=1 kind=display surface={} profile={} fb={}x{} cell={}x{} grid={}x{} root={}x{}",
            self.surface.as_str(),
            self.font_profile,
            self.framebuffer_width,
            self.framebuffer_height,
            self.cell_width,
            self.cell_height,
            self.columns,
            self.rows,
            self.root_columns,
            self.root_rows
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayNegotiationReason {
    ConsoleFontUnavailable,
    LogicalGridTooSmall,
    TerminalSizeUnreadable,
    FontApplicationNotEffective,
    TerminalRendererUnavailable,
}

impl DisplayNegotiationReason {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ConsoleFontUnavailable => "console-font-unavailable",
            Self::LogicalGridTooSmall => "logical-grid-too-small",
            Self::TerminalSizeUnreadable => "terminal-size-unreadable",
            Self::FontApplicationNotEffective => "font-application-not-effective",
            Self::TerminalRendererUnavailable => "terminal-renderer-unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayNegotiationFailure {
    pub surface: Surface,
    pub columns: u16,
    pub rows: u16,
    pub font_profile: Option<String>,
    pub reason: DisplayNegotiationReason,
    pub attempts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayNegotiation {
    Supported(NegotiatedDisplay),
    Unsupported(DisplayNegotiationFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayObservation {
    pub columns: u16,
    pub rows: u16,
    pub framebuffer_width: Option<u32>,
    pub framebuffer_height: Option<u32>,
}

impl DisplayObservation {
    pub const fn new(
        columns: u16,
        rows: u16,
        framebuffer_width: Option<u32>,
        framebuffer_height: Option<u32>,
    ) -> Self {
        Self {
            columns,
            rows,
            framebuffer_width,
            framebuffer_height,
        }
    }
}

fn effective_pixel_size(observed: DisplayObservation, profile: ConsoleFontProfile) -> (u32, u32) {
    let width = observed
        .framebuffer_width
        .unwrap_or_else(|| u32::from(observed.columns) * u32::from(profile.cell_width));
    let height = observed
        .framebuffer_height
        .unwrap_or_else(|| u32::from(observed.rows) * u32::from(profile.cell_height));
    (width, height)
}

pub fn negotiate_display_for_profile(
    surface: Surface,
    profile: ConsoleFontProfile,
    observed: DisplayObservation,
) -> DisplayNegotiation {
    let (fb_width, fb_height) = effective_pixel_size(observed, profile);
    if observed.columns == 0 || observed.rows == 0 {
        return profile_failure(
            surface,
            profile,
            observed,
            DisplayNegotiationReason::TerminalSizeUnreadable,
            format!("{} terminal grid unreadable", profile.name),
        );
    }
    if !profile.is_current_console() {
        let observed_cell_width = (fb_width / u32::from(observed.columns)) as u16;
        let observed_cell_height = (fb_height / u32::from(observed.rows)) as u16;
        if observed_cell_width < profile.cell_width || observed_cell_height < profile.cell_height {
            return profile_failure(
                surface,
                profile,
                observed,
                DisplayNegotiationReason::FontApplicationNotEffective,
                format!(
                    "{} ineffective cell {}x{}",
                    profile.name, observed_cell_width, observed_cell_height
                ),
            );
        }
    }
    let env = surface.envelope();
    if observed.columns >= env.minimum_columns && observed.rows >= env.minimum_rows {
        DisplayNegotiation::Supported(NegotiatedDisplay::new(
            surface,
            profile,
            observed.columns,
            observed.rows,
            fb_width,
            fb_height,
        ))
    } else {
        profile_failure(
            surface,
            profile,
            observed,
            DisplayNegotiationReason::LogicalGridTooSmall,
            format!(
                "{} observed {}x{}",
                profile.name, observed.columns, observed.rows
            ),
        )
    }
}

fn profile_failure(
    surface: Surface,
    profile: ConsoleFontProfile,
    observed: DisplayObservation,
    reason: DisplayNegotiationReason,
    attempt: String,
) -> DisplayNegotiation {
    DisplayNegotiation::Unsupported(DisplayNegotiationFailure {
        surface,
        columns: observed.columns,
        rows: observed.rows,
        font_profile: Some(profile.name.to_string()),
        reason,
        attempts: vec![attempt],
    })
}

pub fn negotiate_display_from_observations(
    surface: Surface,
    observations: &[(ConsoleFontProfile, Result<DisplayObservation, &'static str>)],
) -> DisplayNegotiation {
    let mut attempts = Vec::new();
    let mut last_grid = (0, 0);
    let mut last_profile = None;
    let mut last_reason = DisplayNegotiationReason::LogicalGridTooSmall;
    let framebuffer_height = observations
        .iter()
        .filter_map(|(_, observed)| observed.as_ref().ok()?.framebuffer_height)
        .next()
        .unwrap_or(0);
    for profile in candidate_profiles_for_framebuffer_height(framebuffer_height) {
        let Some((_, observed)) = observations
            .iter()
            .find(|(candidate, _)| candidate.name == profile.name)
        else {
            last_profile = Some(profile.name.to_string());
            last_reason = DisplayNegotiationReason::ConsoleFontUnavailable;
            attempts.push(format!("{} missing observation", profile.name));
            continue;
        };
        last_profile = Some(profile.name.to_string());
        let observed = match observed {
            Ok(observed) => *observed,
            Err(reason) => {
                attempts.push(format!("{} failed {reason}", profile.name));
                last_reason = DisplayNegotiationReason::ConsoleFontUnavailable;
                continue;
            }
        };
        last_grid = (observed.columns, observed.rows);
        match negotiate_display_for_profile(surface, profile, observed) {
            DisplayNegotiation::Supported(display) => {
                attempts.push(format!(
                    "{} eligible {}x{}",
                    profile.name, observed.columns, observed.rows
                ));
                return DisplayNegotiation::Supported(display);
            }
            DisplayNegotiation::Unsupported(failure) => {
                last_reason = failure.reason;
                attempts.extend(failure.attempts);
            }
        }
    }
    DisplayNegotiation::Unsupported(DisplayNegotiationFailure {
        surface,
        columns: last_grid.0,
        rows: last_grid.1,
        font_profile: last_profile,
        reason: last_reason,
        attempts,
    })
}

pub fn negotiate_display_from_current_console(
    surface: Surface,
    observed: DisplayObservation,
) -> DisplayNegotiation {
    negotiate_display_for_profile(surface, CURRENT_CONSOLE_FONT_PROFILE, observed)
}

pub fn candidate_profiles_for_framebuffer_height(
    framebuffer_height_pixels: u32,
) -> [ConsoleFontProfile; 2] {
    if framebuffer_height_pixels >= 1440 {
        [CONSOLE_FONT_PROFILES[1], CONSOLE_FONT_PROFILES[0]]
    } else {
        [CONSOLE_FONT_PROFILES[0], CONSOLE_FONT_PROFILES[1]]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    ArrowUp,
    ArrowDown,
    Tab,
    Enter,
    Resize { columns: u16, rows: u16 },
}

pub fn normalize_installer_event(event: Event) -> Option<Input> {
    match event {
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
            match key.code {
                KeyCode::Up => Some(Input::ArrowUp),
                KeyCode::Down => Some(Input::ArrowDown),
                KeyCode::Tab => Some(Input::Tab),
                KeyCode::Enter => Some(Input::Enter),
                _ => None,
            }
        }
        Event::Resize(columns, rows) => Some(Input::Resize { columns, rows }),
        _ => None,
    }
}

pub fn normalize_dashboard_event(event: Event) -> Option<Input> {
    match event {
        Event::Resize(columns, rows) => Some(Input::Resize { columns, rows }),
        _ => None,
    }
}
