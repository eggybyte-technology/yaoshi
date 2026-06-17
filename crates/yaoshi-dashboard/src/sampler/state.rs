
#[derive(Default)]
pub(crate) struct Sampler {
    cpu: Option<(Instant, CpuTimes)>,
    disk: Option<(Instant, BTreeMap<String, DiskIo>)>,
    network: Option<(Instant, BTreeMap<String, NetCounters>)>,
    process_cpu: Option<(u64, BTreeMap<u32, ProcessCpu>)>,
    static_sample: Option<(Instant, StaticSample)>,
    kernel_alerts: VecDeque<KernelAlertPriority>,
    kernel_alerts_unavailable: bool,
    kmsg: Option<File>,
    core_rows: Vec<DashboardCoreRow>,
}

impl Sampler {
    pub(crate) fn sample(&mut self, display: NegotiatedDisplay) -> DashboardSnapshot {
        let static_sample = self.static_sample();
        let uptime = read_uptime();
        let cpu_used = self.cpu_usage();
        let (load1, load5, load15) = read_loadavg();
        let memory = read_memory();
        let process = self.process_summary(&cpu_used);
        let disks = self.disks();
        let network = self.network();
        let prepare = prepare_state();
        let root_expansion = root_expansion_state();
        let services = read_service_summary(&root_expansion);
        let pressure = read_pressure_summary();
        let thermal = read_thermal_summary();
        let access = read_access_summary();
        self.sample_kernel_alerts();
        let kernel_alerts =
            retained_kernel_alert_summary(&self.kernel_alerts, self.kernel_alerts_unavailable);
        let ssh_ready_state = ssh_ready_state(
            &access.ssh_root_key_state,
            &services.ssh,
            &services.ssh_listening,
        )
        .to_string();
        let cpu_state = percent_state(&cpu_used, 85.0, 95.0);
        let memory_state = percent_state(&memory.percent, 80.0, 90.0);
        let root_state = percent_state(&disks.root_percent, 80.0, 90.0);
        let inode_state = percent_state(&disks.root_inode_percent, 80.0, 90.0);
        let thermal_state = thermal_state(&thermal.value);
        let network_error_state = network_error_state(
            &network.rx_error_rate,
            &network.tx_error_rate,
            &network.rx_drop_rate,
            &network.tx_drop_rate,
        );
        let disk_link_summary = disk_link_summary(&disks.rows);

        DashboardSnapshot {
            display,
            hostname: static_sample.hostname,
            uptime,
            kernel_release: read_kernel_release(),
            system_vendor: static_sample.system_vendor,
            product_name: static_sample.product_name,
            board_vendor: static_sample.board_vendor,
            board_name: static_sample.board_name,
            firmware_vendor: static_sample.firmware_vendor,
            firmware_version: static_sample.firmware_version,
            prepare_state: prepare,
            root_expansion_state: root_expansion.clone(),
            root_partuuid: root_partuuid(),
            root_label: unavailable(),
            ssh_ready_state,
            ssh_root_key_state: access.ssh_root_key_state,
            authorized_key_count: access.authorized_key_count,
            ssh_listening_state: services.ssh_listening.clone(),
            ssh_process_state: services.ssh.clone(),
            networkd_process_state: services.networkd.clone(),
            dashboard_process_state: services.dashboard.clone(),
            root_shell_state: services.root_shell_state.clone(),
            rescue_shell_state: services.root_shell_state.clone(),
            cpu_model: static_sample.cpu_model,
            logical_cpu_count: static_sample.logical_cpu_count,
            cpu_used,
            cpu_state,
            load1,
            load5,
            load15,
            process_count: process.total,
            running_process_count: process.running,
            blocked_process_count: process.blocked,
            top_cpu_process: process.top_cpu,
            memory_used: memory.used,
            memory_total: memory.total,
            memory_dimm_summary: static_sample.memory_dimm_summary,
            memory_used_percent: memory.percent,
            memory_state,
            memory_available: memory.available,
            memory_cache: memory.cache,
            memory_dirty: memory.dirty,
            swap_used: memory.swap_used,
            swap_total: memory.swap_total,
            swap_used_percent: memory.swap_percent,
            top_rss_process: process.top_rss,
            cpu_pressure: pressure.cpu_some,
            memory_pressure: pressure.memory_some,
            memory_full_pressure: pressure.memory_full,
            io_pressure: pressure.io_some,
            io_full_pressure: pressure.io_full,
            thermal: thermal.value,
            thermal_state,
            thermal_source: thermal.source,
            disk_count: disks.count,
            root_used: disks.root_used,
            root_total: disks.root_total,
            root_used_percent: disks.root_percent,
            root_state,
            root_inode_usage: disks.root_inode_usage,
            root_inode_percent: disks.root_inode_percent,
            inode_state,
            disk_read_rate: disks.read_rate,
            disk_write_rate: disks.write_rate,
            disk_read_iops: disks.read_iops,
            disk_write_iops: disks.write_iops,
            core_rows: self.core_rows.clone(),
            disk_rows: disks.rows,
            filesystem_rows: disks.filesystem_rows,
            network_interface_count: network.count,
            ssh_access_address: network.ssh_access_address,
            ssh_access_summary: network.ssh_access_summary,
            route_iface: network.route_iface,
            route_address: network.route_address,
            default_gateway: network.default_gateway,
            route_metric: network.route_metric,
            network_state: network.state,
            network_rx_rate: network.rx_rate,
            network_tx_rate: network.tx_rate,
            network_rx_packet_rate: network.rx_packet_rate,
            network_tx_packet_rate: network.tx_packet_rate,
            network_rx_error_rate: network.rx_error_rate,
            network_tx_error_rate: network.tx_error_rate,
            network_rx_drop_rate: network.rx_drop_rate,
            network_tx_drop_rate: network.tx_drop_rate,
            network_error_state,
            interface_attention_count: network.interface_attention_count,
            network_rows: network.rows,
            route_rows: network.routes,
            grow_root_service: services.grow_root,
            ssh_service: services.ssh,
            networkd_service: services.networkd,
            dashboard_service: services.dashboard,
            root_shell_service: services.root_shell_state,
            kernel_alert_state: kernel_alerts.state,
            kernel_warning_count: kernel_alerts.warning_count,
            kernel_error_count: kernel_alerts.error_count,
            disk_link_summary,
        }
    }

    fn static_sample(&mut self) -> StaticSample {
        let now = Instant::now();
        if let Some((then, sample)) = &self.static_sample
            && now.duration_since(*then) < Duration::from_secs(10)
        {
            return sample.clone();
        }
        let sample = StaticSample {
            hostname: read_hostname(),
            cpu_model: read_cpu_model(),
            logical_cpu_count: read_cpu_count(),
            system_vendor: read_dmi_field("sys_vendor"),
            product_name: read_dmi_field("product_name"),
            board_vendor: read_dmi_field("board_vendor"),
            board_name: read_dmi_field("board_name"),
            firmware_vendor: read_dmi_field("bios_vendor"),
            firmware_version: read_dmi_field("bios_version"),
            memory_dimm_summary: smbios_memory_summary_from_sysfs(
                Path::new("/sys/firmware/dmi/tables/smbios_entry_point"),
                Path::new("/sys/firmware/dmi/tables/DMI"),
            ),
        };
        self.static_sample = Some((now, sample.clone()));
        sample
    }

    fn cpu_usage(&mut self) -> String {
        let Some(now) = read_cpu_times() else {
            self.cpu = None;
            self.core_rows.clear();
            return "unavailable".to_string();
        };
        let now_instant = Instant::now();
        let rendered = match &self.cpu {
            Some((_, previous)) => {
                let total_delta = now.total.saturating_sub(previous.total);
                let idle_delta = now.idle.saturating_sub(previous.idle);
                self.core_rows = render_core_rows(&previous.cores, &now.cores);
                if total_delta == 0 {
                    "unavailable".to_string()
                } else {
                    let used = total_delta.saturating_sub(idle_delta);
                    format_percent_3(used, total_delta)
                }
            }
            None => {
                self.core_rows = now
                    .cores
                    .iter()
                    .enumerate()
                    .map(|(index, _)| DashboardCoreRow {
                        index,
                        used_percent: "measuring".to_string(),
                    })
                    .collect();
                "measuring".to_string()
            }
        };
        self.cpu = Some((now_instant, now));
        rendered
    }

    fn sample_kernel_alerts(&mut self) {
        match read_kernel_alerts(&mut self.kmsg) {
            KernelAlertSample::Unavailable => {
                self.kernel_alerts.clear();
                self.kernel_alerts_unavailable = true;
            }
            KernelAlertSample::Priorities(priorities) => {
                self.kernel_alerts_unavailable = false;
                for priority in priorities {
                    self.kernel_alerts.push_back(priority);
                    while self.kernel_alerts.len() > 24 {
                        self.kernel_alerts.pop_front();
                    }
                }
            }
        }
    }

    fn disks(&mut self) -> DiskSummary {
        let Ok(disks) = read_disks() else {
            self.disk = None;
            return DiskSummary::unavailable();
        };
        let root_disk = root_backing_disk(&disks);
        let root_usage = statvfs_usage(Path::new("/"));
        let root_inode_usage = statvfs_inode_usage(Path::new("/"));
        let disk_ios = read_disk_io();
        let now = Instant::now();
        let (read_rate, write_rate, read_iops, write_iops) = match (&self.disk, &disk_ios) {
            (Some((then, previous)), Some(current)) => {
                let elapsed = now.duration_since(*then).as_secs_f64();
                if elapsed <= 0.0 {
                    (
                        "unavailable".to_string(),
                        "unavailable".to_string(),
                        "unavailable".to_string(),
                        "unavailable".to_string(),
                    )
                } else {
                    let mut read_delta = 0u64;
                    let mut write_delta = 0u64;
                    let mut read_io_delta = 0u64;
                    let mut write_io_delta = 0u64;
                    for disk in &disks {
                        if let (Some(prev), Some(curr)) =
                            (previous.get(&disk.name), current.get(&disk.name))
                        {
                            read_delta = read_delta
                                .saturating_add(curr.read_bytes.saturating_sub(prev.read_bytes));
                            write_delta = write_delta
                                .saturating_add(curr.write_bytes.saturating_sub(prev.write_bytes));
                            read_io_delta = read_io_delta
                                .saturating_add(curr.read_ios.saturating_sub(prev.read_ios));
                            write_io_delta = write_io_delta
                                .saturating_add(curr.write_ios.saturating_sub(prev.write_ios));
                        }
                    }
                    (
                        format_rate((read_delta as f64 / elapsed) as u64),
                        format_rate((write_delta as f64 / elapsed) as u64),
                        format!("{:.3}", read_io_delta as f64 / elapsed),
                        format!("{:.3}", write_io_delta as f64 / elapsed),
                    )
                }
            }
            (None, Some(_)) => (
                "measuring".to_string(),
                "measuring".to_string(),
                "measuring".to_string(),
                "measuring".to_string(),
            ),
            _ => (
                "unavailable".to_string(),
                "unavailable".to_string(),
                "unavailable".to_string(),
                "unavailable".to_string(),
            ),
        };
        if let Some(current) = disk_ios {
            self.disk = Some((now, current));
        } else {
            self.disk = None;
        }

        let mounts = read_mounts().unwrap_or_default();
        let filesystem_rows = required_filesystem_rows(&mounts);
        let mut rows = disks
            .iter()
            .map(|disk| DashboardDiskRow {
                is_root: root_disk.as_deref() == Some(disk.name.as_str()),
                dev: format!("/dev/{}", disk.name),
                size: format_bytes_binary(disk.size_bytes),
                transport: disk.transport.clone(),
                vendor: disk.vendor.clone(),
                model: disk.model.clone(),
                display: disk_display(&disk.vendor, &disk.model),
                serial: disk.serial.clone(),
                partitioned: format_bytes_binary(disk.partitioned_bytes),
                fs: filesystem_summary_for_disk(disk, &mounts),
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.is_root.cmp(&a.is_root).then_with(|| a.dev.cmp(&b.dev)));
        let (root_used, root_total, root_percent) = usage_strings(root_usage);
        let (root_inode_usage, root_inode_percent) = inode_usage_strings(root_inode_usage);

        DiskSummary {
            count: disks.len().to_string(),
            root_used,
            root_total,
            root_percent,
            root_inode_usage,
            root_inode_percent,
            read_rate,
            write_rate,
            read_iops,
            write_iops,
            filesystem_rows,
            rows: if rows.is_empty() {
                vec![DashboardDiskRow {
                    is_root: false,
                    dev: "unavailable".to_string(),
                    size: "unavailable".to_string(),
                    transport: "unavailable".to_string(),
                    vendor: "unavailable".to_string(),
                    model: "unavailable".to_string(),
                    display: "unavailable".to_string(),
                    serial: "unavailable".to_string(),
                    partitioned: "unavailable".to_string(),
                    fs: "unavailable".to_string(),
                }]
            } else {
                rows
            },
        }
    }

    fn network(&mut self) -> NetworkSummary {
        let Ok(ifaces) = read_interfaces() else {
            self.network = None;
            return NetworkSummary::unavailable();
        };
        let now = Instant::now();
        let current = ifaces
            .iter()
            .map(|iface| {
                (
                    iface.name.clone(),
                    NetCounters {
                        rx_bytes: iface.rx_bytes,
                        tx_bytes: iface.tx_bytes,
                        rx_packets: iface.rx_packets,
                        tx_packets: iface.tx_packets,
                        rx_errors: iface.rx_errors,
                        tx_errors: iface.tx_errors,
                        rx_drops: iface.rx_drops,
                        tx_drops: iface.tx_drops,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let rate_for = |name: &str,
                        field: fn(&NetCounters) -> u64,
                        previous: &Option<(Instant, BTreeMap<String, NetCounters>)>|
         -> String {
            let Some((then, previous)) = previous else {
                return "measuring".to_string();
            };
            let elapsed = now.duration_since(*then).as_secs_f64();
            let Some(prev) = previous.get(name) else {
                return "measuring".to_string();
            };
            let Some(curr) = current.get(name) else {
                return "unavailable".to_string();
            };
            if elapsed <= 0.0 {
                "unavailable".to_string()
            } else {
                format_rate((field(curr).saturating_sub(field(prev)) as f64 / elapsed) as u64)
            }
        };
        let decimal_rate_for = |name: &str,
                                field: fn(&NetCounters) -> u64,
                                unit: &str,
                                previous: &Option<(Instant, BTreeMap<String, NetCounters>)>|
         -> String {
            let Some((then, previous)) = previous else {
                return "measuring".to_string();
            };
            let elapsed = now.duration_since(*then).as_secs_f64();
            let Some(prev) = previous.get(name) else {
                return "measuring".to_string();
            };
            let Some(curr) = current.get(name) else {
                return "unavailable".to_string();
            };
            if elapsed <= 0.0 {
                "unavailable".to_string()
            } else {
                format_decimal_rate_unit(
                    field(curr).saturating_sub(field(prev)) as f64 / elapsed,
                    unit,
                )
            }
        };
        let access_address = access_address(&ifaces);
        let default_route = selected_default_route(&ifaces);
        let route_iface = default_route
            .as_ref()
            .map(|route| route.iface.clone())
            .unwrap_or_else(unavailable);
        let route_address = default_route
            .as_ref()
            .and_then(|route| route_address_for(&route.iface, route.family, &ifaces))
            .unwrap_or_else(unavailable);
        let default_gateway = default_route
            .as_ref()
            .map(|route| {
                route
                    .gateway
                    .map(|gateway| gateway.to_string())
                    .unwrap_or_else(|| "direct".to_string())
            })
            .unwrap_or_else(unavailable);
        let route_metric = default_route
            .as_ref()
            .and_then(|route| route.metric)
            .map(|metric| metric.to_string())
            .unwrap_or_else(unavailable);
        let access_summary = access_summary(&ifaces, &access_address);
        let mut rows = ifaces
            .iter()
            .map(|iface| {
                let role = interface_role(&iface.name, &route_iface, &access_summary);
                DashboardNetworkRow {
                    is_route: iface.name == route_iface,
                    is_access: role == "access" || role == "route+access",
                    iface: iface.name.clone(),
                    role,
                    kind: iface.kind.clone(),
                    state: iface.state.clone(),
                    addresses: iface.address_summary.clone(),
                    mac: iface.mac.clone(),
                    mtu: iface.mtu.clone(),
                    speed: iface.speed.clone(),
                    rx_rate: rate_for(&iface.name, |c| c.rx_bytes, &self.network),
                    tx_rate: rate_for(&iface.name, |c| c.tx_bytes, &self.network),
                    rx_drop_rate: decimal_rate_for(
                        &iface.name,
                        |c| c.rx_drops,
                        "drops",
                        &self.network,
                    ),
                    tx_drop_rate: decimal_rate_for(
                        &iface.name,
                        |c| c.tx_drops,
                        "drops",
                        &self.network,
                    ),
                    alert_state: interface_alert_state(
                        &decimal_rate_for(&iface.name, |c| c.rx_errors, "errors", &self.network),
                        &decimal_rate_for(&iface.name, |c| c.tx_errors, "errors", &self.network),
                        &decimal_rate_for(&iface.name, |c| c.rx_drops, "drops", &self.network),
                        &decimal_rate_for(&iface.name, |c| c.tx_drops, "drops", &self.network),
                    ),
                }
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            b.is_route
                .cmp(&a.is_route)
                .then_with(|| b.is_access.cmp(&a.is_access))
                .then_with(|| a.iface.cmp(&b.iface))
        });
        let (
            rx_rate,
            tx_rate,
            rx_packet_rate,
            tx_packet_rate,
            rx_error_rate,
            tx_error_rate,
            rx_drop_rate,
            tx_drop_rate,
        ) = match &self.network {
            Some((then, previous)) => {
                let elapsed = now.duration_since(*then).as_secs_f64();
                if elapsed <= 0.0 {
                    unavailable_rate_tuple8()
                } else {
                    let mut rx_delta = 0u64;
                    let mut tx_delta = 0u64;
                    let mut rx_packet_delta = 0u64;
                    let mut tx_packet_delta = 0u64;
                    let mut rx_error_delta = 0u64;
                    let mut tx_error_delta = 0u64;
                    let mut rx_drop_delta = 0u64;
                    let mut tx_drop_delta = 0u64;
                    for iface in &ifaces {
                        if let (Some(prev), Some(curr)) =
                            (previous.get(&iface.name), current.get(&iface.name))
                        {
                            rx_delta = rx_delta
                                .saturating_add(curr.rx_bytes.saturating_sub(prev.rx_bytes));
                            tx_delta = tx_delta
                                .saturating_add(curr.tx_bytes.saturating_sub(prev.tx_bytes));
                            rx_packet_delta = rx_packet_delta
                                .saturating_add(curr.rx_packets.saturating_sub(prev.rx_packets));
                            tx_packet_delta = tx_packet_delta
                                .saturating_add(curr.tx_packets.saturating_sub(prev.tx_packets));
                            rx_error_delta = rx_error_delta
                                .saturating_add(curr.rx_errors.saturating_sub(prev.rx_errors));
                            tx_error_delta = tx_error_delta
                                .saturating_add(curr.tx_errors.saturating_sub(prev.tx_errors));
                            rx_drop_delta = rx_drop_delta
                                .saturating_add(curr.rx_drops.saturating_sub(prev.rx_drops));
                            tx_drop_delta = tx_drop_delta
                                .saturating_add(curr.tx_drops.saturating_sub(prev.tx_drops));
                        }
                    }
                    (
                        format_rate((rx_delta as f64 / elapsed) as u64),
                        format_rate((tx_delta as f64 / elapsed) as u64),
                        format_decimal_rate_unit(rx_packet_delta as f64 / elapsed, "packets"),
                        format_decimal_rate_unit(tx_packet_delta as f64 / elapsed, "packets"),
                        format_decimal_rate_unit(rx_error_delta as f64 / elapsed, "errors"),
                        format_decimal_rate_unit(tx_error_delta as f64 / elapsed, "errors"),
                        format_decimal_rate_unit(rx_drop_delta as f64 / elapsed, "drops"),
                        format_decimal_rate_unit(tx_drop_delta as f64 / elapsed, "drops"),
                    )
                }
            }
            None => measuring_rate_tuple8(),
        };
        self.network = Some((now, current));
        let routes = route_rows_from_selected(&default_route, &route_address);
        let state = if default_route.is_some()
            && !matches!(access_address.as_str(), "configuring" | "unavailable")
            && (!matches!(route_address.as_str(), "unavailable")
                || !matches!(default_gateway.as_str(), "unavailable"))
        {
            "ready".to_string()
        } else {
            "configuring".to_string()
        };
        let interface_attention_count = rows
            .iter()
            .filter(|row| matches!(row.alert_state.as_str(), "warning" | "critical"))
            .count()
            + rows
                .iter()
                .filter(|row| {
                    row.is_route
                        && matches!(
                            row.state.as_str(),
                            "down" | "lowerlayerdown" | "dormant" | "notpresent"
                        )
                })
                .count();

        NetworkSummary {
            count: ifaces.len().to_string(),
            ssh_access_address: access_address,
            ssh_access_summary: access_summary,
            route_iface,
            route_address,
            default_gateway,
            route_metric,
            interface_attention_count: interface_attention_count.to_string(),
            state,
            rx_rate,
            tx_rate,
            rx_packet_rate,
            tx_packet_rate,
            rx_error_rate,
            tx_error_rate,
            rx_drop_rate,
            tx_drop_rate,
            rows: if rows.is_empty() {
                vec![DashboardNetworkRow {
                    is_route: false,
                    is_access: false,
                    iface: "unavailable".to_string(),
                    role: "iface".to_string(),
                    kind: "other".to_string(),
                    state: "unavailable".to_string(),
                    addresses: "unavailable".to_string(),
                    mac: "unavailable".to_string(),
                    mtu: "unavailable".to_string(),
                    speed: "unavailable".to_string(),
                    rx_rate: "measuring".to_string(),
                    tx_rate: "measuring".to_string(),
                    rx_drop_rate: "measuring".to_string(),
                    tx_drop_rate: "measuring".to_string(),
                    alert_state: String::new(),
                }]
            } else {
                rows
            },
            routes,
        }
    }

    fn process_summary(&mut self, aggregate_cpu_used: &str) -> ProcessSummary {
        let Some(total_ticks) = read_cpu_times().map(|times| times.total) else {
            self.process_cpu = None;
            return read_process_summary(None);
        };
        let processes = read_process_cpu_samples();
        let top_cpu = match (&self.process_cpu, &processes) {
            (Some((previous_total, previous)), Some(current)) => {
                let total_delta = total_ticks.saturating_sub(*previous_total);
                if total_delta == 0 {
                    "unavailable".to_string()
                } else {
                    current
                        .iter()
                        .filter_map(|(pid, sample)| {
                            let prev = previous.get(pid)?;
                            let delta = sample.ticks.saturating_sub(prev.ticks);
                            Some((pid, sample, delta))
                        })
                        .max_by(|(pid_a, sample_a, delta_a), (pid_b, sample_b, delta_b)| {
                            delta_a
                                .cmp(delta_b)
                                .then_with(|| sample_a.rss_bytes.cmp(&sample_b.rss_bytes))
                                .then_with(|| pid_b.cmp(pid_a))
                        })
                        .map(|(pid, sample, delta)| {
                            format!(
                                "{} pid {} - cpu {} - rss {}",
                                display_comm(&sample.comm),
                                pid,
                                yaoshi_common::format_percent(
                                    delta as f64 * 100.0 / total_delta as f64,
                                    false
                                ),
                                format_bytes_binary(sample.rss_bytes)
                            )
                        })
                        .unwrap_or_else(|| "measuring".to_string())
                }
            }
            (None, Some(_)) => "measuring".to_string(),
            _ => "unavailable".to_string(),
        };
        let visible_top_cpu = top_process_required(aggregate_cpu_used, &top_cpu).then_some(top_cpu);
        let summary = read_process_summary(visible_top_cpu);
        if let Some(processes) = processes {
            self.process_cpu = Some((total_ticks, processes));
        } else {
            self.process_cpu = None;
        }
        summary
    }
}

fn percent_value(text: &str) -> Option<f64> {
    text.strip_suffix('%')?.parse().ok()
}

fn top_process_required(aggregate_cpu_used: &str, top_cpu_process: &str) -> bool {
    percent_value(aggregate_cpu_used).is_some_and(|value| value >= 85.0)
        || top_process_cpu_percent(top_cpu_process).is_some_and(|value| value >= 25.0)
}

fn top_process_cpu_percent(top_cpu_process: &str) -> Option<f64> {
    top_cpu_process
        .split_once("cpu ")?
        .1
        .split_whitespace()
        .next()
        .and_then(percent_value)
}

fn percent_for_sort(text: &str) -> f64 {
    percent_value(text).unwrap_or(-1.0)
}

#[derive(Clone)]
struct CpuTimes {
    idle: u64,
    total: u64,
    cores: Vec<CoreTimes>,
}

#[derive(Clone, Copy)]
struct CoreTimes {
    idle: u64,
    total: u64,
}

struct MemorySummary {
    used: String,
    total: String,
    percent: String,
    available: String,
    cache: String,
    dirty: String,
    swap_used: String,
    swap_total: String,
    swap_percent: String,
}

struct DiskSummary {
    count: String,
    root_used: String,
    root_total: String,
    root_percent: String,
    root_inode_usage: String,
    root_inode_percent: String,
    read_rate: String,
    write_rate: String,
    read_iops: String,
    write_iops: String,
    filesystem_rows: Vec<String>,
    rows: Vec<DashboardDiskRow>,
}

impl DiskSummary {
    fn unavailable() -> Self {
        Self {
            count: "unavailable".to_string(),
            root_used: "unavailable".to_string(),
            root_total: "unavailable".to_string(),
            root_percent: "unavailable".to_string(),
            root_inode_usage: "unavailable".to_string(),
            root_inode_percent: "unavailable".to_string(),
            read_rate: "unavailable".to_string(),
            write_rate: "unavailable".to_string(),
            read_iops: "unavailable".to_string(),
            write_iops: "unavailable".to_string(),
            filesystem_rows: required_filesystem_rows(&[]),
            rows: vec![DashboardDiskRow {
                is_root: false,
                dev: "unavailable".to_string(),
                size: "unavailable".to_string(),
                transport: "unavailable".to_string(),
                vendor: "unavailable".to_string(),
                model: "unavailable".to_string(),
                display: "unavailable".to_string(),
                serial: "unavailable".to_string(),
                partitioned: "unavailable".to_string(),
                fs: "unavailable".to_string(),
            }],
        }
    }
}

struct NetworkSummary {
    count: String,
    ssh_access_address: String,
    ssh_access_summary: String,
    route_iface: String,
    route_address: String,
    default_gateway: String,
    route_metric: String,
    interface_attention_count: String,
    state: String,
    rx_rate: String,
    tx_rate: String,
    rx_packet_rate: String,
    tx_packet_rate: String,
    rx_error_rate: String,
    tx_error_rate: String,
    rx_drop_rate: String,
    tx_drop_rate: String,
    rows: Vec<DashboardNetworkRow>,
    routes: Vec<DashboardRouteRow>,
}

struct ProcessSummary {
    total: String,
    running: String,
    blocked: String,
    top_cpu: String,
    top_rss: String,
}

#[derive(Debug, Clone)]
struct ProcessCpu {
    comm: String,
    ticks: u64,
    rss_bytes: u64,
}

struct AccessSummary {
    ssh_root_key_state: String,
    authorized_key_count: String,
}

struct ServiceSummary {
    grow_root: String,
    ssh: String,
    ssh_listening: String,
    networkd: String,
    dashboard: String,
    root_shell_state: String,
}

struct PressureSummary {
    cpu_some: String,
    memory_some: String,
    memory_full: String,
    io_some: String,
    io_full: String,
}

struct ThermalSummary {
    value: String,
    source: String,
}

enum KernelAlertSample {
    Priorities(Vec<KernelAlertPriority>),
    Unavailable,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KernelAlertPriority {
    Warning,
    Error,
}

struct KernelAlertSummary {
    state: String,
    warning_count: String,
    error_count: String,
}

#[derive(Clone)]
struct StaticSample {
    hostname: String,
    cpu_model: String,
    logical_cpu_count: String,
    system_vendor: String,
    product_name: String,
    board_vendor: String,
    board_name: String,
    firmware_vendor: String,
    firmware_version: String,
    memory_dimm_summary: String,
}

impl NetworkSummary {
    fn unavailable() -> Self {
        Self {
            count: "unavailable".to_string(),
            ssh_access_address: "unavailable".to_string(),
            ssh_access_summary: "unavailable".to_string(),
            route_iface: "unavailable".to_string(),
            route_address: "unavailable".to_string(),
            default_gateway: "unavailable".to_string(),
            route_metric: "unavailable".to_string(),
            interface_attention_count: "unavailable".to_string(),
            state: "unavailable".to_string(),
            rx_rate: "unavailable".to_string(),
            tx_rate: "unavailable".to_string(),
            rx_packet_rate: "unavailable".to_string(),
            tx_packet_rate: "unavailable".to_string(),
            rx_error_rate: "unavailable".to_string(),
            tx_error_rate: "unavailable".to_string(),
            rx_drop_rate: "unavailable".to_string(),
            tx_drop_rate: "unavailable".to_string(),
            rows: vec![DashboardNetworkRow {
                is_route: false,
                is_access: false,
                iface: "unavailable".to_string(),
                role: "iface".to_string(),
                kind: "other".to_string(),
                state: "unavailable".to_string(),
                addresses: "unavailable".to_string(),
                mac: "unavailable".to_string(),
                mtu: "unavailable".to_string(),
                speed: "unavailable".to_string(),
                rx_rate: "unavailable".to_string(),
                tx_rate: "unavailable".to_string(),
                rx_drop_rate: "unavailable".to_string(),
                tx_drop_rate: "unavailable".to_string(),
                alert_state: "unavailable".to_string(),
            }],
            routes: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct DiskInfo {
    name: String,
    size_bytes: u64,
    partitioned_bytes: u64,
    transport: String,
    vendor: String,
    model: String,
    serial: String,
    device_refs: BTreeSet<String>,
}

#[derive(Clone, Copy)]
struct DiskIo {
    read_ios: u64,
    write_ios: u64,
    read_bytes: u64,
    write_bytes: u64,
}

#[derive(Debug, Clone)]
struct MountInfo {
    major_minor: String,
    mount_point: PathBuf,
    fs_type: String,
    source: String,
}

#[derive(Debug)]
struct InterfaceInfo {
    ifindex: u32,
    name: String,
    kind: String,
    state: String,
    address_summary: String,
    addresses: Vec<IpAddr>,
    addresses_unavailable: bool,
    mac: String,
    mtu: String,
    speed: String,
    rx_bytes: u64,
    tx_bytes: u64,
    rx_packets: u64,
    tx_packets: u64,
    rx_errors: u64,
    tx_errors: u64,
    rx_drops: u64,
    tx_drops: u64,
}

#[derive(Clone, Copy)]
struct NetCounters {
    rx_bytes: u64,
    tx_bytes: u64,
    rx_packets: u64,
    tx_packets: u64,
    rx_errors: u64,
    tx_errors: u64,
    rx_drops: u64,
    tx_drops: u64,
}

