pub fn render_dashboard_scene(
    snapshot: &DashboardSnapshot,
    width: u16,
    height: u16,
) -> RenderedFrame {
    if size_class_for_surface(Surface::Dashboard, width, height) == SizeClass::Unsupported {
        let failure = DisplayNegotiationFailure {
            surface: Surface::Dashboard,
            columns: width,
            rows: height,
            font_profile: None,
            reason: DisplayNegotiationReason::LogicalGridTooSmall,
            attempts: Vec::new(),
        };
        return text_frame(
            width,
            height,
            &unsupported_display_page(&failure),
            None,
            vec![],
        );
    }
    let root = centered_root(Surface::Dashboard, width, height);
    let mut buffer = new_buffer(width, height);
    let regions = dashboard_regions(root, snapshot);
    draw_line(
        &mut buffer,
        root.x,
        root.y,
        root.width,
        &format!(
            "Yaoshi Dashboard v{VERSION}  {}  up {}",
            snapshot.hostname, snapshot.uptime
        ),
        SemanticStyle::Header,
    );
    draw_line(
        &mut buffer,
        root.x,
        root.y + 1,
        root.width,
        &format!(
            "Prepare {}  Root {}  SSH {}  Net {}  Health {}{}",
            snapshot.prepare_state,
            snapshot.root_expansion_state,
            snapshot.ssh_ready_state,
            snapshot.network_state,
            health_state(snapshot),
            health_reason_suffix(snapshot)
        ),
        SemanticStyle::Plain,
    );
    for region in &regions {
        match region.name {
            "System" => draw_section(&mut buffer, region.rect, "System", system_rows(snapshot)),
            "Compute" => draw_section(
                &mut buffer,
                region.rect,
                "Compute",
                compute_rows(snapshot, region.rect.width, region.rect.height),
            ),
            "Memory" => draw_section(
                &mut buffer,
                region.rect,
                "Memory",
                memory_rows(snapshot, region.rect.width),
            ),
            "Storage" => draw_section(
                &mut buffer,
                region.rect,
                "Storage",
                storage_rows(snapshot, region.rect.width, region.rect.height),
            ),
            "Network" => draw_section(
                &mut buffer,
                region.rect,
                "Network",
                network_rows(snapshot, region.rect.height),
            ),
            _ => {}
        }
    }
    frame_from_buffer(buffer, regions, None)
}

fn dashboard_regions(root: Rect, snapshot: &DashboardSnapshot) -> Vec<SemanticRegion> {
    let body = Rect::new(
        root.x,
        root.y + 3,
        root.width,
        root.height.saturating_sub(3),
    );
    let extra_height = body.height.saturating_sub(21);
    let storage_detail_unit_height = 2u16;
    let network_detail_unit_height = 2u16;
    let storage_extra_demand =
        storage_detail_candidates(snapshot).len().min(6) as u16 * storage_detail_unit_height;
    let network_extra_demand =
        network_detail_candidates(snapshot).len().min(6) as u16 * network_detail_unit_height;
    let compute_extra_demand = compute_detail_candidates(snapshot).len().min(6) as u16;
    let (storage_extra, network_extra, compute_extra) = allocate_dashboard_extra_rows(
        extra_height,
        storage_extra_demand,
        network_extra_demand,
        compute_extra_demand,
    );
    let system_height = 5u16.min(body.height);
    let compute_height = 3u16
        .saturating_add(compute_extra)
        .min(body.height.saturating_sub(system_height));
    let memory_height = 4u16.min(body.height.saturating_sub(system_height + compute_height));
    let storage_height = 4u16.saturating_add(storage_extra).min(
        body.height
            .saturating_sub(system_height + compute_height + memory_height),
    );
    let network_height = 5u16.saturating_add(network_extra).min(
        body.height
            .saturating_sub(system_height + compute_height + memory_height + storage_height),
    );
    let _body_tail_canvas_height = body.height.saturating_sub(
        system_height + compute_height + memory_height + storage_height + network_height,
    );
    let compute_y = body.y + system_height;
    let memory_y = compute_y + compute_height;
    let storage_y = memory_y + memory_height;
    let network_y = storage_y + storage_height;
    vec![
        SemanticRegion {
            name: "HeaderLine",
            rect: Rect::new(root.x, root.y, root.width, 1),
        },
        SemanticRegion {
            name: "StatusLine",
            rect: Rect::new(root.x, root.y + 1, root.width, 1),
        },
        SemanticRegion {
            name: "ChromeGap",
            rect: Rect::new(root.x, root.y + 2, root.width, 1),
        },
        SemanticRegion {
            name: "System",
            rect: Rect::new(body.x, body.y, body.width, system_height),
        },
        SemanticRegion {
            name: "Compute",
            rect: Rect::new(body.x, compute_y, body.width, compute_height),
        },
        SemanticRegion {
            name: "Memory",
            rect: Rect::new(body.x, memory_y, body.width, memory_height),
        },
        SemanticRegion {
            name: "Storage",
            rect: Rect::new(body.x, storage_y, body.width, storage_height),
        },
        SemanticRegion {
            name: "Network",
            rect: Rect::new(body.x, network_y, body.width, network_height),
        },
    ]
}

fn allocate_dashboard_extra_rows(
    extra_height: u16,
    storage_extra_demand: u16,
    network_extra_demand: u16,
    compute_extra_demand: u16,
) -> (u16, u16, u16) {
    let mut storage_extra = 0;
    let mut network_extra = 0;
    let mut compute_extra = 0;
    let mut remaining_extra = extra_height;
    while remaining_extra > 0
        && (storage_extra < storage_extra_demand
            || network_extra < network_extra_demand
            || compute_extra < compute_extra_demand)
    {
        if remaining_extra >= 2 && storage_extra < storage_extra_demand {
            storage_extra += 2;
            remaining_extra -= 2;
        }
        if remaining_extra >= 2 && network_extra < network_extra_demand {
            network_extra += 2;
            remaining_extra -= 2;
        }
        if remaining_extra > 0 && compute_extra < compute_extra_demand {
            compute_extra += 1;
            remaining_extra -= 1;
        }
    }
    (storage_extra, network_extra, compute_extra)
}

fn system_rows(s: &DashboardSnapshot) -> Vec<(String, SemanticStyle)> {
    vec![
        (
            format!(
                "Boot      kernel {} - prepare {} - growth {}",
                s.kernel_release, s.prepare_state, s.root_expansion_state
            ),
            SemanticStyle::Plain,
        ),
        (
            format!(
                "Access    ssh root@{}:22 {} - tty2 {}",
                s.ssh_access_address, s.ssh_ready_state, s.root_shell_state
            ),
            SemanticStyle::Plain,
        ),
        (machine_identity_row(s), SemanticStyle::Plain),
        signals_row(s),
    ]
}

fn machine_identity_row(s: &DashboardSnapshot) -> String {
    let system_vendor = dmi_real_value(&s.system_vendor);
    let product_name = dmi_real_value(&s.product_name);
    let board_vendor = dmi_real_value(&s.board_vendor);
    let board_name = dmi_real_value(&s.board_name);
    let firmware_vendor = dmi_real_value(&s.firmware_vendor).unwrap_or("unavailable");
    let firmware_version = dmi_real_value(&s.firmware_version).unwrap_or("unavailable");

    let machine_display = match (system_vendor, product_name, board_name) {
        (Some(vendor), Some(product), _) => join_identity(vendor, product),
        (Some(vendor), None, Some(board)) => join_identity(vendor, board),
        (None, Some(product), _) => product.to_string(),
        (None, None, Some(board)) => board.to_string(),
        _ => "unavailable".to_string(),
    };

    let product_is_placeholder = dmi_real_value(&s.product_name).is_none();
    let board_segment = if product_is_placeholder && board_name.is_some() {
        String::new()
    } else {
        let board = match (board_vendor, board_name) {
            (Some(vendor), Some(board)) => join_identity(vendor, board),
            (Some(vendor), None) => vendor.to_string(),
            (None, Some(board)) => board.to_string(),
            _ => "unavailable".to_string(),
        };
        format!(" - board {board}")
    };

    format!("Machine   {machine_display}{board_segment} - fw {firmware_vendor} {firmware_version}")
}

fn dmi_real_value(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("unavailable") || is_dmi_placeholder(value) {
        None
    } else {
        Some(value)
    }
}

fn is_dmi_placeholder(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "system product name"
            | "system manufacturer"
            | "system version"
            | "base board product name"
            | "base board manufacturer"
            | "to be filled by o.e.m."
            | "to be filled by oem"
            | "default string"
            | "not applicable"
            | "none"
            | "unknown"
    )
}

fn join_identity(prefix: &str, value: &str) -> String {
    if value.starts_with(prefix) {
        value.to_string()
    } else {
        format!("{prefix} {value}")
    }
}

fn signals_row(s: &DashboardSnapshot) -> (String, SemanticStyle) {
    let style = state_style(if s.thermal_state == "unavailable" {
        &s.kernel_alert_state
    } else {
        &s.thermal_state
    });
    if s.thermal == "unavailable" && s.thermal_source == "unavailable" {
        (
            format!(
                "Signals   thermal unavailable - kernel {}",
                s.kernel_alert_state
            ),
            style,
        )
    } else {
        let thermal_alert = alert_suffix(&s.thermal_state);
        let alert = if thermal_alert.is_empty() {
            String::new()
        } else {
            format!(" {thermal_alert}")
        };
        (
            format!(
                "Signals   thermal {} {}{} - kernel {}",
                s.thermal, s.thermal_source, alert, s.kernel_alert_state
            ),
            style,
        )
    }
}

fn compute_rows(s: &DashboardSnapshot, width: u16, height: u16) -> Vec<(String, SemanticStyle)> {
    let (max_core_index, max_core_percent) = max_core_summary(s);
    let max_core = max_core_index
        .map(|index| format!("max cpu{} {}", index, max_core_percent.unwrap_or("0.000%")))
        .unwrap_or_else(|| "max unavailable".to_string());
    let cpu_alert = alert_suffix(&s.cpu_state);
    let cpu_alert_text = if cpu_alert.is_empty() {
        String::new()
    } else {
        format!(" {cpu_alert}")
    };
    let mut rows = vec![
        (
            format!(
                "CPU       {} - {} CPUs - load {} {} {}",
                s.cpu_model, s.logical_cpu_count, s.load1, s.load5, s.load15
            ),
            SemanticStyle::Plain,
        ),
        (
            format!(
                "Usage     {} {} - {} - psi {}{}",
                s.cpu_used,
                fitted_gauge(
                    "usage",
                    percent_number(&s.cpu_used),
                    width,
                    "Usage     ".len()
                        + s.cpu_used.len()
                        + max_core.len()
                        + s.cpu_pressure.len()
                        + cpu_alert_text.len()
                        + 16,
                ),
                max_core,
                s.cpu_pressure,
                cpu_alert_text
            ),
            state_style(&s.cpu_state),
        ),
    ];
    let available = height.saturating_sub(3) as usize;
    let candidates = compute_detail_candidates(s);
    let rendered = render_with_overflow(candidates, available, "CPU cores");
    rows.extend(rendered);
    if available == 0 && s.core_rows.len() > 1 {
        let _ = s.core_rows.len();
    }
    rows
}

fn memory_rows(s: &DashboardSnapshot, width: u16) -> Vec<(String, SemanticStyle)> {
    vec![
        (
            compact_capacity_gauge_row(
                "RAM       ",
                &s.memory_used,
                &s.memory_total,
                &s.memory_used_percent,
                &s.memory_state,
                width,
            ),
            state_style(&s.memory_state),
        ),
        (
            compact_capacity_gauge_row(
                "Swap      ",
                &s.swap_used,
                &s.swap_total,
                &s.swap_used_percent,
                metric_state(percent_number(&s.swap_used_percent), 80.0, 90.0),
                width,
            ),
            SemanticStyle::Plain,
        ),
        (
            format!("DIMMs     {}", s.memory_dimm_summary),
            if s.memory_dimm_summary == "unavailable" {
                SemanticStyle::Disabled
            } else {
                SemanticStyle::Plain
            },
        ),
    ]
}

fn storage_rows(s: &DashboardSnapshot, width: u16, height: u16) -> Vec<(String, SemanticStyle)> {
    let inode_alert = alert_suffix(&s.inode_state);
    let inode_text = if inode_alert.is_empty() {
        String::new()
    } else {
        format!(" {}", inode_alert)
    };
    let mut rows = vec![
        (
            format!(
                "{} - inode {}{}",
                compact_capacity_gauge_row_reserved(
                    "Root      ",
                    &s.root_used,
                    &s.root_total,
                    &s.root_used_percent,
                    &s.root_state,
                    width,
                    " - inode ".len() + s.root_inode_percent.len() + inode_text.len(),
                ),
                s.root_inode_percent,
                inode_text
            ),
            state_style(&s.root_state),
        ),
        (
            format!(
                "I/O       r {}/s w {}/s - iops r {}/s w {}/s",
                s.disk_read_rate, s.disk_write_rate, s.disk_read_iops, s.disk_write_iops
            ),
            SemanticStyle::Plain,
        ),
        (disk_summary_row(s), SemanticStyle::Plain),
    ];
    let available = height.saturating_sub(4) as usize;
    rows.extend(render_disk_units(
        storage_detail_candidates(s),
        available,
        width,
    ));
    rows
}

fn network_rows(s: &DashboardSnapshot, height: u16) -> Vec<(String, SemanticStyle)> {
    let network_alert_suffix = alert_suffix(&s.network_error_state);
    let network_alert_text = if network_alert_suffix.is_empty() {
        String::new()
    } else {
        format!(" {network_alert_suffix}")
    };
    let mut rows = vec![
        (route_summary_row(s), SemanticStyle::Plain),
        (
            format!(
                "Traffic   all rx {}/s tx {}/s - pkts rx {}/s tx {}/s",
                s.network_rx_rate,
                s.network_tx_rate,
                s.network_rx_packet_rate,
                s.network_tx_packet_rate
            ),
            SemanticStyle::Plain,
        ),
        (
            format!(
                "Errors    all err rx {}/s tx {}/s - drop rx {}/s tx {}/s{}",
                s.network_rx_error_rate,
                s.network_tx_error_rate,
                s.network_rx_drop_rate,
                s.network_tx_drop_rate,
                network_alert_text
            ),
            state_style(&s.network_error_state),
        ),
        (interface_summary_row(s), SemanticStyle::Plain),
    ];
    let available = height.saturating_sub(5) as usize;
    rows.extend(render_interface_units(
        network_detail_candidates(s),
        available,
    ));
    rows
}

fn draw_section(buffer: &mut Buffer, rect: Rect, title: &str, rows: Vec<(String, SemanticStyle)>) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    draw_line(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        title,
        SemanticStyle::Section,
    );
    for (idx, (row, style)) in rows
        .into_iter()
        .take(rect.height.saturating_sub(1) as usize)
        .enumerate()
    {
        let y = rect.y + 1 + idx as u16;
        if title == "Network" && matches!(style, SemanticStyle::Warning | SemanticStyle::Critical) {
            draw_line(buffer, rect.x, y, rect.width, &row, SemanticStyle::Plain);
            draw_network_alert_spans(buffer, rect.x, y, rect.width, &row, style);
        } else {
            draw_line(buffer, rect.x, y, rect.width, &row, style);
        }
    }
}

fn draw_network_alert_spans(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    row: &str,
    style: SemanticStyle,
) {
    draw_network_rate_after(buffer, x, y, width, row, "err rx ", style);
    if let Some(err_start) = row.find("err rx ")
        && let Some(drop_start) = row.find(" - drop ")
    {
        draw_network_rate_after(buffer, x, y, width, &row[..drop_start], " tx ", style);
        let _ = err_start;
    }
    draw_network_rate_after(buffer, x, y, width, row, "drop rx ", style);
    if let Some(drop_start) = row.find("drop rx ") {
        let tail = &row[drop_start..];
        if let Some(relative_tx) = tail.find(" tx ") {
            let marker_start = drop_start + relative_tx;
            draw_network_rate_at(buffer, x, y, width, row, marker_start + " tx ".len(), style);
        }
    }
    for suffix in [" warning", " critical", " unavailable"] {
        if let Some(start) = row.rfind(suffix) {
            let suffix_style = if suffix == " unavailable" {
                SemanticStyle::Disabled
            } else {
                style
            };
            draw_line(
                buffer,
                x + start as u16 + 1,
                y,
                width.saturating_sub(start as u16 + 1),
                suffix.trim(),
                suffix_style,
            );
        }
    }
}

fn draw_network_rate_after(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    row: &str,
    marker: &str,
    style: SemanticStyle,
) {
    if let Some(start) = row.find(marker).map(|idx| idx + marker.len()) {
        draw_network_rate_at(buffer, x, y, width, row, start, style);
    }
}

fn draw_network_rate_at(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    row: &str,
    start: usize,
    style: SemanticStyle,
) {
    if let Some(len) = network_rate_value_len(&row[start..])
        && network_rate_is_alerting(&row[start..start + len])
    {
        draw_line(
            buffer,
            x + start as u16,
            y,
            width.saturating_sub(start as u16),
            &row[start..start + len],
            style,
        );
    }
}

fn network_rate_value_len(text: &str) -> Option<usize> {
    let slash = text.find("/s")?;
    Some(slash + 2)
}

fn network_rate_is_alerting(text: &str) -> bool {
    text.split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .is_some_and(|value| value > 0.0)
}

fn state_style(state: &str) -> SemanticStyle {
    match state {
        "critical" | "failed" | "error" => SemanticStyle::Critical,
        "warning" | "unknown" => SemanticStyle::Warning,
        "ready" | "expanded" | "enabled" | "running" | "none" | "ok" => SemanticStyle::Success,
        "unavailable" | "disabled" => SemanticStyle::Disabled,
        _ => SemanticStyle::Plain,
    }
}

fn health_state(s: &DashboardSnapshot) -> &'static str {
    if s.prepare_state == "failed"
        || s.root_expansion_state == "failed"
        || matches!(s.ssh_ready_state.as_str(), "disabled" | "unavailable")
        || s.kernel_alert_state == "error"
        || s.network_error_state == "critical"
        || [
            &s.cpu_state,
            &s.memory_state,
            &s.root_state,
            &s.inode_state,
            &s.thermal_state,
        ]
        .iter()
        .any(|state| state.as_str() == "critical")
    {
        "critical"
    } else if matches!(s.prepare_state.as_str(), "running" | "pending" | "unknown")
        || s.root_expansion_state == "unknown"
        || s.ssh_ready_state == "starting"
        || s.kernel_alert_state == "warning"
        || [
            &s.cpu_state,
            &s.memory_state,
            &s.root_state,
            &s.inode_state,
            &s.thermal_state,
        ]
        .iter()
        .any(|state| state.as_str() == "warning")
    {
        "warning"
    } else if s.prepare_state != "applied"
        || [&s.cpu_state, &s.memory_state, &s.root_state, &s.inode_state]
            .iter()
            .any(|state| state.as_str() == "unavailable")
    {
        "unavailable"
    } else {
        "ok"
    }
}

fn health_reason_suffix(s: &DashboardSnapshot) -> String {
    if health_state(s) == "ok" {
        return String::new();
    }
    let reason = if s.prepare_state != "applied" {
        "prepare"
    } else if s.root_expansion_state == "failed" || s.root_expansion_state == "unknown" {
        "root"
    } else if matches!(
        s.ssh_ready_state.as_str(),
        "starting" | "disabled" | "unavailable"
    ) {
        "ssh"
    } else if s.kernel_alert_state == "error" || s.kernel_alert_state == "warning" {
        "kernel"
    } else if s.network_error_state == "critical" || s.network_error_state == "warning" {
        "net"
    } else if s.cpu_state == "critical" || s.cpu_state == "warning" {
        "cpu"
    } else if s.memory_state == "critical" || s.memory_state == "warning" {
        "ram"
    } else if s.root_state == "critical" || s.root_state == "warning" {
        "disk"
    } else if s.inode_state == "critical" || s.inode_state == "warning" {
        "inode"
    } else if s.thermal_state == "critical" || s.thermal_state == "warning" {
        "thermal"
    } else {
        "source"
    };
    format!(": {reason}")
}

fn should_render_top_process(s: &DashboardSnapshot) -> bool {
    percent_number(&s.cpu_used) >= 85.0
        || top_process_cpu_percent(&s.top_cpu_process).is_some_and(|value| value >= 25.0)
}

fn max_core_summary(s: &DashboardSnapshot) -> (Option<usize>, Option<&str>) {
    s.core_rows
        .iter()
        .max_by(|a, b| {
            percent_number(&a.used_percent)
                .partial_cmp(&percent_number(&b.used_percent))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.index.cmp(&a.index))
        })
        .map(|row| (Some(row.index), Some(row.used_percent.as_str())))
        .unwrap_or((None, None))
}

fn compute_detail_candidates(s: &DashboardSnapshot) -> Vec<(String, SemanticStyle)> {
    let mut rows = Vec::new();
    if should_render_top_process(s) {
        rows.push((
            format!("Top proc  {}", s.top_cpu_process),
            SemanticStyle::Plain,
        ));
    }
    let mut cores = s.core_rows.clone();
    cores.sort_by(|a, b| {
        core_alert_rank(a)
            .cmp(&core_alert_rank(b))
            .then_with(|| {
                percent_number(&b.used_percent)
                    .partial_cmp(&percent_number(&a.used_percent))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.index.cmp(&b.index))
    });
    for row in cores {
        let state = metric_state(percent_number(&row.used_percent), 85.0, 95.0);
        rows.push((
            metric_gauge_row(
                &format!("cpu{}    ", row.index),
                &row.used_percent,
                percent_number(&row.used_percent),
                state,
                160,
            ),
            state_style(state),
        ));
    }
    rows
}

fn core_alert_rank(row: &DashboardCoreRow) -> u8 {
    match metric_state(percent_number(&row.used_percent), 85.0, 95.0) {
        "critical" => 0,
        "warning" => 1,
        _ => 2,
    }
}

fn storage_detail_candidates(s: &DashboardSnapshot) -> Vec<DashboardDiskRow> {
    let mut rows = Vec::new();
    if let Some(root) = s.disk_rows.iter().find(|row| row.is_root).cloned() {
        rows.push(root);
    }
    let mut rest = s
        .disk_rows
        .iter()
        .filter(|row| {
            !rows
                .iter()
                .any(|existing: &DashboardDiskRow| existing.dev == row.dev)
        })
        .cloned()
        .collect::<Vec<_>>();
    rest.sort_by(|a, b| {
        a.alert_rank()
            .cmp(&b.alert_rank())
            .then_with(|| a.dev.cmp(&b.dev))
    });
    rows.extend(rest);
    rows
}

fn network_detail_candidates(s: &DashboardSnapshot) -> Vec<DashboardNetworkRow> {
    let mut rows = Vec::new();
    if let Some(route) = s.network_rows.iter().find(|row| row.is_route).cloned() {
        rows.push(route);
    }
    for access in s.network_rows.iter().filter(|row| row.is_access) {
        if !rows
            .iter()
            .any(|existing: &DashboardNetworkRow| existing.iface == access.iface)
        {
            rows.push(access.clone());
        }
    }
    let mut rest = s
        .network_rows
        .iter()
        .filter(|row| {
            !rows
                .iter()
                .any(|existing: &DashboardNetworkRow| existing.iface == row.iface)
        })
        .cloned()
        .collect::<Vec<_>>();
    rest.sort_by(|a, b| {
        a.alert_rank(&s.network_error_state)
            .cmp(&b.alert_rank(&s.network_error_state))
            .then_with(|| a.iface.cmp(&b.iface))
    });
    rows.extend(rest);
    rows
}

fn render_disk_units(
    candidates: Vec<DashboardDiskRow>,
    available: usize,
    width: u16,
) -> Vec<(String, SemanticStyle)> {
    let units = available / 2;
    if units == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let overflow = units >= 2 && candidates.len() > units;
    let visible_units = if overflow { units - 1 } else { units };
    for row in candidates.iter().take(visible_units) {
        let style = state_style(metric_state(disk_fs_percent(&row.fs), 80.0, 90.0));
        let prefix = if row.is_root { "* " } else { "  " };
        let role = disk_role(row);
        out.push((
            format!(
                "{prefix}{} {} - {} - {} - {}",
                row.dev, row.size, row.transport, row.display, role
            ),
            SemanticStyle::Plain,
        ));
        out.push((
            format!("  sn {} - {}", row.serial, disk_fs_text(row, width)),
            style,
        ));
    }
    if overflow {
        let hidden = candidates.len() - visible_units;
        out.push((
            format!("... {hidden} more disks not shown"),
            SemanticStyle::Muted,
        ));
        out.push((String::new(), SemanticStyle::Canvas));
    }
    out
}

fn disk_role(row: &DashboardDiskRow) -> String {
    if row.is_root {
        "root /".to_string()
    } else if row.fs == "not mounted" {
        "not mounted".to_string()
    } else {
        "mounted fs".to_string()
    }
}

fn render_interface_units(
    candidates: Vec<DashboardNetworkRow>,
    available: usize,
) -> Vec<(String, SemanticStyle)> {
    let units = available / 2;
    if units == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let overflow = units >= 2 && candidates.len() > units;
    let visible_units = if overflow { units - 1 } else { units };
    for row in candidates.iter().take(visible_units) {
        let style = state_style(&row.alert_state);
        out.push((
            format!(
                "{}   {} {} {} - {} - {} - mtu {}",
                row.iface, row.role, row.kind, row.state, row.addresses, row.speed, row.mtu
            ),
            SemanticStyle::Plain,
        ));
        let alert = alert_suffix(&row.alert_state);
        let alert = if alert.is_empty() {
            String::new()
        } else {
            format!(" {alert}")
        };
        out.push((
            format!(
                "       mac {} - rx {}/s tx {}/s - drop rx {}/s tx {}/s{}",
                row.mac, row.rx_rate, row.tx_rate, row.rx_drop_rate, row.tx_drop_rate, alert
            ),
            style,
        ));
    }
    if overflow {
        let hidden = candidates.len() - visible_units;
        out.push((
            format!("... {hidden} more interfaces not shown"),
            SemanticStyle::Muted,
        ));
        out.push((String::new(), SemanticStyle::Canvas));
    }
    out
}

fn render_with_overflow(
    candidates: Vec<(String, SemanticStyle)>,
    available: usize,
    unit: &str,
) -> Vec<(String, SemanticStyle)> {
    if available == 0 {
        return Vec::new();
    }
    if candidates.len() <= available {
        return candidates;
    }
    if available == 1 {
        return candidates.into_iter().take(1).collect();
    }
    let hidden = candidates.len() - (available - 1);
    let mut rows = candidates
        .into_iter()
        .take(available - 1)
        .collect::<Vec<_>>();
    rows.push((
        format!("... {hidden} more {unit} not shown"),
        SemanticStyle::Muted,
    ));
    rows
}

fn top_process_cpu_percent(top_cpu_process: &str) -> Option<f64> {
    top_cpu_process
        .split_once("cpu ")?
        .1
        .split_whitespace()
        .next()
        .and_then(|raw| raw.strip_suffix('%').unwrap_or(raw).parse::<f64>().ok())
}

fn metric_state(value: f64, warning: f64, critical: f64) -> &'static str {
    if !value.is_finite() {
        "unavailable"
    } else if value >= critical {
        "critical"
    } else if value >= warning {
        "warning"
    } else {
        ""
    }
}

fn metric_gauge_row(
    prefix: &str,
    value: &str,
    percent: f64,
    state: &str,
    row_width: u16,
) -> String {
    let suffix_width = if state.is_empty() { 0 } else { 1 + state.len() };
    let fixed_width = prefix.len() + value.len() + 2 + suffix_width;
    let gauge = fitted_gauge(prefix.trim(), percent, row_width, fixed_width);
    if gauge.is_empty() && state.is_empty() {
        format!("{prefix}{value}")
    } else if gauge.is_empty() {
        format!("{prefix}{value} {state}")
    } else if state.is_empty() {
        format!("{prefix}{value} {gauge}")
    } else {
        format!("{prefix}{value} {gauge} {state}")
    }
}

fn compact_capacity_gauge_row(
    prefix: &str,
    used: impl AsRef<str>,
    total: impl AsRef<str>,
    percent: impl AsRef<str>,
    state: &str,
    row_width: u16,
) -> String {
    compact_capacity_gauge_row_reserved(prefix, used, total, percent, state, row_width, 0)
}

fn compact_capacity_gauge_row_reserved(
    prefix: &str,
    used: impl AsRef<str>,
    total: impl AsRef<str>,
    percent: impl AsRef<str>,
    state: &str,
    row_width: u16,
    reserved_width: usize,
) -> String {
    let value = format!("{}/{} {}", used.as_ref(), total.as_ref(), percent.as_ref());
    let row_width = row_width.saturating_sub(reserved_width as u16);
    metric_gauge_row(
        prefix,
        &value,
        percent_number(percent.as_ref()),
        state,
        row_width,
    )
}

fn disk_fs_text(row: &DashboardDiskRow, width: u16) -> String {
    if row.fs == "not mounted" {
        return "fs not mounted".to_string();
    }
    let percent = row
        .fs
        .rsplit_once('(')
        .and_then(|(_, right)| right.strip_suffix(')'))
        .unwrap_or("unavailable");
    let state = metric_state(percent_number(percent), 80.0, 90.0);
    let suffix = alert_suffix(state);
    let fixed_width = "fs ".len() + row.fs.len() + 2 + suffix.len();
    let gauge = fitted_gauge("fs", percent_number(percent), width, fixed_width);
    if gauge.is_empty() && suffix.is_empty() {
        format!("fs {}", row.fs)
    } else if gauge.is_empty() {
        format!("fs {} {}", row.fs, suffix)
    } else if suffix.is_empty() {
        format!("fs {} {}", row.fs, gauge)
    } else {
        format!("fs {} {} {}", row.fs, gauge, suffix)
    }
}

fn disk_fs_percent(fs: &str) -> f64 {
    fs.rsplit_once('(')
        .and_then(|(_, right)| right.strip_suffix(')'))
        .map(percent_number)
        .unwrap_or(0.0)
}

fn disk_summary_row(s: &DashboardSnapshot) -> String {
    let disk_count = s.disk_count.as_str();
    let root = s.disk_rows.iter().find(|row| row.is_root);
    let attention = s
        .disk_rows
        .iter()
        .filter(|row| row.alert_rank() < 2)
        .count();
    if let Some(root) = root {
        format!(
            "Disks     {disk_count} disks - root {} {} {} {} - attention {attention}",
            root.dev, root.size, root.transport, root.display
        )
    } else {
        format!("Disks     {disk_count} disks - root unavailable - attention {attention}")
    }
}

fn route_summary_row(s: &DashboardSnapshot) -> String {
    if s.route_iface == "unavailable" {
        "Route     default unavailable".to_string()
    } else {
        format!(
            "Route     default {} {} - gw {} - metric {}",
            s.route_iface, s.route_address, s.default_gateway, s.route_metric
        )
    }
}

fn interface_summary_row(s: &DashboardSnapshot) -> String {
    if s.network_interface_count == "unavailable" {
        return "NICs      unavailable".to_string();
    }
    let interface_count = s.network_rows.len();
    format!(
        "NICs      {interface_count} ifaces - route {} - access {} - attention {}",
        s.route_iface, s.ssh_access_summary, s.interface_attention_count
    )
}

fn alert_suffix(state: &str) -> &str {
    match state {
        "warning" => "warning",
        "critical" => "critical",
        "unavailable" => "unavailable",
        _ => "",
    }
}

fn fitted_gauge(label: &str, percent: f64, row_width: u16, fixed_width: usize) -> String {
    let max_gauge_cells = (row_width as usize).saturating_sub(fixed_width);
    if max_gauge_cells < 18 {
        return String::new();
    }
    let bar_width = max_gauge_cells.saturating_sub(2).min(48);
    let _ = label;
    ascii_gauge_bar(percent, bar_width)
}
