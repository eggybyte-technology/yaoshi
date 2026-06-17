use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use yaoshi_common::format_bytes_binary;

pub fn root_expansion_state_from_text(text: Option<&str>) -> &'static str {
    match text.map(str::trim) {
        Some("expanded") => "expanded",
        Some("failed") => "failed",
        _ => "unknown",
    }
}

pub fn cpu_display_from_cpuinfo(text: &str) -> String {
    let vendor = first_cpuinfo_field(text, "vendor_id").map(cpu_vendor_display);
    let model =
        first_cpuinfo_field(text, "model name").or_else(|| first_cpuinfo_field(text, "Hardware"));
    match (vendor, model) {
        (Some(vendor), Some(model)) => {
            if model.starts_with(&vendor) {
                model
            } else {
                format!("{vendor} {model}")
            }
        }
        (Some(vendor), None) => vendor,
        (None, Some(model)) => model,
        (None, None) => "unavailable".to_string(),
    }
}

fn first_cpuinfo_field(text: &str, wanted: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == wanted)
            .then(|| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn cpu_vendor_display(vendor: String) -> String {
    match vendor.as_str() {
        "GenuineIntel" => "Intel".to_string(),
        "AuthenticAMD" => "AMD".to_string(),
        _ => vendor,
    }
}

pub fn disk_display(vendor: &str, model: &str) -> String {
    match (available(vendor), available(model)) {
        (Some(vendor), Some(model)) if model.starts_with(vendor) => model.to_string(),
        (Some(vendor), Some(model)) => format!("{vendor} {model}"),
        (Some(vendor), None) => vendor.to_string(),
        (None, Some(model)) => model.to_string(),
        (None, None) => "unavailable".to_string(),
    }
}

fn available(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty() && value != "unavailable").then_some(value)
}

pub fn smbios_memory_summary_from_sysfs(entry_point: &Path, dmi: &Path) -> String {
    if fs::read(entry_point).is_err() {
        return "unavailable".to_string();
    }
    let Ok(table) = fs::read(dmi) else {
        return "unavailable".to_string();
    };
    smbios_memory_summary_from_dmi_table(&table).unwrap_or_else(|| "unavailable".to_string())
}

pub fn smbios_memory_summary_from_dmi_table(table: &[u8]) -> Option<String> {
    let devices = parse_smbios_type17(table);
    memory_devices_summary(&devices)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryDevice {
    pub size_bytes: u64,
    pub manufacturer: Option<String>,
    pub speed_mt: Option<u16>,
    pub configured_speed_mt: Option<u16>,
    pub total_width: Option<u16>,
    pub data_width: Option<u16>,
}

fn parse_smbios_type17(table: &[u8]) -> Vec<MemoryDevice> {
    let mut offset = 0usize;
    let mut out = Vec::new();
    while offset + 4 <= table.len() {
        let structure_type = table[offset];
        let length = table[offset + 1] as usize;
        if length < 4 || offset + length > table.len() {
            break;
        }
        let formatted = &table[offset..offset + length];
        let strings_start = offset + length;
        let Some((strings, next)) = parse_smbios_strings(table, strings_start) else {
            break;
        };
        if structure_type == 17
            && let Some(device) = parse_type17_device(formatted, &strings)
            && device.size_bytes > 0
        {
            out.push(device);
        }
        offset = next;
    }
    out
}

fn parse_smbios_strings(table: &[u8], mut offset: usize) -> Option<(Vec<String>, usize)> {
    if offset >= table.len() {
        return Some((Vec::new(), table.len()));
    }
    let mut strings = Vec::new();
    loop {
        if offset >= table.len() {
            return None;
        }
        if table[offset] == 0 {
            let next = offset + 1;
            if next < table.len() && table[next] == 0 {
                return Some((strings, next + 1));
            }
            offset = next;
            continue;
        }
        let start = offset;
        while offset < table.len() && table[offset] != 0 {
            offset += 1;
        }
        strings.push(
            String::from_utf8_lossy(&table[start..offset])
                .trim()
                .to_string(),
        );
    }
}

fn parse_type17_device(formatted: &[u8], strings: &[String]) -> Option<MemoryDevice> {
    let size_raw = le_u16(formatted, 0x0c)?;
    let size_bytes = decode_memory_size(size_raw, le_u32(formatted, 0x1c));
    Some(MemoryDevice {
        size_bytes,
        manufacturer: string_at(strings, byte_at(formatted, 0x17)?),
        speed_mt: speed_at(formatted, 0x15),
        configured_speed_mt: speed_at(formatted, 0x20),
        total_width: width_at(formatted, 0x08),
        data_width: width_at(formatted, 0x0a),
    })
}

fn decode_memory_size(size_raw: u16, extended: Option<u32>) -> u64 {
    match size_raw {
        0 | 0xffff => 0,
        0x7fff => extended
            .map(|value| (value & 0x7fff_ffff) as u64 * 1024 * 1024)
            .unwrap_or(0),
        value if value & 0x8000 != 0 => ((value & 0x7fff) as u64) * 1024,
        value => value as u64 * 1024 * 1024,
    }
}

fn memory_devices_summary(devices: &[MemoryDevice]) -> Option<String> {
    if devices.is_empty() {
        return None;
    }
    let count = devices.len();
    let total = devices
        .iter()
        .fold(0u64, |acc, device| acc.saturating_add(device.size_bytes));
    let manufacturer = manufacturer_summary(devices);
    let speed = speed_summary(devices);
    let ecc = ecc_summary(devices);
    Some(format!(
        "{count} DIMMs - {} - {manufacturer} - {speed} - ecc {ecc}",
        format_bytes_binary(total)
    ))
}

fn manufacturer_summary(devices: &[MemoryDevice]) -> String {
    let mut totals: BTreeMap<String, (u64, usize)> = BTreeMap::new();
    for device in devices {
        let Some(manufacturer) = device
            .manufacturer
            .as_deref()
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let entry = totals.entry(manufacturer.to_string()).or_default();
        entry.0 = entry.0.saturating_add(device.size_bytes);
        entry.1 += 1;
    }
    if totals.is_empty() {
        return "unavailable".to_string();
    }
    if totals.len() == 1 {
        let (manufacturer, (_, count)) = totals.iter().next().unwrap();
        return format!("{manufacturer} x{count}");
    }
    let mut ordered = totals.into_iter().collect::<Vec<_>>();
    ordered.sort_by(
        |(name_a, (bytes_a, count_a)), (name_b, (bytes_b, count_b))| {
            bytes_b
                .cmp(bytes_a)
                .then_with(|| count_b.cmp(count_a))
                .then_with(|| name_a.cmp(name_b))
        },
    );
    format!("{}+{} vendors", ordered[0].0, ordered.len() - 1)
}

fn speed_summary(devices: &[MemoryDevice]) -> String {
    let mut speeds = devices
        .iter()
        .filter_map(|device| device.configured_speed_mt.or(device.speed_mt))
        .collect::<Vec<_>>();
    if speeds.is_empty() {
        return "speed unavailable".to_string();
    }
    speeds.sort_unstable();
    let min = speeds[0];
    let max = *speeds.last().unwrap();
    if min == max {
        format!("{min} MT/s")
    } else {
        format!("mixed {min}-{max} MT/s")
    }
}

fn ecc_summary(devices: &[MemoryDevice]) -> &'static str {
    if devices
        .iter()
        .any(|device| matches!(device.total_width.zip(device.data_width), Some((total, data)) if total > data))
    {
        "yes"
    } else if devices
        .iter()
        .all(|device| matches!(device.total_width.zip(device.data_width), Some((total, data)) if total == data))
    {
        "no"
    } else {
        "unknown"
    }
}

fn string_at(strings: &[String], index: u8) -> Option<String> {
    if index == 0 {
        return None;
    }
    strings
        .get(index as usize - 1)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn speed_at(bytes: &[u8], offset: usize) -> Option<u16> {
    le_u16(bytes, offset).filter(|value| *value != 0 && *value != 0xffff)
}

fn width_at(bytes: &[u8], offset: usize) -> Option<u16> {
    le_u16(bytes, offset).filter(|value| *value != 0xffff)
}

fn byte_at(bytes: &[u8], offset: usize) -> Option<u8> {
    bytes.get(offset).copied()
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let raw = bytes.get(offset..end)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let raw = bytes.get(offset..end)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

pub fn prepare_state_from_text(text: Option<&str>) -> &'static str {
    match text.map(str::trim) {
        Some("applied") => "applied",
        Some("running") => "running",
        Some("failed") => "failed",
        Some("") | None => "pending",
        _ => "unknown",
    }
}

pub fn ssh_root_key_state_and_count(text: Option<&str>) -> (&'static str, usize) {
    match text {
        Some(text) => {
            let count = text.lines().filter(|line| !line.trim().is_empty()).count();
            if count == 0 {
                ("disabled", 0)
            } else {
                ("enabled", count)
            }
        }
        None => ("unavailable", 0),
    }
}

pub fn ssh_ready_state(
    key_state: &str,
    process_state: &str,
    listening_state: &str,
) -> &'static str {
    match (key_state, process_state, listening_state) {
        ("enabled", "running", "listening") => "ready",
        ("enabled", "unavailable", _) | ("enabled", _, "unavailable") => "unavailable",
        ("enabled", _, _) => "starting",
        ("disabled", _, _) => "disabled",
        _ => "unavailable",
    }
}

pub fn tcp_22_listening_from_proc_tables(tcp4: Option<&str>, tcp6: Option<&str>) -> &'static str {
    let mut readable = false;
    for table in [tcp4, tcp6].into_iter().flatten() {
        readable = true;
        for line in table.lines().skip(1) {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() > 3
                && fields[1]
                    .rsplit_once(':')
                    .is_some_and(|(_, port)| port == "0016")
                && fields[3] == "0A"
            {
                return "listening";
            }
        }
    }
    if readable {
        "not-listening"
    } else {
        "unavailable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn root_expansion_states_match_design() {
        assert_eq!(
            root_expansion_state_from_text(Some("expanded\n")),
            "expanded"
        );
        assert_eq!(root_expansion_state_from_text(Some("failed\n")), "failed");
        assert_eq!(root_expansion_state_from_text(Some("other\n")), "unknown");
        assert_eq!(root_expansion_state_from_text(None), "unknown");
    }

    #[test]
    fn prepare_states_match_design() {
        assert_eq!(prepare_state_from_text(Some("applied\n")), "applied");
        assert_eq!(prepare_state_from_text(Some("running\n")), "running");
        assert_eq!(prepare_state_from_text(Some("failed\n")), "failed");
        assert_eq!(prepare_state_from_text(None), "pending");
        assert_eq!(prepare_state_from_text(Some("other\n")), "unknown");
    }

    #[test]
    fn tcp_22_listener_detection_matches_proc_net_shape() {
        let table = "sl local_address rem_address st\n0: 00000000:0016 00000000:0000 0A\n";
        assert_eq!(
            tcp_22_listening_from_proc_tables(Some(table), None),
            "listening"
        );
        assert_eq!(
            tcp_22_listening_from_proc_tables(Some("sl local_address rem_address st\n"), None),
            "not-listening"
        );
        assert_eq!(tcp_22_listening_from_proc_tables(None, None), "unavailable");
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
    fn cpu_identity_deduplicates_vendor_and_handles_missing_fields() {
        assert_eq!(
            cpu_display_from_cpuinfo("vendor_id : GenuineIntel\nmodel name : Intel Core Test\n"),
            "Intel Core Test"
        );
        assert_eq!(
            cpu_display_from_cpuinfo("vendor_id : AuthenticAMD\nmodel name : Ryzen Test\n"),
            "AMD Ryzen Test"
        );
        assert_eq!(
            cpu_display_from_cpuinfo("model name : ARM Test\n"),
            "ARM Test"
        );
        assert_eq!(
            cpu_display_from_cpuinfo("vendor_id : GenuineIntel\n"),
            "Intel"
        );
        assert_eq!(cpu_display_from_cpuinfo("bogus\n"), "unavailable");
    }

    #[test]
    fn disk_identity_display_matches_vendor_model_rules() {
        assert_eq!(disk_display("ACME", "ACME FastDisk"), "ACME FastDisk");
        assert_eq!(disk_display("ACME", "FastDisk"), "ACME FastDisk");
        assert_eq!(disk_display("ACME", "unavailable"), "ACME");
        assert_eq!(disk_display("unavailable", "FastDisk"), "FastDisk");
        assert_eq!(disk_display("unavailable", "unavailable"), "unavailable");
    }

    #[test]
    fn smbios_memory_summary_handles_populated_mixed_and_ecc_states() {
        let table = [
            type17(8192, 72, 64, 3200, 0, 1, &["ACME"]),
            type17(8192, 64, 64, 2666, 2933, 1, &["ACME"]),
            type17(0, 0xffff, 0xffff, 0, 0, 0, &[]),
        ]
        .concat();
        assert_eq!(
            smbios_memory_summary_from_dmi_table(&table).unwrap(),
            "2 DIMMs - 16.000 GiB - ACME x2 - mixed 2933-3200 MT/s - ecc yes"
        );

        let no_ecc = [type17(1024, 64, 64, 2400, 2400, 1, &["Solo"])].concat();
        assert_eq!(
            smbios_memory_summary_from_dmi_table(&no_ecc).unwrap(),
            "1 DIMMs - 1.000 GiB - Solo x1 - 2400 MT/s - ecc no"
        );

        let unknown = [type17(1024, 0xffff, 64, 0, 0, 0, &[])].concat();
        assert_eq!(
            smbios_memory_summary_from_dmi_table(&unknown).unwrap(),
            "1 DIMMs - 1.000 GiB - unavailable - speed unavailable - ecc unknown"
        );
    }

    #[test]
    fn smbios_memory_summary_rejects_missing_or_malformed_tables() {
        assert_eq!(smbios_memory_summary_from_dmi_table(&[]), None);
        assert_eq!(smbios_memory_summary_from_dmi_table(&[17, 3, 0, 0]), None);

        let root = std::env::temp_dir().join(format!(
            "yaoshi-smbios-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let entry = root.join("smbios_entry_point");
        let dmi = root.join("DMI");
        fs::write(&dmi, type17(1024, 64, 64, 2400, 0, 1, &["ACME"])).unwrap();
        assert_eq!(
            smbios_memory_summary_from_sysfs(&entry, &dmi),
            "unavailable"
        );
        fs::write(&entry, b"_SM_").unwrap();
        assert!(smbios_memory_summary_from_sysfs(&entry, &dmi).contains("1 DIMMs"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn smbios_memory_summary_sorts_multiple_manufacturers() {
        let table = [
            type17(4096, 64, 64, 2400, 0, 1, &["Beta"]),
            type17(8192, 64, 64, 2400, 0, 1, &["Alpha"]),
            type17(8192, 64, 64, 2400, 0, 1, &["Alpha"]),
        ]
        .concat();
        assert!(
            smbios_memory_summary_from_dmi_table(&table)
                .unwrap()
                .contains("Alpha+1 vendors")
        );
    }

    #[test]
    fn smbios_memory_summary_reads_bounded_manufacturer_string_index() {
        let table = [type17(
            24 * 1024,
            64,
            64,
            5600,
            6000,
            3,
            &["DIMM 1", "P0 CHANNEL A", "Biwin Storage"],
        )]
        .concat();
        assert_eq!(
            smbios_memory_summary_from_dmi_table(&table).unwrap(),
            "1 DIMMs - 24.000 GiB - Biwin Storage x1 - 6000 MT/s - ecc no"
        );
    }

    fn type17(
        size_mib: u16,
        total_width: u16,
        data_width: u16,
        speed: u16,
        configured_speed: u16,
        manufacturer_index: u8,
        strings: &[&str],
    ) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x22];
        bytes[0] = 17;
        bytes[1] = 0x22;
        bytes[0x08..0x0a].copy_from_slice(&total_width.to_le_bytes());
        bytes[0x0a..0x0c].copy_from_slice(&data_width.to_le_bytes());
        bytes[0x0c..0x0e].copy_from_slice(&size_mib.to_le_bytes());
        bytes[0x15..0x17].copy_from_slice(&speed.to_le_bytes());
        bytes[0x17] = manufacturer_index;
        bytes[0x20..0x22].copy_from_slice(&configured_speed.to_le_bytes());
        for string in strings {
            bytes.extend_from_slice(string.as_bytes());
            bytes.push(0);
        }
        if strings.is_empty() {
            bytes.push(0);
        }
        bytes.push(0);
        bytes
    }
}
