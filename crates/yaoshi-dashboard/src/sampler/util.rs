fn statvfs_usage(path: &Path) -> Option<(u64, u64)> {
    let c_path = CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    let total = stat.f_blocks.saturating_mul(stat.f_frsize);
    if total == 0 {
        return None;
    }
    let used = stat
        .f_blocks
        .saturating_sub(stat.f_bfree)
        .saturating_mul(stat.f_frsize);
    Some((used, total))
}

fn statvfs_inode_usage(path: &Path) -> Option<(u64, u64)> {
    let c_path = CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    let total = stat.f_files;
    if total == 0 {
        return None;
    }
    let used = stat.f_files.saturating_sub(stat.f_ffree);
    Some((used, total))
}

fn usage_strings(usage: Option<(u64, u64)>) -> (String, String, String) {
    usage
        .map(|(used, total)| {
            (
                format_bytes_binary(used),
                format_bytes_binary(total),
                format_percent_3(used, total),
            )
        })
        .unwrap_or_else(|| {
            (
                "unavailable".to_string(),
                "unavailable".to_string(),
                "unavailable".to_string(),
            )
        })
}

fn inode_usage_strings(usage: Option<(u64, u64)>) -> (String, String) {
    usage
        .map(|(used, total)| (format!("{used}/{total}"), format_percent_3(used, total)))
        .unwrap_or_else(|| ("unavailable".to_string(), "unavailable".to_string()))
}

fn read_nonempty(path: impl AsRef<Path>) -> String {
    read_trimmed(path)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(unavailable)
}

fn read_trimmed(path: impl AsRef<Path>) -> Result<String, std::io::Error> {
    Ok(fs::read_to_string(path)?.trim().to_string())
}

fn read_u64(path: impl AsRef<Path>) -> Result<u64, std::io::Error> {
    read_trimmed(&path)?.parse().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("parse {} as u64: {e}", path.as_ref().display()),
        )
    })
}

fn unescape_mount_path(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4].iter().all(|b| matches!(b, b'0'..=b'7'))
        {
            let value =
                (bytes[i + 1] - b'0') * 64 + (bytes[i + 2] - b'0') * 8 + (bytes[i + 3] - b'0');
            out.push(value);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn format_rate(bytes_per_second: u64) -> String {
    format_bytes_binary(bytes_per_second)
}

fn format_decimal_rate_unit(value: f64, unit: &str) -> String {
    if !value.is_finite() || value < 0.0 {
        return unavailable();
    }
    format!("{value:.3} {unit}")
}

fn unavailable() -> String {
    "unavailable".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_format_uses_binary_units() {
        assert_eq!(format_bytes_binary(512), "512 B");
        assert_eq!(format_bytes_binary(1536), "1.500 KiB");
        assert_eq!(format_bytes_binary(1024 * 1024), "1.000 MiB");
    }

    #[test]
    fn mount_path_unescapes_octal_bytes() {
        assert_eq!(unescape_mount_path("/a\\040b"), "/a b");
    }

    #[test]
    fn proc_stat_tty_and_ticks_parse_from_kernel_shape() {
        let stat = "1234 (bash) S 1 2 3 34818 5 0 0 0 0 0 17 23 0 0 20 0 1 0";
        assert_eq!(parse_proc_stat_tty_nr(stat), Some(34818));
        assert_eq!(parse_proc_stat_ticks(stat), Some(40));
    }

    #[test]
    fn empty_process_comm_displays_unavailable() {
        assert_eq!(display_comm("  "), "unavailable");
        assert_eq!(display_comm("sshd"), "sshd");
    }

    #[test]
    fn top_process_visibility_uses_design_thresholds() {
        let low = "bash pid 10 - cpu 24.999% - rss 1.000 MiB";
        let high = "bash pid 10 - cpu 25.000% - rss 1.000 MiB";
        assert!(!top_process_required("84.999%", low));
        assert!(top_process_required("84.999%", high));
        assert!(top_process_required("85.000%", "measuring"));
    }

    #[test]
    fn ssh_ready_requires_key_process_and_listening_port() {
        assert_eq!(ssh_ready_state("enabled", "running", "listening"), "ready");
        assert_eq!(
            ssh_ready_state("enabled", "running", "not-listening"),
            "starting"
        );
        assert_eq!(
            ssh_ready_state("enabled", "not-running", "listening"),
            "starting"
        );
        assert_eq!(
            ssh_ready_state("enabled", "running", "unavailable"),
            "unavailable"
        );
        assert_eq!(
            ssh_ready_state("disabled", "running", "listening"),
            "disabled"
        );
    }

    #[test]
    fn first_delta_network_errors_are_not_unavailable_alerts() {
        assert_eq!(
            network_error_state("measuring", "measuring", "measuring", "measuring"),
            ""
        );
        assert_eq!(
            network_error_state("0.000", "0.000", "0.001", "0.000"),
            "warning"
        );
        assert_eq!(
            network_error_state("0.000", "11.000", "0.000", "0.000"),
            "critical"
        );
        assert_eq!(
            network_error_state("unavailable", "unavailable", "unavailable", "unavailable"),
            "unavailable"
        );
    }

    #[test]
    fn inode_usage_keeps_percent_separate() {
        let (usage, percent) = inode_usage_strings(Some((10, 100)));
        assert_eq!(usage, "10/100");
        assert_eq!(percent, "10.000%");
    }

    #[test]
    fn disk_transport_matches_design_name_classes() {
        let path = Path::new("/does/not/exist");
        assert_eq!(disk_transport("nvme0n1", path), "nvme");
        assert_eq!(disk_transport("vda", path), "virtio");
        assert_eq!(disk_transport("sda", path), "sata/scsi");
        assert_eq!(disk_transport("mmcblk0", path), "mmc");
        assert_eq!(disk_transport("xvdz", path), "other");
    }
}
