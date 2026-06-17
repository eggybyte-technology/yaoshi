fn read_disks() -> Result<Vec<DiskInfo>, std::io::Error> {
    let mut disks = Vec::new();
    for entry in fs::read_dir("/sys/block")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if excluded_disk(&name) {
            continue;
        }
        let sysfs = entry.path();
        let size_bytes = read_u64(sysfs.join("size"))
            .unwrap_or(0)
            .saturating_mul(512);
        let mut partitioned_bytes = 0u64;
        let mut device_refs = BTreeSet::new();
        if let Ok(dev) = read_trimmed(sysfs.join("dev")) {
            device_refs.insert(dev);
        }
        if let Ok(children) = fs::read_dir(&sysfs) {
            for child in children.flatten() {
                let path = child.path();
                if !path.join("partition").exists() {
                    continue;
                }
                partitioned_bytes = partitioned_bytes
                    .saturating_add(read_u64(path.join("size")).unwrap_or(0).saturating_mul(512));
                if let Ok(dev) = read_trimmed(path.join("dev")) {
                    device_refs.insert(dev);
                }
            }
        }
        disks.push(DiskInfo {
            transport: disk_transport(&name, &sysfs),
            name,
            size_bytes,
            partitioned_bytes,
            vendor: read_nonempty(sysfs.join("device/vendor")),
            model: read_nonempty(sysfs.join("device/model")),
            serial: read_nonempty(sysfs.join("device/serial")),
            device_refs,
        });
    }
    disks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(disks)
}

fn read_disk_io() -> Option<BTreeMap<String, DiskIo>> {
    let text = fs::read_to_string("/proc/diskstats").ok()?;
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }
        let name = fields[2].to_string();
        if excluded_disk(&name) {
            continue;
        }
        let read_ios = fields[3].parse::<u64>().ok()?;
        let read_sectors = fields[5].parse::<u64>().ok()?;
        let write_ios = fields[7].parse::<u64>().ok()?;
        let write_sectors = fields[9].parse::<u64>().ok()?;
        out.insert(
            name,
            DiskIo {
                read_ios,
                write_ios,
                read_bytes: read_sectors.saturating_mul(512),
                write_bytes: write_sectors.saturating_mul(512),
            },
        );
    }
    Some(out)
}

fn root_backing_disk(disks: &[DiskInfo]) -> Option<String> {
    let root_dev = read_mounts()
        .ok()?
        .into_iter()
        .find(|mount| mount.mount_point == Path::new("/"))?
        .major_minor;
    disks
        .iter()
        .find(|disk| disk.device_refs.contains(&root_dev))
        .map(|disk| disk.name.clone())
}

fn read_mounts() -> Result<Vec<MountInfo>, std::io::Error> {
    let text = fs::read_to_string("/proc/self/mountinfo")?;
    let mut mounts = Vec::new();
    for line in text.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let left_fields = left.split_whitespace().collect::<Vec<_>>();
        let right_fields = right.split_whitespace().collect::<Vec<_>>();
        if left_fields.len() < 5 || right_fields.len() < 2 {
            continue;
        }
        mounts.push(MountInfo {
            major_minor: left_fields[2].to_string(),
            mount_point: PathBuf::from(unescape_mount_path(left_fields[4])),
            fs_type: right_fields[0].to_string(),
            source: right_fields[1].to_string(),
        });
    }
    Ok(mounts)
}

fn filesystem_summary_for_disk(disk: &DiskInfo, mounts: &[MountInfo]) -> String {
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    let mut used = 0u64;
    let mut had_mount = false;
    let mut had_error = false;
    for mount in mounts {
        if !disk.device_refs.contains(&mount.major_minor) {
            continue;
        }
        let key = format!("{}:{}:{}", mount.major_minor, mount.fs_type, mount.source);
        if !seen.insert(key) {
            continue;
        }
        had_mount = true;
        match statvfs_usage(&mount.mount_point) {
            Some((mount_used, mount_total)) => {
                used = used.saturating_add(mount_used);
                total = total.saturating_add(mount_total);
            }
            None => had_error = true,
        }
    }
    if !had_mount {
        "not mounted".to_string()
    } else if had_error || total == 0 {
        "unavailable".to_string()
    } else {
        format!(
            "{}/{} ({})",
            format_bytes_binary(used),
            format_bytes_binary(total),
            format_percent_3(used, total)
        )
    }
}

fn required_filesystem_rows(mounts: &[MountInfo]) -> Vec<String> {
    [
        "/",
        "/boot",
        "/boot/efi",
        "/run",
        "/tmp",
        "/var",
        "/var/lib",
    ]
    .into_iter()
    .filter_map(|mount_point| {
        let path = Path::new(mount_point);
        let mount = mounts.iter().find(|mount| mount.mount_point == path)?;
        Some(match statvfs_usage(path) {
            Some((used, total)) if total > 0 => format!(
                "{mount_point} {} {}/{} ({}) - source {}",
                mount.fs_type,
                format_bytes_binary(used),
                format_bytes_binary(total),
                format_percent_3(used, total),
                mount.source
            ),
            _ => format!(
                "{mount_point} {} unavailable - source {}",
                mount.fs_type, mount.source
            ),
        })
    })
    .collect()
}

fn read_interfaces() -> Result<Vec<InterfaceInfo>, std::io::Error> {
    let address_map = read_address_map();
    let addresses_unavailable = address_map.is_err();
    let address_map = address_map.unwrap_or_default();
    let mut names = fs::read_dir("/sys/class/net")?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    names.sort();
    if names.iter().any(|name| name != "lo") {
        names.retain(|name| name != "lo");
    }
    let mut ifaces = Vec::new();
    for name in names {
        let sysfs = Path::new("/sys/class/net").join(&name);
        let addresses = address_map.get(&name).cloned().unwrap_or_default();
        let address_summary = if addresses_unavailable {
            unavailable()
        } else {
            address_summary(&addresses)
        };
        ifaces.push(InterfaceInfo {
            ifindex: read_trimmed(sysfs.join("ifindex"))
                .ok()
                .and_then(|raw| raw.parse::<u32>().ok())
                .unwrap_or(0),
            state: interface_state(&sysfs),
            kind: interface_kind(&name, &sysfs),
            mac: read_nonempty(sysfs.join("address")),
            mtu: read_nonempty(sysfs.join("mtu")),
            speed: interface_speed(&sysfs),
            rx_bytes: read_u64(sysfs.join("statistics/rx_bytes")).unwrap_or(0),
            tx_bytes: read_u64(sysfs.join("statistics/tx_bytes")).unwrap_or(0),
            rx_packets: read_u64(sysfs.join("statistics/rx_packets")).unwrap_or(0),
            tx_packets: read_u64(sysfs.join("statistics/tx_packets")).unwrap_or(0),
            rx_errors: read_u64(sysfs.join("statistics/rx_errors")).unwrap_or(0),
            tx_errors: read_u64(sysfs.join("statistics/tx_errors")).unwrap_or(0),
            rx_drops: read_u64(sysfs.join("statistics/rx_dropped")).unwrap_or(0),
            tx_drops: read_u64(sysfs.join("statistics/tx_dropped")).unwrap_or(0),
            name,
            address_summary,
            addresses,
            addresses_unavailable,
        });
    }
    Ok(ifaces)
}
