fn centered_root(surface: Surface, width: u16, height: u16) -> Rect {
    let env = surface.envelope();
    let root_width = width.min(env.maximum_columns);
    let root_height = height.min(env.maximum_rows);
    Rect::new(
        (width - root_width) / 2,
        (height - root_height) / 2,
        root_width,
        root_height,
    )
}

fn new_buffer(width: u16, height: u16) -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    fill_rect(
        &mut buffer,
        Rect::new(0, 0, width, height),
        " ",
        SemanticStyle::Canvas,
    );
    buffer
}

fn fill_rect(buffer: &mut Buffer, rect: Rect, symbol: &str, style: SemanticStyle) {
    for y in rect.y..rect.y.saturating_add(rect.height) {
        for x in rect.x..rect.x.saturating_add(rect.width) {
            set_symbol(buffer, x, y, symbol, style);
        }
    }
}

fn draw_line(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: SemanticStyle) {
    if width == 0 {
        return;
    }
    let text = truncate_ascii_preserving_spaces(text, width as usize);
    let mut cx = x;
    for byte in text.bytes() {
        if cx >= x + width {
            break;
        }
        let ch = char::from(byte);
        set_symbol(buffer, cx, y, ch.encode_utf8(&mut [0; 4]), style);
        cx = cx.saturating_add(1);
    }
}

fn draw_styled_line(buffer: &mut Buffer, x: u16, y: u16, width: u16, segments: &[StyledSegment]) {
    if width == 0 {
        return;
    }

    let mut cells = Vec::new();
    for segment in segments {
        let text = truncate_ascii_preserving_spaces(&segment.text, usize::MAX);
        for byte in text.bytes() {
            cells.push((byte, segment.style));
        }
    }

    let max_width = width as usize;
    let visible_len = cells.len();
    let mut out = Vec::new();
    if visible_len <= max_width {
        out.extend(cells);
    } else if max_width <= 3 {
        let ellipsis_style = cells
            .first()
            .map(|(_, style)| *style)
            .unwrap_or(SemanticStyle::Action);
        out.extend((0..max_width).map(|_| (b'.', ellipsis_style)));
    } else {
        let keep = max_width - 3;
        let ellipsis_style = cells
            .get(keep.saturating_sub(1))
            .map(|(_, style)| *style)
            .unwrap_or(SemanticStyle::Action);
        out.extend(cells.into_iter().take(keep));
        out.extend((0..3).map(|_| (b'.', ellipsis_style)));
    }

    let mut cx = x;
    for (byte, style) in out {
        if cx >= x + width {
            break;
        }
        let ch = char::from(byte);
        set_symbol(buffer, cx, y, ch.encode_utf8(&mut [0; 4]), style);
        cx = cx.saturating_add(1);
    }
}

fn set_symbol(buffer: &mut Buffer, x: u16, y: u16, symbol: &str, style: SemanticStyle) {
    let area = buffer.area;
    if x < area.x || y < area.y || x >= area.x + area.width || y >= area.y + area.height {
        return;
    }
    buffer[(x, y)]
        .set_symbol(symbol)
        .set_style(rat_style(style));
}

fn frame_from_buffer(
    buffer: Buffer,
    region_tree: Vec<SemanticRegion>,
    focus_identity: Option<String>,
) -> RenderedFrame {
    let cells_text = buffer_to_text(&buffer);
    let semantic_spans = buffer_to_semantic_spans(&buffer);
    let normalized_cells = buffer_to_normalized_cells(&buffer);
    RenderedFrame {
        cells_text,
        buffer,
        semantic_spans,
        region_tree,
        focus_identity,
        normalized_cells,
    }
}

fn text_frame(
    width: u16,
    height: u16,
    text: &str,
    focus: Option<String>,
    regions: Vec<SemanticRegion>,
) -> RenderedFrame {
    let mut buffer = new_buffer(width, height);
    for (idx, line) in text.lines().enumerate() {
        if idx as u16 >= height {
            break;
        }
        let style = if idx == 0 {
            SemanticStyle::Header
        } else {
            SemanticStyle::Plain
        };
        draw_line(&mut buffer, 0, idx as u16, width, line, style);
    }
    frame_from_buffer(buffer, regions, focus)
}

pub fn buffer_to_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

pub fn buffer_to_fixed_cells(buffer: &Buffer, width: u16, height: u16) -> String {
    let mut out = String::new();
    for y in 0..height {
        for x in 0..width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

pub fn buffer_to_styles(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for span in buffer_to_semantic_spans(buffer) {
        let style = span.semantic_style;
        let style_name = style.name();
        let fg = color_name(span.fg);
        let modifier = modifier_name(span.modifiers);
        out.push_str(&format!(
            "row={} col={} len={} style={} fg={} bg=default modifier={}\n",
            span.y, span.x, span.width, style_name, fg, modifier
        ));
    }
    if area.width == 0 || area.height == 0 {
        out.push_str("empty\n");
    }
    out
}

impl SemanticStyle {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Canvas => "Canvas",
            Self::Plain => "Plain",
            Self::Header => "Header",
            Self::Section => "Section",
            Self::Muted => "Muted",
            Self::Info => "Info",
            Self::Focus => "Focus",
            Self::Action => "Action",
            Self::ActionFocus => "ActionFocus",
            Self::DestructiveFocus => "DestructiveFocus",
            Self::Warning => "Warning",
            Self::Success => "Success",
            Self::Disabled => "Disabled",
            Self::Critical => "Critical",
            Self::GaugeFill => "GaugeFill",
            Self::GaugeSuccess => "GaugeSuccess",
            Self::GaugeWarning => "GaugeWarning",
            Self::GaugeCritical => "GaugeCritical",
            Self::GaugeRest => "GaugeRest",
        }
    }
}

fn color_name(color: Color) -> &'static str {
    match color {
        Color::Reset => "default",
        Color::White => "white",
        Color::LightCyan => "bright-cyan",
        Color::DarkGray => "gray",
        Color::LightRed => "bright-red",
        Color::Yellow => "yellow",
        Color::Green => "green",
        Color::Cyan => "cyan",
        _ => "default",
    }
}

fn modifier_name(modifier: Modifier) -> &'static str {
    if modifier.contains(Modifier::BOLD) {
        "bold"
    } else if modifier.contains(Modifier::DIM) {
        "dim"
    } else {
        "none"
    }
}

fn buffer_to_normalized_cells(buffer: &Buffer) -> Vec<NormalizedCell> {
    let area = buffer.area;
    let mut cells = Vec::new();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            cells.push(NormalizedCell {
                x,
                y,
                symbol: buffer[(x, y)].symbol().to_string(),
                semantic_style: style_from_cell(buffer[(x, y)].style()),
            });
        }
    }
    cells
}

fn buffer_to_semantic_spans(buffer: &Buffer) -> Vec<SemanticSpan> {
    let area = buffer.area;
    let mut spans = Vec::new();
    for y in area.y..area.y + area.height {
        let mut x = area.x;
        while x < area.x + area.width {
            let style = style_from_cell(buffer[(x, y)].style());
            let start = x;
            while x < area.x + area.width && style_from_cell(buffer[(x, y)].style()) == style {
                x += 1;
            }
            let (fg, bg, modifiers) = semantic_style_definition(style);
            spans.push(SemanticSpan {
                x: start,
                y,
                width: x - start,
                semantic_style: style,
                fg,
                bg,
                modifiers,
            });
        }
    }
    spans
}

fn style_from_cell(style: Style) -> SemanticStyle {
    for candidate in [
        SemanticStyle::Canvas,
        SemanticStyle::Plain,
        SemanticStyle::Header,
        SemanticStyle::Section,
        SemanticStyle::Muted,
        SemanticStyle::Info,
        SemanticStyle::Focus,
        SemanticStyle::Action,
        SemanticStyle::ActionFocus,
        SemanticStyle::DestructiveFocus,
        SemanticStyle::Warning,
        SemanticStyle::Success,
        SemanticStyle::Disabled,
        SemanticStyle::Critical,
        SemanticStyle::GaugeFill,
        SemanticStyle::GaugeSuccess,
        SemanticStyle::GaugeWarning,
        SemanticStyle::GaugeCritical,
        SemanticStyle::GaugeRest,
    ] {
        if style.underline_color == Some(semantic_style_marker(candidate)) {
            return candidate;
        }
    }
    for candidate in [
        SemanticStyle::Canvas,
        SemanticStyle::Plain,
        SemanticStyle::Header,
        SemanticStyle::Section,
        SemanticStyle::Muted,
        SemanticStyle::Info,
        SemanticStyle::Focus,
        SemanticStyle::Action,
        SemanticStyle::ActionFocus,
        SemanticStyle::DestructiveFocus,
        SemanticStyle::Warning,
        SemanticStyle::Success,
        SemanticStyle::Disabled,
        SemanticStyle::Critical,
        SemanticStyle::GaugeFill,
        SemanticStyle::GaugeSuccess,
        SemanticStyle::GaugeWarning,
        SemanticStyle::GaugeCritical,
        SemanticStyle::GaugeRest,
    ] {
        let (fg, bg, modifiers) = semantic_style_definition(candidate);
        if style.fg == Some(fg) && style.bg == Some(bg) && style.add_modifier == modifiers {
            return candidate;
        }
    }
    SemanticStyle::Plain
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsciiCellString(String);

impl AsciiCellString {
    pub fn new(input: &str) -> Self {
        let mut out = String::new();
        let mut previous_space = false;
        for ch in input.chars() {
            let next = if ch.is_ascii() && !ch.is_ascii_control() && ch != '\x7f' {
                ch
            } else if ch == '\t' || ch == '\r' || ch == '\n' || ch == '\x1b' || ch.is_control() {
                ' '
            } else {
                '?'
            };
            if next == ' ' {
                if !previous_space {
                    out.push(' ');
                }
                previous_space = true;
            } else {
                out.push(next);
                previous_space = false;
            }
        }
        Self(out.trim().to_string())
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

pub fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let text = AsciiCellString::new(text).into_string();
    if text.len() <= max_width {
        return text;
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }
    let mut out = text.as_bytes()[..max_width - 3]
        .iter()
        .map(|b| char::from(*b))
        .collect::<String>();
    out.push_str("...");
    out
}

fn truncate_ascii_preserving_spaces(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let mut sanitized = String::new();
    for ch in text.chars() {
        if ch.is_ascii() && !ch.is_ascii_control() && ch != '\x7f' {
            sanitized.push(ch);
        } else if ch == '\t' || ch == '\r' || ch == '\n' || ch == '\x1b' || ch.is_control() {
            sanitized.push(' ');
        } else {
            sanitized.push('?');
        }
    }
    if sanitized.len() <= max_width {
        return sanitized;
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }
    let mut out = sanitized.as_bytes()[..max_width - 3]
        .iter()
        .map(|b| char::from(*b))
        .collect::<String>();
    out.push_str("...");
    out
}

fn gauge(label: &str, percent: f64, region_width: usize) -> String {
    let label_width = label.len();
    let width = region_width
        .saturating_sub(label_width)
        .saturating_sub(4)
        .clamp(16, 48);
    ascii_gauge_bar(percent, width)
}

fn ascii_gauge_bar(percent: f64, width: usize) -> String {
    let percent = percent.clamp(0.0, 100.0);
    let filled = ((width as f64 * percent / 100.0).floor() as usize).min(width);
    format!("[{}{}]", "#".repeat(filled), "-".repeat(width - filled))
}

fn percent_number(text: &str) -> f64 {
    text.trim_end_matches('%').parse::<f64>().unwrap_or(0.0)
}

fn payload_footprint_percent(payload_bytes: u64, target_bytes: u64) -> f64 {
    if target_bytes == 0 {
        0.0
    } else {
        (payload_bytes as f64 * 100.0 / target_bytes as f64).clamp(0.0, 100.0)
    }
}

fn percent_ratio(current: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (current as f64 * 100.0 / total as f64).clamp(0.0, 100.0)
    }
}

fn format_decimal_bytes(bytes: u64) -> String {
    bytes.to_string()
}

fn prepare_device_state(state: &str) -> String {
    match state {
        "not-started" => "starting".to_string(),
        "loaded" | "ready" => "ready".to_string(),
        state if state.starts_with("loaded ") => "ready".to_string(),
        "loading" => "loading".to_string(),
        "failed" => "failed".to_string(),
        state if state.starts_with("loading ") => "loading".to_string(),
        _ => "waiting".to_string(),
    }
}

fn prepare_driver_detail(modules_state: &str, drivers_state: &str) -> String {
    let count = modules_state
        .strip_prefix("loading ")
        .or_else(|| modules_state.strip_prefix("loaded "))
        .and_then(parse_driver_count)
        .unwrap_or("0 / 0");
    match drivers_state {
        "done" | "active" => count.to_string(),
        "failed" => format!("{count} failed"),
        _ => count.to_string(),
    }
}

fn parse_driver_count(text: &str) -> Option<&str> {
    let (loaded, total) = text.split_once(" / ")?;
    if loaded.chars().all(|ch| ch.is_ascii_digit()) && total.chars().all(|ch| ch.is_ascii_digit()) {
        Some(text)
    } else {
        None
    }
}

fn prepare_media_state(state: &str) -> &'static str {
    match state {
        "scanning" | "searching" => "searching",
        "checking" | "validating" => "checking",
        "media-found" | "found" => "found",
        "not-found" => "not found",
        "ambiguous" => "ambiguous",
        _ => "pending",
    }
}

fn prepare_payload_state(state: &str) -> &'static str {
    match state {
        "validating" => "validating",
        "valid" => "valid",
        "invalid" => "invalid",
        _ => "",
    }
}

fn prepare_targets_state(state: &str) -> &'static str {
    match state {
        "inspecting" => "inspecting",
        "ready" => "ready",
        "failed" => "failed",
        _ => "",
    }
}

fn prepare_step_text(
    device_state: &str,
    media_state: &str,
    payload_text: &str,
    targets_state: &str,
) -> &'static str {
    if device_state == "starting" || device_state == "waiting" {
        "starting input and storage drivers"
    } else if device_state == "loading" {
        "loading input and storage support"
    } else if media_state == "searching" {
        "looking for installer media"
    } else if media_state == "checking" || media_state == "found" && payload_text == "validating" {
        "checking installer image"
    } else if targets_state == "inspecting" {
        "inspecting target disks"
    } else if payload_text.starts_with("image ") {
        "ready for target selection"
    } else {
        "starting input and storage drivers"
    }
}

fn other_disk_summary(model: &TargetModel) -> String {
    format!(
        "{} installer media - {} installed target - {} blocked by installed target - {} too small - {} unsupported sector - {} no stable id - {} unreadable",
        model.installer_media_count,
        model.installed_target_count,
        model.blocked_by_installed_target_count,
        model.too_small_count,
        model.unsupported_sector_size_count,
        model.no_stable_id_count,
        model.read_error_count
    )
}

fn existing_label(existing: ExistingPartitionTable) -> &'static str {
    match existing {
        ExistingPartitionTable::Gpt => "GPT",
        ExistingPartitionTable::Mbr => "MBR",
        ExistingPartitionTable::None => "none",
        ExistingPartitionTable::Unrecognized => "unrecognized",
    }
}

fn write_task_index(task: WriteTask) -> usize {
    match task {
        WriteTask::VerifyTarget => 0,
        WriteTask::PrepareDiskBeginning => 1,
        WriteTask::PrepareDiskEnd => 2,
        WriteTask::CopyYaoshiImage => 3,
        WriteTask::FinalizeWrites => 4,
    }
}

pub fn candidate_counts(
    candidates: &[TargetDiskCandidate],
) -> (usize, usize, usize, usize, usize, usize, usize, usize, usize) {
    let mut selectable = 0;
    let mut installer_media = 0;
    let mut installed_target = 0;
    let mut blocked_by_installed_target = 0;
    let mut too_small = 0;
    let mut unsupported = 0;
    let mut no_stable = 0;
    let mut read_error = 0;
    for candidate in candidates {
        match candidate.status {
            CandidateStatus::Selectable => selectable += 1,
            CandidateStatus::InstallerMedia => installer_media += 1,
            CandidateStatus::InstalledTarget => installed_target += 1,
            CandidateStatus::BlockedByInstalledTarget => blocked_by_installed_target += 1,
            CandidateStatus::TooSmall => too_small += 1,
            CandidateStatus::UnsupportedSectorSize => unsupported += 1,
            CandidateStatus::NoStableId => no_stable += 1,
            CandidateStatus::ReadError => read_error += 1,
        }
    }
    (
        candidates.len(),
        selectable,
        installer_media,
        installed_target,
        blocked_by_installed_target,
        too_small,
        unsupported,
        no_stable,
        read_error,
    )
}
