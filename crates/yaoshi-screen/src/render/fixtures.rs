#[cfg(test)]
fn fixture_disk(name: &str, bytes: u64) -> TargetDiskCandidate {
    TargetDiskCandidate {
        disk: yaoshi_common::KernelDiskRef {
            sysfs_path: PathBuf::from(format!("/sys/block/{name}")),
            major_minor: "8:0".to_string(),
            kernel_name: name.to_string(),
            dev_path: PathBuf::from(format!("/dev/{name}")),
            logical_block_size: 512,
            byte_size: bytes,
            model: Some("Yaoshi Test Disk".to_string()),
            serial: Some(format!("SERIAL-{name}")),
            stable_disk_id: Some(PathBuf::from(format!(
                "/dev/disk/by-id/virtio-{name}-stable"
            ))),
        },
        status: CandidateStatus::Selectable,
        existing: ExistingPartitionTable::Gpt,
    }
}

#[cfg(test)]
fn fixture_dashboard() -> DashboardSnapshot {
    fixture_dashboard_inventory(1, 1, 1)
}

#[cfg(test)]
fn fixture_dashboard_inventory(
    logical_cpus: usize,
    disk_count: usize,
    interface_count: usize,
) -> DashboardSnapshot {
    let logical_cpus = logical_cpus.max(1);
    let disk_count = disk_count.max(1);
    let interface_count = interface_count.max(1);
    let core_rows = (0..logical_cpus)
        .map(|index| DashboardCoreRow {
            index,
            used_percent: if index == 0 {
                "12.000%".to_string()
            } else {
                format!("{:.3}%", 10.0 + (index % 70) as f64)
            },
        })
        .collect::<Vec<_>>();
    let disk_rows = (0..disk_count)
        .map(|index| DashboardDiskRow {
            is_root: index == 0,
            dev: if index == 0 {
                "/dev/vda".to_string()
            } else {
                format!("/dev/vd{index:03}")
            },
            size: if index == 0 {
                "5.000 GiB".to_string()
            } else {
                format!("{}.000 GiB", 5 + index)
            },
            transport: "virtio".to_string(),
            vendor: "Virtio".to_string(),
            serial: if index == 0 {
                "unavailable".to_string()
            } else {
                format!("DISK{index:03}")
            },
            partitioned: "5.000 GiB".to_string(),
            fs: if index == 0 {
                "1.000 GiB/4.000 GiB (30.000%)".to_string()
            } else {
                "not mounted".to_string()
            },
            model: if index == 0 {
                "Virtio Block".to_string()
            } else {
                format!("Virtio Block {index}")
            },
            display: if index == 0 {
                "Virtio Block".to_string()
            } else {
                format!("Virtio Block {index}")
            },
        })
        .collect::<Vec<_>>();
    let network_rows = (0..interface_count)
        .map(|index| DashboardNetworkRow {
            is_route: index == 0,
            is_access: index == 0,
            iface: if index == 0 {
                "eth0".to_string()
            } else {
                format!("eth{index}")
            },
            role: if index == 0 {
                "route+access".to_string()
            } else {
                "iface".to_string()
            },
            kind: "physical".to_string(),
            state: "up".to_string(),
            addresses: if index == 0 {
                "192.0.2.10".to_string()
            } else {
                format!("198.51.100.{}", index % 254 + 1)
            },
            mac: "unavailable".to_string(),
            mtu: "1500".to_string(),
            speed: "1000 Mb/s".to_string(),
            rx_rate: "measuring".to_string(),
            tx_rate: "measuring".to_string(),
            rx_drop_rate: "measuring".to_string(),
            tx_drop_rate: "measuring".to_string(),
            alert_state: String::new(),
        })
        .collect::<Vec<_>>();
    DashboardSnapshot {
        display: NegotiatedDisplay::fixture(Surface::Dashboard, 184, 52),
        hostname: "yaoshi".to_string(),
        uptime: "1m 2.000s".to_string(),
        kernel_release: "6.12.0-yaoshi".to_string(),
        system_vendor: "Yaoshi".to_string(),
        product_name: "Test Machine".to_string(),
        board_vendor: "Yaoshi".to_string(),
        board_name: "Test Board".to_string(),
        firmware_vendor: "Yaoshi".to_string(),
        firmware_version: "1.0.0".to_string(),
        prepare_state: "applied".to_string(),
        root_expansion_state: "expanded".to_string(),
        root_partuuid: "5c5c9f71-bc23-4f8d-80b2-6d4bb64a0f33".to_string(),
        root_label: "YAOSHI_ROOT".to_string(),
        ssh_ready_state: "ready".to_string(),
        ssh_root_key_state: "enabled".to_string(),
        authorized_key_count: "1".to_string(),
        ssh_listening_state: "listening".to_string(),
        ssh_process_state: "running".to_string(),
        networkd_process_state: "running".to_string(),
        dashboard_process_state: "running".to_string(),
        root_shell_state: "running".to_string(),
        rescue_shell_state: "running".to_string(),
        network_state: "ready".to_string(),
        kernel_alert_state: "none".to_string(),
        kernel_warning_count: "0".to_string(),
        kernel_error_count: "0".to_string(),
        ssh_access_address: "192.0.2.10".to_string(),
        ssh_access_summary: "eth0".to_string(),
        route_iface: "eth0".to_string(),
        route_address: "192.0.2.10".to_string(),
        default_gateway: "192.0.2.1".to_string(),
        route_metric: "100".to_string(),
        cpu_used: "12.000%".to_string(),
        cpu_model: "Yaoshi CPU".to_string(),
        logical_cpu_count: logical_cpus.to_string(),
        cpu_state: "".to_string(),
        load1: "0.100".to_string(),
        load5: "0.080".to_string(),
        load15: "0.040".to_string(),
        memory_used_percent: "20.000%".to_string(),
        memory_state: "".to_string(),
        memory_used: "512.000 MiB".to_string(),
        memory_total: "2.000 GiB".to_string(),
        memory_dimm_summary: "2 DIMMs - 2.000 GiB - Yaoshi Memory x2 - 3200 MT/s - ecc yes"
            .to_string(),
        root_used_percent: "30.000%".to_string(),
        root_state: "".to_string(),
        root_used: "1.000 GiB".to_string(),
        root_total: "4.000 GiB".to_string(),
        root_inode_percent: "4.000%".to_string(),
        inode_state: "".to_string(),
        root_inode_usage: "1000/25000".to_string(),
        thermal: "unavailable".to_string(),
        thermal_state: "unavailable".to_string(),
        thermal_source: "unavailable".to_string(),
        cpu_pressure: "0.000%".to_string(),
        memory_pressure: "0.000%".to_string(),
        io_pressure: "0.000%".to_string(),
        top_cpu_process: "unavailable".to_string(),
        process_count: "1".to_string(),
        running_process_count: "1".to_string(),
        blocked_process_count: "0".to_string(),
        memory_available: "1.500 GiB".to_string(),
        memory_cache: "128.000 MiB".to_string(),
        memory_dirty: "0 B".to_string(),
        swap_used: "0 B".to_string(),
        swap_total: "0 B".to_string(),
        swap_used_percent: "0.000%".to_string(),
        top_rss_process: "unavailable".to_string(),
        memory_full_pressure: "0.000%".to_string(),
        io_full_pressure: "0.000%".to_string(),
        disk_count: disk_count.to_string(),
        filesystem_rows: Vec::new(),
        network_interface_count: interface_count.to_string(),
        route_rows: Vec::new(),
        grow_root_service: "expanded".to_string(),
        ssh_service: "running".to_string(),
        networkd_service: "running".to_string(),
        dashboard_service: "running".to_string(),
        root_shell_service: "running".to_string(),
        disk_read_rate: "measuring".to_string(),
        disk_write_rate: "measuring".to_string(),
        disk_read_iops: "measuring".to_string(),
        disk_write_iops: "measuring".to_string(),
        network_rx_rate: "measuring".to_string(),
        network_tx_rate: "measuring".to_string(),
        network_rx_packet_rate: "measuring".to_string(),
        network_tx_packet_rate: "measuring".to_string(),
        network_rx_error_rate: "measuring".to_string(),
        network_tx_error_rate: "measuring".to_string(),
        network_rx_drop_rate: "measuring".to_string(),
        network_tx_drop_rate: "measuring".to_string(),
        network_error_state: "".to_string(),
        interface_attention_count: "0".to_string(),
        core_rows,
        disk_rows,
        network_rows,
        disk_link_summary: "nvme 0 - sata/scsi 0 - virtio 1 - usb 0 - mmc 0 - other 0".to_string(),
    }
}
