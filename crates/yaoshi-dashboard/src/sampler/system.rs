fn read_hostname() -> String {
    fs::read_to_string("/etc/hostname")
        .ok()
        .and_then(|text| {
            text.lines()
                .map(|line| line.trim_matches(|c: char| c.is_ascii_whitespace()))
                .find(|line| !line.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unavailable".to_string())
}

fn read_uptime() -> String {
    let Some(seconds) = fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
        .map(|value| value.floor() as u64)
    else {
        return "unavailable".to_string();
    };
    if seconds >= 86_400 {
        format!("{}d {}h", seconds / 86_400, (seconds % 86_400) / 3600)
    } else if seconds >= 3600 {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    } else if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn read_cpu_model() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .map(|text| cpu_display_from_cpuinfo(&text))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unavailable".to_string())
}

fn read_kernel_release() -> String {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unavailable".to_string())
}

fn read_dmi_field(name: &str) -> String {
    read_nonempty(Path::new("/sys/class/dmi/id").join(name))
}

fn read_cpu_count() -> String {
    if let Ok(text) = fs::read_to_string("/proc/cpuinfo") {
        let count = text
            .lines()
            .filter(|line| {
                line.split_once(':')
                    .is_some_and(|(key, _)| key.trim() == "processor")
            })
            .count();
        if count > 0 {
            return count.to_string();
        }
    }
    fs::read_to_string("/proc/stat")
        .ok()
        .map(|text| {
            text.lines()
                .filter(|line| {
                    let Some(name) = line.split_whitespace().next() else {
                        return false;
                    };
                    name.strip_prefix("cpu").is_some_and(|suffix| {
                        !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
                    })
                })
                .count()
        })
        .filter(|count| *count > 0)
        .map(|count| count.to_string())
        .unwrap_or_else(|| "unavailable".to_string())
}

fn read_cpu_times() -> Option<CpuTimes> {
    let text = fs::read_to_string("/proc/stat").ok()?;
    let line = text.lines().find(|line| line.starts_with("cpu "))?;
    let aggregate = parse_cpu_time_fields(line)?;
    let mut cores = Vec::new();
    for line in text.lines() {
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };
        if name.strip_prefix("cpu").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
        }) && let Some(core) = parse_cpu_time_fields(line)
        {
            cores.push(CoreTimes {
                idle: core.idle,
                total: core.total,
            });
        }
    }
    Some(CpuTimes {
        idle: aggregate.idle,
        total: aggregate.total,
        cores,
    })
}

fn parse_cpu_time_fields(line: &str) -> Option<CoreTimes> {
    let values = line
        .split_whitespace()
        .skip(1)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() < 4 {
        return None;
    }
    let idle = values
        .get(3)
        .copied()
        .unwrap_or(0)
        .saturating_add(values.get(4).copied().unwrap_or(0));
    let total = values.iter().copied().sum();
    Some(CoreTimes { idle, total })
}

fn render_core_rows(previous: &[CoreTimes], current: &[CoreTimes]) -> Vec<DashboardCoreRow> {
    let mut rows = current
        .iter()
        .enumerate()
        .map(|(index, now)| {
            let used_percent = previous
                .get(index)
                .and_then(|previous| {
                    let total_delta = now.total.saturating_sub(previous.total);
                    (total_delta > 0).then(|| {
                        let idle_delta = now.idle.saturating_sub(previous.idle);
                        let used = total_delta.saturating_sub(idle_delta);
                        format_percent_3(used, total_delta)
                    })
                })
                .unwrap_or_else(|| "measuring".to_string());
            DashboardCoreRow {
                index,
                used_percent,
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        percent_for_sort(&b.used_percent)
            .partial_cmp(&percent_for_sort(&a.used_percent))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.index.cmp(&b.index))
    });
    rows
}

fn read_loadavg() -> (String, String, String) {
    let Some(values) = fs::read_to_string("/proc/loadavg").ok().and_then(|text| {
        let parsed = text
            .split_whitespace()
            .take(3)
            .map(str::parse::<f64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        (parsed.len() == 3).then_some(parsed)
    }) else {
        return (
            "unavailable".to_string(),
            "unavailable".to_string(),
            "unavailable".to_string(),
        );
    };
    (
        format!("{:.3}", values[0]),
        format!("{:.3}", values[1]),
        format!("{:.3}", values[2]),
    )
}

fn read_memory() -> MemorySummary {
    let mut values = BTreeMap::new();
    if let Ok(text) = fs::read_to_string("/proc/meminfo") {
        for line in text.lines() {
            let Some((key, rest)) = line.split_once(':') else {
                continue;
            };
            let Some(kib) = rest
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u64>().ok())
            else {
                continue;
            };
            values.insert(key.to_string(), kib.saturating_mul(1024));
        }
    }
    let total = values.get("MemTotal").copied();
    let available = values.get("MemAvailable").copied();
    let swap_total = values.get("SwapTotal").copied();
    let swap_free = values.get("SwapFree").copied();
    let cache = values
        .get("Cached")
        .copied()
        .zip(values.get("SReclaimable").copied())
        .zip(values.get("Shmem").copied())
        .map(|((cached, sreclaimable), shmem)| {
            cached.saturating_add(sreclaimable).saturating_sub(shmem)
        });
    let dirty = values.get("Dirty").copied();
    let used = total
        .zip(available)
        .map(|(total, available)| total.saturating_sub(available));
    MemorySummary {
        used: used.map(format_bytes_binary).unwrap_or_else(unavailable),
        total: total.map(format_bytes_binary).unwrap_or_else(unavailable),
        percent: used
            .zip(total)
            .map(|(used, total)| format_percent_3(used, total))
            .unwrap_or_else(unavailable),
        available: available
            .map(format_bytes_binary)
            .unwrap_or_else(unavailable),
        cache: cache.map(format_bytes_binary).unwrap_or_else(unavailable),
        dirty: dirty.map(format_bytes_binary).unwrap_or_else(unavailable),
        swap_used: swap_total
            .zip(swap_free)
            .map(|(total, free)| format_bytes_binary(total.saturating_sub(free)))
            .unwrap_or_else(unavailable),
        swap_total: swap_total
            .map(format_bytes_binary)
            .unwrap_or_else(unavailable),
        swap_percent: match swap_total.zip(swap_free) {
            Some((0, _)) => "0.000%".to_string(),
            Some((total, free)) => format_percent_3(total.saturating_sub(free), total),
            None => unavailable(),
        },
    }
}

fn root_expansion_state() -> String {
    match fs::read_to_string("/var/lib/yaoshi/root-growth-state") {
        Ok(text) => yaoshi_dashboard::root_expansion_state_from_text(Some(&text)).to_string(),
        Err(_) => yaoshi_dashboard::root_expansion_state_from_text(None).to_string(),
    }
}

fn prepare_state() -> String {
    match fs::read_to_string("/var/lib/yaoshi/prepare-state") {
        Ok(text) => prepare_state_from_text(Some(&text)).to_string(),
        Err(_) => prepare_state_from_text(None).to_string(),
    }
}

fn root_partuuid() -> String {
    fs::read_to_string("/proc/cmdline")
        .ok()
        .and_then(|cmdline| {
            cmdline
                .split_whitespace()
                .find_map(|field| field.strip_prefix("root=PARTUUID=").map(str::to_string))
        })
        .unwrap_or_else(unavailable)
}

fn read_access_summary() -> AccessSummary {
    let key_count = fs::read_to_string("/root/.ssh/authorized_keys")
        .ok()
        .map(|keys| keys.lines().filter(|line| !line.trim().is_empty()).count());
    AccessSummary {
        ssh_root_key_state: match key_count {
            Some(count) if count > 0 => "enabled".to_string(),
            Some(_) => "disabled".to_string(),
            None => "unavailable".to_string(),
        },
        authorized_key_count: key_count
            .map(|count| count.to_string())
            .unwrap_or_else(unavailable),
    }
}

fn read_process_summary(top_cpu: Option<String>) -> ProcessSummary {
    let Ok(entries) = fs::read_dir("/proc") else {
        return ProcessSummary {
            total: unavailable(),
            running: unavailable(),
            blocked: unavailable(),
            top_cpu: unavailable(),
            top_rss: unavailable(),
        };
    };
    let mut total = 0u64;
    let mut running = 0u64;
    let mut blocked = 0u64;
    let mut top_rss: Option<(u32, String, u64)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let pid = name.parse::<u32>().unwrap_or(0);
        total += 1;
        if let Ok(stat) = fs::read_to_string(entry.path().join("stat"))
            && let Some(state) = parse_proc_stat_state(&stat)
        {
            match state {
                'R' => running += 1,
                'D' => blocked += 1,
                _ => {}
            }
        }
        if let Some(rss_bytes) = read_process_rss_bytes(&entry.path()) {
            let comm = read_trimmed(entry.path().join("comm")).unwrap_or_else(|_| unavailable());
            if top_rss
                .as_ref()
                .is_none_or(|(_, _, previous)| rss_bytes > *previous)
            {
                top_rss = Some((pid, comm, rss_bytes));
            }
        }
    }
    ProcessSummary {
        total: total.to_string(),
        running: running.to_string(),
        blocked: blocked.to_string(),
        top_cpu: top_cpu.unwrap_or_else(|| "measuring".to_string()),
        top_rss: top_rss
            .map(|(pid, comm, rss)| {
                format!(
                    "{} pid {} - cpu unavailable - rss {}",
                    display_comm(&comm),
                    pid,
                    format_bytes_binary(rss)
                )
            })
            .unwrap_or_else(unavailable),
    }
}

fn parse_proc_stat_state(stat: &str) -> Option<char> {
    let end = stat.rfind(')')?;
    stat[end + 1..].split_whitespace().next()?.chars().next()
}

fn read_process_cpu_samples() -> Option<BTreeMap<u32, ProcessCpu>> {
    let entries = fs::read_dir("/proc").ok()?;
    let mut out = BTreeMap::new();
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let proc_path = entry.path();
        let Ok(stat) = fs::read_to_string(proc_path.join("stat")) else {
            continue;
        };
        let Some(ticks) = parse_proc_stat_ticks(&stat) else {
            continue;
        };
        let comm = read_trimmed(proc_path.join("comm")).unwrap_or_else(|_| unavailable());
        let rss_bytes = read_process_rss_bytes(&proc_path).unwrap_or(0);
        out.insert(
            pid,
            ProcessCpu {
                comm,
                ticks,
                rss_bytes,
            },
        );
    }
    Some(out)
}

fn parse_proc_stat_ticks(stat: &str) -> Option<u64> {
    let end = stat.rfind(')')?;
    let fields = stat[end + 1..].split_whitespace().collect::<Vec<_>>();
    let utime = fields.get(11)?.parse::<u64>().ok()?;
    let stime = fields.get(12)?.parse::<u64>().ok()?;
    Some(utime.saturating_add(stime))
}

fn read_process_rss_bytes(proc_path: &Path) -> Option<u64> {
    let statm = fs::read_to_string(proc_path.join("statm")).ok()?;
    let rss_pages = statm.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return None;
    }
    Some(rss_pages.saturating_mul(page_size as u64))
}

fn display_comm(comm: &str) -> &str {
    if comm.trim().is_empty() {
        "unavailable"
    } else {
        comm.trim()
    }
}

fn read_service_summary(root_expansion: &str) -> ServiceSummary {
    ServiceSummary {
        grow_root: root_expansion.to_string(),
        ssh: process_state_for_names(&["sshd"]),
        ssh_listening: tcp_22_listening_state(),
        networkd: process_state_for_names(&["systemd-network"]),
        dashboard: "running".to_string(),
        root_shell_state: process_state_for_name_on_tty("bash", "/dev/tty2"),
    }
}

fn tcp_22_listening_state() -> String {
    let tcp4 = fs::read_to_string("/proc/net/tcp").ok();
    let tcp6 = fs::read_to_string("/proc/net/tcp6").ok();
    tcp_22_listening_from_proc_tables(tcp4.as_deref(), tcp6.as_deref()).to_string()
}

fn percent_state(value: &str, warning: f64, critical: f64) -> String {
    let Some(value) = percent_value(value) else {
        return "unavailable".to_string();
    };
    if value >= critical {
        "critical"
    } else if value >= warning {
        "warning"
    } else {
        ""
    }
    .to_string()
}

fn thermal_state(value: &str) -> String {
    let value = value.trim_end_matches(" C").parse::<f64>().ok();
    match value {
        Some(v) if v >= 85.0 => "critical",
        Some(v) if v >= 70.0 => "warning",
        Some(_) => "",
        None => "unavailable",
    }
    .to_string()
}

fn network_error_state(rx_err: &str, tx_err: &str, rx_drop: &str, tx_drop: &str) -> String {
    let samples = [rx_err, tx_err, rx_drop, tx_drop];
    if samples.iter().any(|value| is_delta_pending(value)) {
        return String::new();
    }
    let values = samples
        .into_iter()
        .filter_map(rate_number)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return "unavailable".to_string();
    }
    if values.iter().any(|v| *v > 10.0) {
        "critical"
    } else if values.iter().any(|v| *v > 0.0) {
        "warning"
    } else {
        ""
    }
    .to_string()
}

fn is_delta_pending(value: &str) -> bool {
    matches!(value, "measuring" | "configuring")
}

fn rate_number(value: &str) -> Option<f64> {
    value
        .split_whitespace()
        .next()
        .and_then(|number| number.parse::<f64>().ok())
}

fn disk_link_summary(rows: &[DashboardDiskRow]) -> String {
    let mut nvme = 0usize;
    let mut sata = 0usize;
    let mut virtio = 0usize;
    let mut usb = 0usize;
    let mut mmc = 0usize;
    let mut other = 0usize;
    for row in rows {
        match row.transport.as_str() {
            "nvme" => nvme += 1,
            "sata/scsi" => sata += 1,
            "virtio" => virtio += 1,
            "usb" => usb += 1,
            "mmc" => mmc += 1,
            _ => other += 1,
        }
    }
    format!(
        "nvme {nvme} - sata/scsi {sata} - virtio {virtio} - usb {usb} - mmc {mmc} - other {other}"
    )
}

fn process_state_for_names(names: &[&str]) -> String {
    let Ok(entries) = fs::read_dir("/proc") else {
        return unavailable();
    };
    for entry in entries.flatten() {
        let pid = entry.file_name().to_string_lossy().to_string();
        if !pid.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        if let Ok(comm) = read_trimmed(entry.path().join("comm"))
            && names.iter().any(|name| comm == *name)
        {
            return "running".to_string();
        }
    }
    "not-running".to_string()
}

fn process_state_for_name_on_tty(name: &str, tty_path: &str) -> String {
    let Ok(tty_meta) = fs::metadata(tty_path) else {
        return unavailable();
    };
    let tty_rdev = tty_meta.rdev() as i64;
    let Ok(entries) = fs::read_dir("/proc") else {
        return unavailable();
    };
    for entry in entries.flatten() {
        let pid = entry.file_name().to_string_lossy().to_string();
        if !pid.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let proc_path = entry.path();
        let Ok(comm) = read_trimmed(proc_path.join("comm")) else {
            continue;
        };
        if comm != name {
            continue;
        }
        let Ok(stat) = fs::read_to_string(proc_path.join("stat")) else {
            continue;
        };
        if parse_proc_stat_tty_nr(&stat) == Some(tty_rdev) {
            return "running".to_string();
        }
    }
    "not-running".to_string()
}

fn parse_proc_stat_tty_nr(stat: &str) -> Option<i64> {
    let end = stat.rfind(')')?;
    stat[end + 1..].split_whitespace().nth(4)?.parse().ok()
}

fn read_pressure_summary() -> PressureSummary {
    PressureSummary {
        cpu_some: pressure_avg10("/proc/pressure/cpu", "some").unwrap_or_else(unavailable),
        memory_some: pressure_avg10("/proc/pressure/memory", "some").unwrap_or_else(unavailable),
        memory_full: pressure_avg10("/proc/pressure/memory", "full").unwrap_or_else(unavailable),
        io_some: pressure_avg10("/proc/pressure/io", "some").unwrap_or_else(unavailable),
        io_full: pressure_avg10("/proc/pressure/io", "full").unwrap_or_else(unavailable),
    }
}

fn pressure_avg10(path: &str, label: &str) -> Option<String> {
    fs::read_to_string(path).ok()?.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()? == label).then(|| {
            fields.find_map(|field| {
                field
                    .strip_prefix("avg10=")
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(|value| format!("{value:.3}%"))
            })
        })?
    })
}

fn read_thermal_summary() -> ThermalSummary {
    let mut best: Option<(i64, String)> = None;
    collect_temperatures("/sys/class/thermal", "thermal_zone", "temp", &mut best);
    collect_temperatures("/sys/class/hwmon", "hwmon", "temp", &mut best);
    match best {
        Some((millidegrees, source)) => ThermalSummary {
            value: format!("{:.3} C", millidegrees as f64 / 1000.0),
            source,
        },
        None => ThermalSummary {
            value: unavailable(),
            source: unavailable(),
        },
    }
}

fn collect_temperatures(
    root: &str,
    dir_prefix: &str,
    file_prefix: &str,
    best: &mut Option<(i64, String)>,
) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let dirname = entry.file_name().to_string_lossy().to_string();
        if !dirname.starts_with(dir_prefix) {
            continue;
        }
        let Ok(files) = fs::read_dir(entry.path()) else {
            continue;
        };
        for file in files.flatten() {
            let filename = file.file_name().to_string_lossy().to_string();
            if !filename.starts_with(file_prefix)
                || !filename.ends_with("input") && filename != file_prefix
            {
                continue;
            }
            if let Ok(raw) = read_trimmed(file.path())
                && let Ok(value) = raw.parse::<i64>()
                && best.as_ref().is_none_or(|(current, _)| value > *current)
            {
                *best = Some((value, format!("{dirname}/{filename}")));
            }
        }
    }
}

fn read_kernel_alerts(kmsg: &mut Option<File>) -> KernelAlertSample {
    if kmsg.is_none() {
        let opened = match OpenOptions::new().read(true).open("/dev/kmsg") {
            Ok(file) => file,
            Err(_) => return KernelAlertSample::Unavailable,
        };
        let flags = unsafe { libc::fcntl(opened.as_raw_fd(), libc::F_GETFL) };
        if flags >= 0 {
            let _ =
                unsafe { libc::fcntl(opened.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
        }
        *kmsg = Some(opened);
    }
    let Some(file) = kmsg.as_mut() else {
        return KernelAlertSample::Unavailable;
    };
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => {
                *kmsg = None;
                return KernelAlertSample::Unavailable;
            }
        }
        if bytes.len() > 256 * 1024 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut priorities = Vec::new();
    for line in text.lines() {
        let Some((prefix, _message)) = line.split_once(';') else {
            continue;
        };
        let Some(priority) = prefix
            .split(',')
            .next()
            .and_then(|raw| raw.parse::<u8>().ok())
        else {
            continue;
        };
        let priority = match priority {
            0..=3 => KernelAlertPriority::Error,
            4 => KernelAlertPriority::Warning,
            _ => continue,
        };
        priorities.push(priority);
    }
    KernelAlertSample::Priorities(priorities)
}

fn retained_kernel_alert_summary(
    priorities: &VecDeque<KernelAlertPriority>,
    unavailable: bool,
) -> KernelAlertSummary {
    if unavailable {
        return KernelAlertSummary {
            state: "unavailable".to_string(),
            warning_count: "unavailable".to_string(),
            error_count: "unavailable".to_string(),
        };
    }
    let warning_count = priorities
        .iter()
        .filter(|priority| **priority == KernelAlertPriority::Warning)
        .count();
    let error_count = priorities
        .iter()
        .filter(|priority| **priority == KernelAlertPriority::Error)
        .count();
    let state = if error_count > 0 {
        "error"
    } else if warning_count > 0 {
        "warning"
    } else {
        "none"
    };
    KernelAlertSummary {
        state: state.to_string(),
        warning_count: warning_count.to_string(),
        error_count: error_count.to_string(),
    }
}

