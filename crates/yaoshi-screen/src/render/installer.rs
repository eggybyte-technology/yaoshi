pub fn unsupported_display_page(failure: &DisplayNegotiationFailure) -> String {
    unsupported_display_page_with_poweroff(failure, None)
}

pub fn unsupported_display_page_with_poweroff(
    failure: &DisplayNegotiationFailure,
    auto_poweroff_seconds: Option<u64>,
) -> String {
    let env = failure.surface.envelope();
    let profile = failure.font_profile.as_deref().unwrap_or("unavailable");
    let mut lines = vec![
        failure.surface.title().to_string(),
        "Display unsupported.".to_string(),
        format!(
            "Required grid: {}x{} or larger after Terminus font negotiation.",
            env.minimum_columns, env.minimum_rows
        ),
        format!("Current grid: {}x{}", failure.columns, failure.rows),
        format!("Font profile: {profile}"),
        format!("Reason: {}", failure.reason.as_str()),
    ];
    if let Some(seconds) = auto_poweroff_seconds {
        lines.push("Press Enter to power off now.".to_string());
        lines.push(format!("Automatic poweroff in {seconds} s."));
    }
    lines.join("\n")
}

pub fn render_installer_scene(scene: &InstallerScene, width: u16, height: u16) -> RenderedFrame {
    if size_class_for_surface(Surface::Installer, width, height) == SizeClass::Unsupported {
        let failure = DisplayNegotiationFailure {
            surface: Surface::Installer,
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
    let root = centered_root(Surface::Installer, width, height);
    let (lines, focus) = installer_lines(scene, root.width);
    let content_rows = lines.content.len().min(22);
    let mut regions = vec![
        SemanticRegion {
            name: "TitleLine",
            rect: Rect::new(root.x, root.y, root.width, 1),
        },
        SemanticRegion {
            name: "StateLine",
            rect: Rect::new(root.x, root.y + 1, root.width, 1),
        },
        SemanticRegion {
            name: "ContentLines",
            rect: Rect::new(
                root.x,
                root.y + 3,
                root.width,
                22.min(root.height.saturating_sub(5)),
            ),
        },
        SemanticRegion {
            name: "ActionChoiceLine",
            rect: Rect::new(root.x, root.y + 4 + content_rows as u16, root.width, 1),
        },
        SemanticRegion {
            name: "ActionMeaningLine",
            rect: Rect::new(root.x, root.y + 5 + content_rows as u16, root.width, 1),
        },
    ];
    regions.retain(|region| region.rect.y < height);
    let mut buffer = new_buffer(width, height);
    draw_line(
        &mut buffer,
        root.x,
        root.y,
        root.width,
        &lines.title,
        SemanticStyle::Header,
    );
    draw_line(
        &mut buffer,
        root.x,
        root.y + 1,
        root.width,
        &lines.state,
        lines.state_style,
    );
    for (idx, (line, style)) in lines.content.iter().take(22).enumerate() {
        draw_line(
            &mut buffer,
            root.x,
            root.y + 3 + idx as u16,
            root.width,
            line,
            *style,
        );
    }
    let action_y = root.y + 4 + content_rows as u16;
    draw_styled_line(
        &mut buffer,
        root.x,
        action_y,
        root.width,
        &lines.action_choice,
    );
    draw_line(
        &mut buffer,
        root.x,
        action_y + 1,
        root.width,
        &lines.action_meaning,
        lines.action_meaning_style,
    );
    frame_from_buffer(buffer, regions, focus)
}

struct InstallerLines {
    title: String,
    state: String,
    state_style: SemanticStyle,
    content: Vec<(String, SemanticStyle)>,
    action_choice: Vec<StyledSegment>,
    action_meaning: String,
    action_meaning_style: SemanticStyle,
}

struct StyledSegment {
    text: String,
    style: SemanticStyle,
}

struct ActionSpec {
    label: String,
    kind: ActionKind,
    enabled: bool,
    focused: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionKind {
    Normal,
    Destructive,
}

impl ActionSpec {
    fn focused_normal(label: &str) -> Self {
        Self {
            label: label.to_string(),
            kind: ActionKind::Normal,
            enabled: true,
            focused: true,
        }
    }
}

fn styled_line(text: &str, style: SemanticStyle) -> Vec<StyledSegment> {
    vec![StyledSegment {
        text: text.to_string(),
        style,
    }]
}

fn action_choice_segments(actions: Vec<ActionSpec>) -> Vec<StyledSegment> {
    let mut segments = Vec::new();
    for (idx, action) in actions.into_iter().enumerate() {
        if idx > 0 {
            segments.push(StyledSegment {
                text: "    ".to_string(),
                style: SemanticStyle::Action,
            });
        }
        let label = truncate_to_width(&action.label, 48);
        let (text, style) = if action.focused && action.enabled {
            let style = if action.kind == ActionKind::Destructive {
                SemanticStyle::DestructiveFocus
            } else {
                SemanticStyle::ActionFocus
            };
            (format!("[ {label} ]"), style)
        } else if action.enabled {
            (label, SemanticStyle::Action)
        } else {
            (label, SemanticStyle::Disabled)
        };
        segments.push(StyledSegment { text, style });
    }
    segments
}

fn field_rows(
    label: &str,
    value: &str,
    width: u16,
    max_rows: usize,
) -> Vec<(String, SemanticStyle)> {
    let label = truncate_to_width(label, 10).to_ascii_lowercase();
    let label_col = format!("{label:<10}");
    let value_width = width.saturating_sub(10) as usize;
    if value_width == 0 || max_rows == 0 {
        return Vec::new();
    }
    let mut rows = Vec::new();
    let mut current = String::new();
    for token in AsciiCellString::new(value).into_string().split(' ') {
        if token.is_empty() {
            continue;
        }
        if token.len() > value_width {
            if !current.is_empty() {
                rows.push(current);
                current = String::new();
            }
            rows.push(truncate_to_width(token, value_width));
            continue;
        }
        let next_len = if current.is_empty() {
            token.len()
        } else {
            current.len() + 1 + token.len()
        };
        if next_len <= value_width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(token);
        } else {
            rows.push(current);
            current = token.to_string();
        }
    }
    if !current.is_empty() || rows.is_empty() {
        rows.push(current);
    }
    rows.into_iter()
        .take(max_rows)
        .enumerate()
        .map(|(idx, value)| {
            if idx == 0 {
                (format!("{label_col}{value}"), SemanticStyle::Plain)
            } else {
                (format!("{:10}{value}", ""), SemanticStyle::Plain)
            }
        })
        .collect()
}

fn section_row(label: &str) -> (String, SemanticStyle) {
    (label.to_string(), SemanticStyle::Section)
}

fn task_row(state: &str, label: &str, detail: impl AsRef<str>) -> (String, SemanticStyle) {
    let token = match state {
        "done" => "[done]",
        "active" => "[active]",
        "failed" => "[failed]",
        _ => "[pending]",
    };
    let detail = detail.as_ref();
    let text = if detail.is_empty() {
        format!("{token:<9} {label}")
    } else {
        format!("{token:<9} {label} {detail}")
    };
    let style = match state {
        "done" => SemanticStyle::Success,
        "active" => SemanticStyle::Focus,
        "failed" => SemanticStyle::Critical,
        _ => SemanticStyle::Muted,
    };
    (text, style)
}

fn installer_lines(scene: &InstallerScene, width: u16) -> (InstallerLines, Option<String>) {
    match scene {
        InstallerScene::Prepare(model) => {
            let elapsed = model.media_check_elapsed.as_secs_f64().clamp(0.0, 10.0);
            let device_state = prepare_device_state(&model.modules_state);
            let media_state = prepare_media_state(&model.media_state);
            let percent = if media_state == "pending" {
                0.0
            } else {
                (elapsed * 10.0).clamp(0.0, 100.0)
            };
            let gauge = gauge("media", percent, width as usize);
            let payload_values = match (
                model.target_minimum_bytes,
                model.payload_planned_extent_bytes,
                model.payload_container_bytes,
                model.payload_zero_extent_bytes,
            ) {
                (Some(target), Some(planned), Some(packed), Some(zeroes)) => {
                    Some((target, planned, packed, zeroes))
                }
                _ => None,
            };
            let payload_detail = payload_values
                .map(|_| "metadata ok".to_string())
                .unwrap_or_else(|| prepare_payload_state(&model.payload_state).to_string());
            let current = prepare_step_text(
                &device_state,
                media_state,
                &payload_detail,
                prepare_targets_state(&model.disk_inspection_state),
            );
            let drivers_state = if matches!(device_state.as_str(), "ready") {
                "done"
            } else if matches!(device_state.as_str(), "failed") {
                "failed"
            } else {
                "active"
            };
            let media_task_state = match media_state {
                "found" => "done",
                "not found" | "ambiguous" => "failed",
                "searching" | "checking" => "active",
                _ => "pending",
            };
            let payload_task_state = match payload_detail.as_str() {
                "metadata ok" | "valid" => "done",
                "invalid" => "failed",
                "validating" => "active",
                _ => "pending",
            };
            let target_state = prepare_targets_state(&model.disk_inspection_state);
            let target_task_state = match target_state {
                "ready" => "done",
                "failed" => "failed",
                "inspecting" => "active",
                _ => "pending",
            };
            let drivers_detail = prepare_driver_detail(&model.modules_state, drivers_state);
            let media_detail = match media_task_state {
                "active" => format!(
                    "{:.3} s / 10.000 s {gauge} {}",
                    elapsed,
                    format_percent(percent, true)
                ),
                "done" => format!("found in {:.3} s", elapsed),
                "failed" => media_state.to_string(),
                _ => format!("0.000 s / 10.000 s {gauge} 0.000%"),
            };
            let target_detail = if target_task_state == "done" {
                "0 usable target disks"
            } else if target_task_state == "active" {
                "inspecting local disks"
            } else if target_task_state == "failed" {
                "disk inspection failed"
            } else {
                ""
            };
            (
                InstallerLines {
                    title: "Yaoshi Installer - Prepare".to_string(),
                    state: "safe - no disk will be changed".to_string(),
                    state_style: SemanticStyle::Success,
                    content: {
                        let mut content = vec![
                            (
                                "status    preparing installer".to_string(),
                                SemanticStyle::Plain,
                            ),
                            (format!("current   {current}"), SemanticStyle::Plain),
                            task_row("done", "console ready", ""),
                            task_row(drivers_state, "input and storage drivers", drivers_detail),
                            task_row(media_task_state, "installer media", media_detail),
                            task_row(payload_task_state, "payload validation", payload_detail),
                            task_row(target_task_state, "target disk scan", target_detail),
                        ];
                        if let Some((target, planned, packed, zeroes)) = payload_values {
                            content.push(section_row("payload"));
                            content.extend(field_rows(
                                "image",
                                &format_decimal_bytes(target),
                                width,
                                1,
                            ));
                            content.extend(field_rows(
                                "write",
                                &format_decimal_bytes(planned),
                                width,
                                1,
                            ));
                            content.extend(field_rows(
                                "packed",
                                &format_decimal_bytes(packed),
                                width,
                                1,
                            ));
                            content.extend(field_rows(
                                "zero",
                                &format_decimal_bytes(zeroes),
                                width,
                                1,
                            ));
                        }
                        content
                    },
                    action_choice: action_choice_segments(vec![ActionSpec::focused_normal(
                        "Power off",
                    )]),
                    action_meaning: "Power off without opening a target disk.".to_string(),
                    action_meaning_style: SemanticStyle::Plain,
                },
                Some("Power off".to_string()),
            )
        }
        InstallerScene::Target(model) => target_lines(model, width),
        InstallerScene::Install(model) => install_lines(model, width),
        InstallerScene::Write(model) => write_lines(model, width),
        InstallerScene::Done(model) => done_lines(model),
        InstallerScene::Stopped(model) => stopped_lines(model),
        InstallerScene::Poweroff => (
            InstallerLines {
                title: "Yaoshi Installer - Poweroff".to_string(),
                state: "exiting - poweroff requested - input locked".to_string(),
                state_style: SemanticStyle::Warning,
                content: vec![(
                    "syncing disks and requesting poweroff".to_string(),
                    SemanticStyle::Plain,
                )],
                action_choice: styled_line("Input locked", SemanticStyle::Muted),
                action_meaning: "Exiting; wait for firmware or power state change.".to_string(),
                action_meaning_style: SemanticStyle::Plain,
            },
            None,
        ),
        InstallerScene::Reboot => (
            InstallerLines {
                title: "Yaoshi Installer - Reboot".to_string(),
                state: "exiting - reboot requested - input locked".to_string(),
                state_style: SemanticStyle::Warning,
                content: vec![(
                    "syncing disks and requesting reboot".to_string(),
                    SemanticStyle::Plain,
                )],
                action_choice: styled_line("Input locked", SemanticStyle::Muted),
                action_meaning: "Exiting; wait for firmware or power state change.".to_string(),
                action_meaning_style: SemanticStyle::Plain,
            },
            None,
        ),
    }
}

fn target_lines(model: &TargetModel, _width: u16) -> (InstallerLines, Option<String>) {
    let selectable = target_action_candidates(&model.candidates);
    let has_disks = !selectable.is_empty();
    let selected_index = model
        .selected_visible
        .min(selectable.len().saturating_sub(1));
    let mut content = Vec::new();
    if has_disks {
        content.push(section_row("image"));
        content.extend(field_rows(
            "target size",
            &format_decimal_bytes(model.target_minimum_bytes),
            _width,
            1,
        ));
        content.extend(field_rows(
            "write",
            &format_decimal_bytes(model.payload_planned_extent_bytes),
            _width,
            1,
        ));
        content.extend(field_rows(
            "packed",
            &format_decimal_bytes(model.payload_container_bytes),
            _width,
            1,
        ));
        content.push(section_row("target disks"));
        let max_rows = if selectable.len() > 4 { 3 } else { 4 };
        let shown = selectable.len().min(max_rows);
        let selected = &selectable[selected_index];
        for (idx, candidate) in selectable.iter().take(shown).enumerate() {
            let marker = if idx == selected_index { ">" } else { " " };
            let stable_id = candidate
                .disk
                .stable_disk_id
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| truncate_to_width(&name.to_string_lossy(), 32))
                .unwrap_or_else(|| "unavailable".to_string());
            let style = if idx == selected_index {
                SemanticStyle::Focus
            } else {
                SemanticStyle::Plain
            };
            content.push((
                format!(
                    "{marker} {}  {}  {}  {}  id {}",
                    candidate.disk.dev_path.display(),
                    format_capacity_binary(candidate.disk.byte_size),
                    existing_label(candidate.existing),
                    candidate_action_label(candidate.status),
                    stable_id,
                ),
                style,
            ));
        }
        if selectable.len() > shown {
            content.push((
                format!(
                    "... {} more target disks not shown",
                    selectable.len() - shown
                ),
                SemanticStyle::Muted,
            ));
        }
        let percent =
            payload_footprint_percent(model.target_minimum_bytes, selected.disk.byte_size);
        let model_text = selected.disk.model.as_deref().unwrap_or("unavailable");
        let serial_text = selected.disk.serial.as_deref().unwrap_or("unavailable");
        let stable_id = selected
            .disk
            .stable_disk_id
            .as_ref()
            .map(|path| AsciiCellString::new(&path.display().to_string()).into_string())
            .unwrap_or_else(|| "unavailable".to_string());
        content.push(section_row("selected"));
        content.extend(field_rows(
            "disk",
            &selected.disk.dev_path.display().to_string(),
            _width,
            1,
        ));
        content.extend(field_rows(
            "size",
            &format!(
                "{} - sector {} - existing {}",
                format_capacity_binary(selected.disk.byte_size),
                format_exact_byte_count(selected.disk.logical_block_size),
                existing_label(selected.existing)
            ),
            _width,
            1,
        ));
        content.extend(field_rows("identity", &stable_id, _width, 2));
        content.extend(field_rows("model", model_text, _width, 2));
        content.extend(field_rows("serial", serial_text, _width, 2));
        content.extend(field_rows(
            "usage",
            &format!("{} before first boot growth", format_percent(percent, true)),
            _width,
            1,
        ));
        content.push(section_row("other disks"));
        content.push((
            format!("  {}", other_disk_summary(model)),
            SemanticStyle::Muted,
        ));
    } else {
        content.push((
            "no usable target disk found".to_string(),
            SemanticStyle::Warning,
        ));
        content.push(section_row("required"));
        for row in [
            "separate from installer media".to_string(),
            "stable sysfs disk identity".to_string(),
            "512 B sectors".to_string(),
            format!(
                "at least {}",
                format_decimal_bytes(model.target_minimum_bytes)
            ),
            "readable whole disk".to_string(),
        ] {
            content.push((format!("  {row}"), SemanticStyle::Plain));
        }
        content.push(section_row("other"));
        content.push((
            format!("  {}", other_disk_summary(model)),
            SemanticStyle::Muted,
        ));
    }
    let effective_focus = if !has_disks && model.focus == TargetFocus::ConfirmTarget {
        TargetFocus::Refresh
    } else {
        model.focus
    };
    let action_choice = action_choice_segments(vec![
        ActionSpec {
            label: "Confirm target".to_string(),
            kind: ActionKind::Normal,
            enabled: has_disks,
            focused: effective_focus == TargetFocus::ConfirmTarget,
        },
        ActionSpec {
            label: "Refresh".to_string(),
            kind: ActionKind::Normal,
            enabled: true,
            focused: effective_focus == TargetFocus::Refresh,
        },
        ActionSpec {
            label: "Power off".to_string(),
            kind: ActionKind::Normal,
            enabled: true,
            focused: effective_focus == TargetFocus::PowerOff,
        },
    ]);
    let action_meaning = match (has_disks, model.focus) {
        (true, TargetFocus::ConfirmTarget) => {
            "Confirm the selected disk before install or erase; Up/Down changes selected disk."
        }
        (_, TargetFocus::Refresh) | (false, TargetFocus::ConfirmTarget) => {
            "Rescan disks and rebuild the target list."
        }
        (_, TargetFocus::PowerOff) => "Power off without starting target writes.",
    }
    .to_string();
    let focus = match model.focus {
        TargetFocus::ConfirmTarget if has_disks => Some("Confirm target".to_string()),
        TargetFocus::ConfirmTarget => Some("Refresh".to_string()),
        TargetFocus::Refresh => Some("Refresh".to_string()),
        TargetFocus::PowerOff => Some("Power off".to_string()),
    };
    (
        InstallerLines {
            title: "Yaoshi Installer - Target".to_string(),
            state: if has_disks {
                "safe - choose target disk - installer media excluded".to_string()
            } else {
                "safe - no writes started - no usable target disk".to_string()
            },
            state_style: SemanticStyle::Success,
            content,
            action_choice,
            action_meaning,
            action_meaning_style: SemanticStyle::Plain,
        },
        focus,
    )
}

fn target_action_candidates(candidates: &[TargetDiskCandidate]) -> Vec<TargetDiskCandidate> {
    let mut selectable = candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.status,
                CandidateStatus::Selectable
                    | CandidateStatus::InstalledTarget
                    | CandidateStatus::BlockedByInstalledTarget
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    selectable.sort_by(|a, b| a.disk.kernel_name.cmp(&b.disk.kernel_name));
    selectable
}

fn candidate_action_label(status: CandidateStatus) -> &'static str {
    match status {
        CandidateStatus::Selectable => "install",
        CandidateStatus::InstalledTarget | CandidateStatus::BlockedByInstalledTarget => "erase-only",
        CandidateStatus::InstallerMedia
        | CandidateStatus::TooSmall
        | CandidateStatus::UnsupportedSectorSize
        | CandidateStatus::NoStableId
        | CandidateStatus::ReadError => "blocked",
    }
}

fn install_lines(model: &InstallModel, width: u16) -> (InstallerLines, Option<String>) {
    let disk = &model.target.disk;
    let dev = disk.dev_path.display().to_string();
    let remaining = disk.byte_size.saturating_sub(model.target_minimum_bytes);
    let destructive = format!("Install Yaoshi to {dev}");
    let erase = format!("Erase disk only {dev}");
    let install_enabled = model.target.status == CandidateStatus::Selectable;
    let stable_id = disk
        .stable_disk_id
        .as_ref()
        .map(|path| AsciiCellString::new(&path.display().to_string()).into_string())
        .unwrap_or_else(|| "unavailable".to_string());
    let model_text = disk.model.as_deref().unwrap_or("unavailable");
    let serial_text = disk.serial.as_deref().unwrap_or("unavailable");
    let action_choice = action_choice_segments(vec![
        ActionSpec {
            label: "Back".to_string(),
            kind: ActionKind::Normal,
            enabled: true,
            focused: model.focus == InstallFocus::Back,
        },
        ActionSpec {
            label: erase.clone(),
            kind: ActionKind::Destructive,
            enabled: true,
            focused: model.focus == InstallFocus::Erase,
        },
        ActionSpec {
            label: destructive.clone(),
            kind: ActionKind::Destructive,
            enabled: install_enabled,
            focused: model.focus == InstallFocus::Destructive,
        },
        ActionSpec {
            label: "Power off".to_string(),
            kind: ActionKind::Normal,
            enabled: true,
            focused: model.focus == InstallFocus::PowerOff,
        },
    ]);
    let action_meaning = match model.focus {
        InstallFocus::Back => {
            "Return to target selection without opening the disk for writing.".to_string()
        }
        InstallFocus::Erase => format!("Clear boot records and partition metadata on {dev}."),
        InstallFocus::Destructive => format!("Write the Yaoshi image to {dev}."),
        InstallFocus::PowerOff => "Power off without starting target writes.".to_string(),
    };
    let mut content = Vec::new();
    content.push(section_row("target"));
    content.extend(field_rows("disk", &dev, width, 1));
    content.extend(field_rows(
        "size",
        &format_capacity_binary(disk.byte_size),
        width,
        1,
    ));
    content.extend(field_rows(
        "sector",
        &format_exact_byte_count(disk.logical_block_size),
        width,
        1,
    ));
    content.extend(field_rows(
        "existing",
        existing_label(model.target.existing),
        width,
        1,
    ));
    content.push(section_row("identity"));
    content.extend(field_rows("id", &stable_id, width, 2));
    content.extend(field_rows("model", model_text, width, 2));
    content.extend(field_rows("serial", serial_text, width, 2));
    content.push(section_row("install plan"));
    content.extend(field_rows(
        "write",
        &format!(
            "{} Yaoshi image",
            format_decimal_bytes(model.payload_planned_extent_bytes)
        ),
        width,
        1,
    ));
    content.extend(field_rows(
        "zero",
        &format!(
            "{} declared empty bytes",
            format_decimal_bytes(model.payload_zero_extent_bytes)
        ),
        width,
        1,
    ));
    content.extend(field_rows(
        "grow",
        &format!(
            "{} available for first boot root growth",
            format_decimal_bytes(remaining)
        ),
        width,
        1,
    ));
    content.extend(field_rows(
        "result",
        "replaces boot records and filesystem metadata on the target disk",
        width,
        1,
    ));
    content.push(section_row("erase plan"));
    content.extend(field_rows(
        "clear",
        "target head and tail metadata only",
        width,
        1,
    ));
    content.extend(field_rows(
        "result",
        "removes partition tables and boot records without installing Yaoshi",
        width,
        1,
    ));
    (
        InstallerLines {
            title: "Yaoshi Installer - Confirm Install".to_string(),
            state: "danger - no writes started - back is still safe".to_string(),
            state_style: SemanticStyle::Critical,
            content,
            action_choice,
            action_meaning,
            action_meaning_style: SemanticStyle::Plain,
        },
        Some(if model.focus == InstallFocus::Back {
            "Back".to_string()
        } else if model.focus == InstallFocus::Erase {
            erase
        } else if model.focus == InstallFocus::PowerOff {
            "Power off".to_string()
        } else {
            destructive
        }),
    )
}

fn write_lines(model: &WriteModel, width: u16) -> (InstallerLines, Option<String>) {
    let copy_percent = percent_ratio(model.planned_written_bytes, model.planned_total_bytes);
    let head_percent = percent_ratio(
        model.head_scrub_written_bytes,
        model.target_head_scrub_bytes,
    );
    let tail_percent = percent_ratio(
        model.tail_scrub_written_bytes,
        model.target_tail_scrub_bytes,
    );
    let current = model
        .current_rate_bps
        .map(|v| format_byte_rate_binary(v as f64))
        .unwrap_or_else(|| "measuring".to_string());
    let average = model
        .average_rate_bps
        .map(|v| format_byte_rate_binary(v as f64))
        .unwrap_or_else(|| "measuring".to_string());
    let eta = model
        .eta
        .map(|v| format_duration_seconds_3(v.as_secs_f64()))
        .unwrap_or_else(|| "measuring".to_string());
    let (status, image_label, action_meaning) = match model.operation {
        TargetOperation::Install => (
            "status    installing Yaoshi".to_string(),
            "image",
            "Writing target disk; wait for completion.",
        ),
        TargetOperation::Erase => (
            "status    erasing target metadata".to_string(),
            "clear",
            "Erasing target metadata; wait for completion.",
        ),
    };
    let mut content = vec![
        (status, SemanticStyle::Plain),
        (
            format!("target    {}", model.target_dev_path.display()),
            SemanticStyle::Plain,
        ),
        (
            format!(
                "{image_label:<10}{}",
                format_decimal_bytes(model.target_image_bytes)
            ),
            SemanticStyle::Plain,
        ),
        section_row("tasks"),
    ];
    let current_index = write_task_index(model.task);
    for (idx, task) in WriteTask::ordered().into_iter().enumerate() {
        let state = if idx < current_index {
            "done"
        } else if idx == current_index {
            "active"
        } else {
            "pending"
        };
        let detail = match task {
            WriteTask::VerifyTarget | WriteTask::FinalizeWrites => String::new(),
            WriteTask::PrepareDiskBeginning => format!(
                "{} / {} {} {}",
                format_decimal_bytes(model.head_scrub_written_bytes),
                format_decimal_bytes(model.target_head_scrub_bytes),
                gauge("beginning", head_percent, width as usize),
                format_percent(head_percent, true)
            ),
            WriteTask::PrepareDiskEnd => format!(
                "{} / {} {} {}",
                format_decimal_bytes(model.tail_scrub_written_bytes),
                format_decimal_bytes(model.target_tail_scrub_bytes),
                gauge("end", tail_percent, width as usize),
                format_percent(tail_percent, true)
            ),
            WriteTask::CopyYaoshiImage => match model.operation {
                TargetOperation::Install => format!(
                    "{} {}",
                    format_percent(copy_percent, true),
                    gauge("image", copy_percent, width as usize)
                ),
                TargetOperation::Erase => "not used".to_string(),
            },
        };
        content.push(task_row(state, task.label(), detail));
    }
    content.push(section_row("progress"));
    content.push((
        format!(
            "written   {} / {}",
            format_decimal_bytes(model.planned_written_bytes),
            format_decimal_bytes(model.planned_total_bytes)
        ),
        SemanticStyle::Plain,
    ));
    content.push((
        format!(
            "zeroed    {} / {}",
            format_decimal_bytes(model.zero_written_bytes),
            format_decimal_bytes(model.payload_zero_extent_bytes)
        ),
        SemanticStyle::Plain,
    ));
    content.push((format!("rate      current {current}"), SemanticStyle::Plain));
    content.push((format!("average   {average}"), SemanticStyle::Plain));
    content.push((
        format!(
            "left      {}",
            format_decimal_bytes(
                model
                    .planned_total_bytes
                    .saturating_sub(model.planned_written_bytes)
            )
        ),
        SemanticStyle::Plain,
    ));
    content.push((format!("time      {eta}"), SemanticStyle::Plain));
    (
        InstallerLines {
            title: "Yaoshi Installer - Write".to_string(),
            state: format!(
                "locked - keep power connected - target {}",
                model.target_dev_path.display()
            ),
            state_style: SemanticStyle::Warning,
            content,
            action_choice: styled_line("Input locked", SemanticStyle::Muted),
            action_meaning: action_meaning.to_string(),
            action_meaning_style: SemanticStyle::Plain,
        },
        None,
    )
}

fn done_lines(model: &DoneModel) -> (InstallerLines, Option<String>) {
    let stable_id = model
        .target_stable_id
        .as_deref()
        .map(|id| {
            Path::new(id)
                .file_name()
                .map(|name| truncate_to_width(&name.to_string_lossy(), 32))
                .unwrap_or_else(|| truncate_to_width(id, 32))
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let effective_focus =
        if model.operation == TargetOperation::Install && model.focus == DoneFocus::ChooseTarget {
            DoneFocus::Reboot
        } else {
            model.focus
        };
    let mut actions = Vec::new();
    if model.operation == TargetOperation::Erase {
        actions.push(ActionSpec {
            label: "Choose target".to_string(),
            kind: ActionKind::Normal,
            enabled: true,
            focused: effective_focus == DoneFocus::ChooseTarget,
        });
    }
    actions.extend([
        ActionSpec {
            label: "Reboot".to_string(),
            kind: ActionKind::Normal,
            enabled: true,
            focused: effective_focus == DoneFocus::Reboot,
        },
        ActionSpec {
            label: "Power off".to_string(),
            kind: ActionKind::Normal,
            enabled: true,
            focused: effective_focus == DoneFocus::PowerOff,
        },
    ]);
    let action_choice = action_choice_segments(actions);
    let action_meaning = match effective_focus {
        DoneFocus::ChooseTarget => {
            "Return to target selection without rebooting or powering off.".to_string()
        }
        DoneFocus::Reboot => match model.operation {
            TargetOperation::Install => {
                "Reboot into the installed target after removing installer media.".to_string()
            }
            TargetOperation::Erase => "Reboot after completed and flushed erase.".to_string(),
        },
        DoneFocus::PowerOff => "Power off after completed and flushed writes.".to_string(),
    };
    let (section, written_label, next_rows) = match model.operation {
        TargetOperation::Install => (
            "installed",
            "written",
            vec![
                "  remove installer media".to_string(),
                "  boot with one Yaoshi target attached".to_string(),
                "  first boot grows root filesystem when possible".to_string(),
            ],
        ),
        TargetOperation::Erase => (
            "erased",
            "cleared",
            vec![
                "  choose another target or install now".to_string(),
                "  reboot or power off when finished".to_string(),
            ],
        ),
    };
    let mut content = vec![
        section_row(section),
        (
            format!("  target       {}", model.target_dev_path.display()),
            SemanticStyle::Plain,
        ),
        (format!("  identity     {stable_id}"), SemanticStyle::Plain),
        (
            format!(
                "  {written_label:<12}{}",
                format_decimal_bytes(model.payload_planned_extent_bytes)
            ),
            SemanticStyle::Plain,
        ),
        (
            format!(
                "  zeroed       {}",
                format_decimal_bytes(model.payload_zero_extent_bytes)
            ),
            SemanticStyle::Plain,
        ),
    ];
    if model.operation == TargetOperation::Install {
        content.push(("  boot         UEFI fallback".to_string(), SemanticStyle::Plain));
    }
    content.push(section_row("next"));
    content.extend(next_rows.into_iter().map(|row| (row, SemanticStyle::Plain)));
    if model.operation == TargetOperation::Install {
        content.push(section_row("access"));
        content.push(("  SSH root key".to_string(), SemanticStyle::Plain));
        content.push(("  tty2 root shell".to_string(), SemanticStyle::Plain));
    }
    (
        InstallerLines {
            title: match model.operation {
                TargetOperation::Install => "Yaoshi Installer - Done".to_string(),
                TargetOperation::Erase => "Yaoshi Installer - Erase Result".to_string(),
            },
            state: match model.operation {
                TargetOperation::Install => format!(
                    "complete - writes flushed - target {}",
                    model.target_dev_path.display()
                ),
                TargetOperation::Erase => format!(
                    "erase complete - metadata cleared - target {}",
                    model.target_dev_path.display()
                ),
            },
            state_style: SemanticStyle::Success,
            content,
            action_choice,
            action_meaning,
            action_meaning_style: SemanticStyle::Plain,
        },
        Some(match effective_focus {
            DoneFocus::ChooseTarget => "Choose target".to_string(),
            DoneFocus::Reboot => "Reboot".to_string(),
            DoneFocus::PowerOff => "Power off".to_string(),
        }),
    )
}

fn stopped_lines(model: &StoppedModel) -> (InstallerLines, Option<String>) {
    let writes_started = model.failure_write_state == FailureWriteState::TargetWriteStarted;
    let state = if writes_started {
        "target write started"
    } else {
        "no target writes started"
    };
    let recovery = if writes_started {
        "power off; reinstall to the same disk after checking hardware"
    } else {
        "power off, check installer media and target disk, then boot again"
    };
    let mut content = vec![
        (
            format!("reason    {}", model.reason.sentence()),
            SemanticStyle::Critical,
        ),
        (
            format!("task      {}", model.failed_step_name),
            SemanticStyle::Plain,
        ),
        (
            format!(
                "target    {}",
                model
                    .affected_disk
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "none".to_string())
            ),
            SemanticStyle::Plain,
        ),
        (format!("state     {state}"), SemanticStyle::Plain),
        (format!("next      {recovery}"), SemanticStyle::Plain),
    ];
    if let Some(seconds) = model.auto_poweroff_seconds {
        content.push((
            format!("poweroff  automatic in {seconds} s; Enter powers off now"),
            SemanticStyle::Warning,
        ));
    }
    (
        InstallerLines {
            title: "Yaoshi Installer - Stopped".to_string(),
            state: if writes_started {
                "failed - target image incomplete".to_string()
            } else {
                "failed - no target writes started".to_string()
            },
            state_style: SemanticStyle::Critical,
            content,
            action_choice: action_choice_segments(vec![ActionSpec::focused_normal("Power off")]),
            action_meaning: if writes_started {
                "power off; reinstall to the same disk after checking hardware".to_string()
            } else {
                "power off, check installer media and target disk, then boot again".to_string()
            },
            action_meaning_style: SemanticStyle::Plain,
        },
        Some("Power off".to_string()),
    )
}
