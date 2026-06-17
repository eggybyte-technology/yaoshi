use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const OUT_ROOT: &str = ".yaoshi/check/real-dashboard";
const TARGET_DASHBOARD: &str = "/boot/YAOSHI/DASHBOARD/YAOSHI-DASHBOARD";
const TARGET_CAPTURE: &str = "/tmp/yaoshi-real-capture";
const TARGET_CAPTURE_DIR: &str = "/run/yaoshi-real-dashboard";

fn main() {
    if let Err(err) = run() {
        eprintln!("real-dashboard-check: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    if std::env::args_os().len() != 1 {
        return Err("real-dashboard-check accepts no command-line arguments".to_string());
    }
    reject_extra_env()?;
    let host = required_env("YAOSHI_REAL_HOST")?;
    let key = required_env("YAOSHI_REAL_SSH_KEY")?;
    let out = PathBuf::from(OUT_ROOT);
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).map_err(|e| format!("create {}: {e}", out.display()))?;

    let mut preflight = Vec::new();
    let ssh_ok = ssh(&host, &key, "true").is_ok();
    preflight.push(("root_ssh", ssh_ok));
    preflight.push((
        "dashboard_symlink",
        ssh(
            &host,
            &key,
            "test -L /usr/bin/yaoshi-dashboard && test \"$(readlink /usr/bin/yaoshi-dashboard)\" = /boot/YAOSHI/DASHBOARD/YAOSHI-DASHBOARD",
        )
        .is_ok(),
    ));
    preflight.push((
        "boot_yaoshi_esp",
        ssh(
            &host,
            &key,
            "boot_dev=$(awk '$2==\"/boot\" { print $1; exit }' /proc/mounts); test -n \"$boot_dev\" && test \"$(readlink -f \"$boot_dev\")\" = \"$(readlink -f /dev/disk/by-label/YAOSHI_ESP)\"",
        )
        .is_ok(),
    ));
    for (name, path) in [
        ("tty1_exists", "/dev/tty1"),
        ("vcs1_readable", "/dev/vcs1"),
        ("vcsa1_readable", "/dev/vcsa1"),
    ] {
        preflight.push((name, ssh(&host, &key, &format!("test -r {path}")).is_ok()));
    }
    preflight.push((
        "vc_grid_80x24",
        ssh(
            &host,
            &key,
            "set -- $(dd if=/dev/vcsa1 bs=1 count=2 2>/dev/null | od -An -tu1); test \"$1\" -ge 24 && test \"$2\" -ge 80",
        )
        .is_ok(),
    ));
    preflight.push((
        "dashboard_service_exists",
        ssh(
            &host,
            &key,
            "test -e /etc/systemd/system/yaoshi-dashboard.service -o -e /lib/systemd/system/yaoshi-dashboard.service",
        )
        .is_ok(),
    ));
    write_kv(&out.join("preflight.txt"), &preflight)?;
    if preflight.iter().any(|(_, ok)| !*ok) {
        return Err("required preflight failed".to_string());
    }

    redeploy(&host, &key)?;
    let service_active = settle(&host, &key)?;
    fs::write(
        out.join("service.txt"),
        format!("restart=pass\nactive={}\n", passfail(service_active)),
    )
    .map_err(|e| format!("write service.txt: {e}"))?;
    if !service_active {
        return Err("service restart did not settle".to_string());
    }

    copy_capture_helper(&host, &key)?;
    ssh(
        &host,
        &key,
        &format!("rm -rf {TARGET_CAPTURE_DIR}; {TARGET_CAPTURE} {TARGET_CAPTURE_DIR}"),
    )?;
    for file in [
        "tty1.cells.txt",
        "tty1.attrs.bin",
        "sources.txt",
        "framebuffer.info",
        "framebuffer.raw",
        "framebuffer.ppm",
        "framebuffer.png",
    ] {
        let remote = format!("{TARGET_CAPTURE_DIR}/{file}");
        if let Ok(bytes) = ssh_file_bytes(&host, &key, &remote) {
            fs::write(out.join(file), bytes).map_err(|e| format!("write {file}: {e}"))?;
        }
    }
    let _ = ssh(
        &host,
        &key,
        &format!("rm -f {TARGET_CAPTURE}; rm -rf {TARGET_CAPTURE_DIR}"),
    );
    evaluate(&out)
}

fn reject_extra_env() -> Result<(), String> {
    for (key, _) in std::env::vars() {
        if key.starts_with("YAOSHI_REAL_")
            && key != "YAOSHI_REAL_HOST"
            && key != "YAOSHI_REAL_SSH_KEY"
        {
            return Err(format!("unsupported environment variable {key}"));
        }
    }
    Ok(())
}

fn required_env(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is required"))
}

fn redeploy(host: &str, key: &str) -> Result<(), String> {
    let local_dashboard = Path::new("target/x86_64-unknown-linux-musl/debug/yaoshi-dashboard");
    if !local_dashboard.is_file() {
        return Err(format!(
            "missing {}; run cargo build --locked --target x86_64-unknown-linux-musl -p yaoshi-dashboard",
            local_dashboard.display()
        ));
    }
    ssh(host, key, "mount -o remount,rw /boot")?;
    scp_to(
        host,
        key,
        local_dashboard,
        &format!("{TARGET_DASHBOARD}.next"),
    )?;
    ssh(
        host,
        key,
        &format!(
            "chmod 0755 {TARGET_DASHBOARD}.next && mv -f {TARGET_DASHBOARD}.next {TARGET_DASHBOARD} && sync && systemctl restart yaoshi-dashboard.service && mount -o remount,ro /boot"
        ),
    )
}

fn settle(host: &str, key: &str) -> Result<bool, String> {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut first_frame: Option<Instant> = None;
    while Instant::now() < deadline {
        let active = ssh(
            host,
            key,
            "systemctl is-active --quiet yaoshi-dashboard.service",
        )
        .is_ok();
        let cells = ssh_string(host, key, "cat /dev/vcs1").unwrap_or_default();
        let matched = cells.contains("Yaoshi Dashboard v0.0.1")
            && [
                "Prepare", "Root", "SSH", "Net", "Health", "System", "Compute", "Memory",
                "Storage", "Network",
            ]
            .iter()
            .all(|needle| cells.contains(needle));
        if active && matched {
            let first = *first_frame.get_or_insert_with(Instant::now);
            if first.elapsed() >= Duration::from_millis(1000) {
                return Ok(true);
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    Ok(false)
}

fn copy_capture_helper(host: &str, key: &str) -> Result<(), String> {
    let helper = Path::new("target/x86_64-unknown-linux-musl/debug/yaoshi-real-capture");
    if !helper.is_file() {
        return Err(format!(
            "missing {}; run cargo build --locked --target x86_64-unknown-linux-musl -p yaoshi-test --bin yaoshi-real-capture",
            helper.display()
        ));
    }
    scp_to(host, key, helper, TARGET_CAPTURE)?;
    ssh(host, key, &format!("chmod 0755 {TARGET_CAPTURE}"))
}

fn evaluate(out: &Path) -> Result<(), String> {
    let cells = fs::read_to_string(out.join("tty1.cells.txt"))
        .map_err(|e| format!("read tty1.cells.txt: {e}"))?;
    let attrs =
        fs::read(out.join("tty1.attrs.bin")).map_err(|e| format!("read tty1.attrs.bin: {e}"))?;
    let fb_info = fs::read_to_string(out.join("framebuffer.info"))
        .map_err(|e| format!("read framebuffer.info: {e}"))?;
    let predicates = [
        ("required_preflight", true),
        (
            "service_restart",
            fs::read_to_string(out.join("service.txt"))
                .unwrap_or_default()
                .contains("active=pass"),
        ),
        (
            "dashboard_header",
            cells.contains("Yaoshi Dashboard v0.0.1"),
        ),
        (
            "statusline_groups",
            ["Prepare", "Root", "SSH", "Net", "Health"]
                .iter()
                .all(|needle| cells.contains(needle)),
        ),
        (
            "section_labels",
            ["System", "Compute", "Memory", "Storage", "Network"]
                .iter()
                .all(|needle| cells.contains(needle)),
        ),
        (
            "machine_row",
            cells.contains("Machine   ")
                && (cells.contains(" - board ") || cells.contains("unavailable")),
        ),
        ("cpu_row", cells.contains("CPU       ")),
        ("dimms_row", cells.contains("DIMMs     ")),
        (
            "storage_root_row",
            cells.contains("Root      ") && cells.contains(" - inode "),
        ),
        (
            "storage_disks_row",
            cells.contains("Disks     ")
                && cells.contains(" - root ")
                && cells.contains(" - attention "),
        ),
        (
            "storage_detail_order",
            storage_detail_units_are_two_rows(&cells),
        ),
        ("network_route_row", cells.contains("Route     default ")),
        (
            "network_nics_row",
            cells.contains("NICs      ")
                && cells.contains(" - route ")
                && cells.contains(" - access ")
                && cells.contains(" - attention "),
        ),
        (
            "network_detail_order",
            network_detail_units_are_two_rows(&cells),
        ),
        (
            "ascii_visible",
            cells
                .bytes()
                .all(|b| b == b'\n' || (0x20..=0x7e).contains(&b)),
        ),
        (
            "vcsa_grid",
            attrs.first().copied().unwrap_or(0) >= 24 && attrs.get(1).copied().unwrap_or(0) >= 80,
        ),
        (
            "framebuffer_state",
            fb_info.contains("framebuffer_capture=captured")
                || fb_info.contains("framebuffer_capture=skipped"),
        ),
        ("no_ocr_predicate", true),
    ];
    let mut text = String::new();
    for (name, ok) in predicates {
        text.push_str(&format!("{name}={}\n", passfail(ok)));
        if !ok {
            fs::write(out.join("evaluation.txt"), text)
                .map_err(|e| format!("write evaluation.txt: {e}"))?;
            return Err(format!("evaluation predicate failed: {name}"));
        }
    }
    if fb_info.contains("framebuffer_capture=captured") {
        let (width, height) = framebuffer_dimensions(&fb_info)?;
        let ppm = fs::read(out.join("framebuffer.ppm"))
            .map_err(|e| format!("read framebuffer.ppm: {e}"))?;
        let png = fs::read(out.join("framebuffer.png"))
            .map_err(|e| format!("read framebuffer.png: {e}"))?;
        let ppm_dimensions_ok = ppm_dimensions(&ppm) == Some((width, height));
        let png_dimensions_ok = png_dimensions(&png) == Some((width, height));
        let ppm_nonuniform = ppm.len() > 16 && !ppm_uniform(&ppm);
        let png_nonuniform = !png_uniform(&png).unwrap_or(true);
        for (name, ok) in [
            ("framebuffer_ppm_dimensions", ppm_dimensions_ok),
            ("framebuffer_png_dimensions", png_dimensions_ok),
            ("framebuffer_ppm_nonempty_nonuniform", ppm_nonuniform),
            ("framebuffer_png_nonempty_nonuniform", png_nonuniform),
        ] {
            text.push_str(&format!("{name}={}\n", passfail(ok)));
            if !ok {
                fs::write(out.join("evaluation.txt"), text)
                    .map_err(|e| format!("write evaluation.txt: {e}"))?;
                return Err(format!("captured framebuffer artifact failed: {name}"));
            }
        }
    }
    fs::write(out.join("evaluation.txt"), text).map_err(|e| format!("write evaluation.txt: {e}"))
}

fn ppm_uniform(bytes: &[u8]) -> bool {
    let Some(start) = nth_newline(bytes, 3) else {
        return true;
    };
    let data = &bytes[start..];
    data.chunks_exact(3)
        .next()
        .is_none_or(|first| data.chunks_exact(3).all(|pixel| pixel == first))
}

fn storage_detail_units_are_two_rows(cells: &str) -> bool {
    let lines = cells.lines().collect::<Vec<_>>();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if (trimmed.starts_with("* /dev/") || trimmed.starts_with("/dev/"))
            && line.contains(" - ")
            && !line.contains(" - fs ")
        {
            let Some(next) = lines.get(idx + 1) else {
                return false;
            };
            if !next.trim_start().starts_with("sn ") || !next.contains(" - fs ") {
                return false;
            }
        }
    }
    true
}

fn network_detail_units_are_two_rows(cells: &str) -> bool {
    let mut saw_unit = false;
    let lines = cells.lines().collect::<Vec<_>>();
    for (idx, line) in lines.iter().enumerate() {
        if line.contains(" route ")
            || line.contains(" route+access ")
            || line.contains(" access ")
            || line.contains(" iface ")
        {
            if !line.contains(" - mtu ") {
                continue;
            }
            let Some(next) = lines.get(idx + 1) else {
                return false;
            };
            if !next.contains("mac ") || !next.contains(" - drop rx ") {
                return false;
            }
            saw_unit = true;
        }
    }
    saw_unit || !cells.contains("       mac ")
}

fn framebuffer_dimensions(info: &str) -> Result<(u32, u32), String> {
    let width = info
        .lines()
        .find_map(|line| line.strip_prefix("width="))
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "framebuffer.info missing width".to_string())?;
    let height = info
        .lines()
        .find_map(|line| line.strip_prefix("height="))
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "framebuffer.info missing height".to_string())?;
    Ok((width, height))
}

fn ppm_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let text = std::str::from_utf8(bytes.get(..bytes.len().min(64))?).ok()?;
    let mut lines = text.lines();
    (lines.next()? == "P6").then_some(())?;
    let dims = lines.next()?;
    let mut parts = dims.split_whitespace();
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    (bytes.get(..8)? == b"\x89PNG\r\n\x1a\n").then_some(())?;
    (bytes.get(12..16)? == b"IHDR").then_some(())?;
    Some((
        u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?),
        u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?),
    ))
}

fn png_uniform(bytes: &[u8]) -> Option<bool> {
    let (width, height) = png_dimensions(bytes)?;
    let idat = png_idat(bytes)?;
    let data = zlib_stored_data(&idat)?;
    let stride = width as usize * 3;
    if data.len() != (stride + 1) * height as usize {
        return Some(true);
    }
    let mut first = None;
    for row in 0..height as usize {
        let start = row * (stride + 1);
        if data[start] != 0 {
            return Some(true);
        }
        for pixel in data[start + 1..start + 1 + stride].chunks_exact(3) {
            match first {
                Some(first) if first != pixel => return Some(false),
                None => first = Some(pixel),
                _ => {}
            }
        }
    }
    Some(true)
}

fn png_idat(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut offset = 8usize;
    let mut out = Vec::new();
    while offset + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as usize;
        let kind = bytes.get(offset + 4..offset + 8)?;
        let data_start = offset + 8;
        let data_end = data_start + len;
        let next = data_end + 4;
        if next > bytes.len() {
            return None;
        }
        if kind == b"IDAT" {
            out.extend_from_slice(&bytes[data_start..data_end]);
        } else if kind == b"IEND" {
            break;
        }
        offset = next;
    }
    Some(out)
}

fn zlib_stored_data(bytes: &[u8]) -> Option<Vec<u8>> {
    (bytes.get(..2)? == [0x78, 0x01]).then_some(())?;
    let mut offset = 2usize;
    let mut out = Vec::new();
    loop {
        let header = *bytes.get(offset)?;
        offset += 1;
        let final_block = header & 1 == 1;
        if header & 0b0000_0110 != 0 {
            return None;
        }
        let len = u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?) as usize;
        offset += 2;
        let nlen = u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?);
        offset += 2;
        if nlen != !(len as u16) || offset + len > bytes.len() {
            return None;
        }
        out.extend_from_slice(&bytes[offset..offset + len]);
        offset += len;
        if final_block {
            break;
        }
    }
    (offset + 4 == bytes.len()).then_some(out)
}

fn nth_newline(bytes: &[u8], count: usize) -> Option<usize> {
    let mut seen = 0;
    for (idx, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            seen += 1;
            if seen == count {
                return Some(idx + 1);
            }
        }
    }
    None
}

fn write_kv(path: &Path, values: &[(&str, bool)]) -> Result<(), String> {
    let mut text = String::new();
    for (key, ok) in values {
        text.push_str(&format!("{key}={}\n", passfail(*ok)));
    }
    fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

fn passfail(ok: bool) -> &'static str {
    if ok { "pass" } else { "fail" }
}

fn ssh(host: &str, key: &str, remote_cmd: &str) -> Result<(), String> {
    let status = base_ssh(host, key)
        .arg(remote_cmd)
        .stdout(Stdio::null())
        .status()
        .map_err(|e| format!("spawn ssh: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("ssh command failed: {remote_cmd}"))
    }
}

fn ssh_file_bytes(host: &str, key: &str, remote_path: &str) -> Result<Vec<u8>, String> {
    const MARKER: &[u8] = b"\nYAOSHI_REAL_DASHBOARD_FILE_BEGIN\n";
    let bytes = ssh_bytes(
        host,
        key,
        &format!("printf '\\nYAOSHI_REAL_DASHBOARD_FILE_BEGIN\\n'; cat {remote_path}"),
    )?;
    let Some(offset) = find_subslice(&bytes, MARKER) else {
        return Err(format!("ssh file marker missing for {remote_path}"));
    };
    Ok(bytes[offset + MARKER.len()..].to_vec())
}

fn ssh_string(host: &str, key: &str, remote_cmd: &str) -> Result<String, String> {
    let bytes = ssh_bytes(host, key, remote_cmd)?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn ssh_bytes(host: &str, key: &str, remote_cmd: &str) -> Result<Vec<u8>, String> {
    let output = base_ssh(host, key)
        .arg(remote_cmd)
        .output()
        .map_err(|e| format!("spawn ssh: {e}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!("ssh command failed: {remote_cmd}"))
    }
}

fn base_ssh(host: &str, key: &str) -> Command {
    let mut command = Command::new("ssh");
    command
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-i",
            key,
        ])
        .arg(format!("root@{host}"));
    command
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn scp_to(host: &str, key: &str, local: &Path, remote: &str) -> Result<(), String> {
    let mut child = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-i",
            key,
            &format!("root@{host}"),
            &format!("cat > {remote}"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn ssh copy: {e}"))?;
    let mut input = fs::File::open(local).map_err(|e| format!("open {}: {e}", local.display()))?;
    let mut stdin = child.stdin.take().ok_or("open ssh stdin")?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = input
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", local.display()))?;
        if n == 0 {
            break;
        }
        stdin
            .write_all(&buf[..n])
            .map_err(|e| format!("write remote {remote}: {e}"))?;
    }
    drop(stdin);
    let status = child.wait().map_err(|e| format!("wait ssh copy: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("copy to root@{host}:{remote} failed"))
    }
}
