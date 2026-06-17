use std::fs::{self, File};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FbBitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FbVarScreeninfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: FbBitfield,
    green: FbBitfield,
    blue: FbBitfield,
    transp: FbBitfield,
    nonstd: u32,
    activate: u32,
    height: u32,
    width: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

impl Default for FbVarScreeninfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FbFixScreeninfo {
    id: [libc::c_char; 16],
    smem_start: libc::c_ulong,
    smem_len: u32,
    type_: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    line_length: u32,
    mmio_start: libc::c_ulong,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

impl Default for FbFixScreeninfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 2 {
        eprintln!("usage: yaoshi-real-capture <output-dir>");
        std::process::exit(2);
    }
    let root = PathBuf::from(&args[1]);
    if let Err(err) = run(&root) {
        eprintln!("yaoshi-real-capture: {err}");
        std::process::exit(1);
    }
}

fn run(root: &Path) -> std::io::Result<()> {
    fs::create_dir_all(root)?;
    let vcsa = fs::read("/dev/vcsa1").unwrap_or_default();
    let (rows, cols) = vcsa_grid(&vcsa).unwrap_or((0, 0));
    fs::write(root.join("tty1.attrs.bin"), &vcsa)?;
    fs::write(root.join("tty1.cells.txt"), tty_cells(rows, cols))?;
    fs::write(root.join("sources.txt"), sources_text())?;
    capture_framebuffer(root, rows, cols)?;
    Ok(())
}

fn vcsa_grid(bytes: &[u8]) -> Option<(usize, usize)> {
    let rows = *bytes.first()? as usize;
    let cols = *bytes.get(1)? as usize;
    (rows > 0 && cols > 0).then_some((rows, cols))
}

fn tty_cells(rows: usize, cols: usize) -> Vec<u8> {
    let raw = fs::read("/dev/vcs1").unwrap_or_default();
    if rows == 0 || cols == 0 {
        return sanitize_visible(&raw);
    }
    let mut out = Vec::with_capacity(rows.saturating_mul(cols + 1));
    for row in 0..rows {
        let start = row.saturating_mul(cols);
        let end = start.saturating_add(cols).min(raw.len());
        out.extend_from_slice(&sanitize_visible(&raw[start..end]));
        if end.saturating_sub(start) < cols {
            out.extend(std::iter::repeat_n(b' ', cols - end.saturating_sub(start)));
        }
        out.push(b'\n');
    }
    out
}

fn sanitize_visible(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .map(|byte| match *byte {
            b'\n' | 0x20..=0x7e => *byte,
            _ => b' ',
        })
        .collect()
}

fn sources_text() -> String {
    let checks = [
        ("cpu.procfs", "/proc/cpuinfo"),
        ("memory.procfs", "/proc/meminfo"),
        (
            "smbios.entry",
            "/sys/firmware/dmi/tables/smbios_entry_point",
        ),
        ("smbios.dmi", "/sys/firmware/dmi/tables/DMI"),
        ("storage.sysfs", "/sys/block"),
        ("storage.mountinfo", "/proc/self/mountinfo"),
        ("network.sysfs", "/sys/class/net"),
        ("thermal.sysfs", "/sys/class/thermal"),
        ("yaoshi.prepare", "/var/lib/yaoshi/prepare-state"),
        ("yaoshi.root_growth", "/var/lib/yaoshi/root-growth-state"),
    ];
    let mut text = String::new();
    for (key, path) in checks {
        let state = if File::open(path).is_ok() || fs::read_dir(path).is_ok() {
            "readable"
        } else if Path::new(path).exists() {
            "unreadable"
        } else {
            "absent"
        };
        text.push_str(&format!("{key}={state}\n"));
    }
    text
}

fn capture_framebuffer(root: &Path, rows: usize, cols: usize) -> std::io::Result<()> {
    let fb_path = Path::new("/dev/fb0");
    if !fb_path.exists() {
        return write_framebuffer_skip(root, rows, cols, "fb0-absent");
    }
    let mut file = match File::open(fb_path) {
        Ok(file) => file,
        Err(_) => return write_framebuffer_skip(root, rows, cols, "fb0-unreadable"),
    };
    let mut var = FbVarScreeninfo::default();
    let mut fix = FbFixScreeninfo::default();
    let var_ok = unsafe { libc::ioctl(file.as_raw_fd(), FBIOGET_VSCREENINFO as _, &mut var) } == 0;
    let fix_ok = unsafe { libc::ioctl(file.as_raw_fd(), FBIOGET_FSCREENINFO as _, &mut fix) } == 0;
    if !var_ok || !fix_ok {
        return write_framebuffer_skip(root, rows, cols, "fbioctl-unavailable");
    }
    if !matches!(var.bits_per_pixel, 16 | 24 | 32)
        || var.red.length == 0
        || var.green.length == 0
        || var.blue.length == 0
        || var.grayscale != 0
    {
        return write_framebuffer_skip(root, rows, cols, "fb-format-unsupported");
    }
    let visible_len = (fix.line_length as usize).saturating_mul(var.yres as usize);
    if visible_len == 0 || fix.line_length < bytes_per_pixel(var.bits_per_pixel) * var.xres {
        return write_framebuffer_skip(root, rows, cols, "fb-geometry-inconsistent");
    }
    let profile = framebuffer_profile(var.xres, var.yres, rows, cols);
    let Some(profile) = profile else {
        return write_framebuffer_skip(root, rows, cols, "fb-grid-mismatch");
    };
    let mut raw = vec![0u8; visible_len];
    file.read_exact(&mut raw)?;
    let rgb = normalized_rgb(&raw, &var, &fix);
    fs::write(root.join("framebuffer.raw"), &raw)?;
    fs::write(root.join("framebuffer.ppm"), ppm(&rgb, var.xres, var.yres))?;
    fs::write(root.join("framebuffer.png"), png(&rgb, var.xres, var.yres))?;
    let info = framebuffer_info("captured", "", rows, cols, &var, &fix, profile);
    fs::write(root.join("framebuffer.info"), info)
}

fn write_framebuffer_skip(
    root: &Path,
    rows: usize,
    cols: usize,
    reason: &str,
) -> std::io::Result<()> {
    let mut text = String::new();
    text.push_str("framebuffer_capture=skipped\n");
    text.push_str(&format!("skip_reason={reason}\n"));
    text.push_str(&format!("vc_cols={cols}\nvc_rows={rows}\n"));
    text.push_str("profile=unavailable\n");
    fs::write(root.join("framebuffer.info"), text)
}

fn framebuffer_info(
    state: &str,
    reason: &str,
    rows: usize,
    cols: usize,
    var: &FbVarScreeninfo,
    fix: &FbFixScreeninfo,
    profile: &str,
) -> String {
    let mut text = String::new();
    text.push_str(&format!("framebuffer_capture={state}\n"));
    if !reason.is_empty() {
        text.push_str(&format!("skip_reason={reason}\n"));
    }
    text.push_str(&format!("width={}\nheight={}\n", var.xres, var.yres));
    text.push_str(&format!("line_length={}\n", fix.line_length));
    text.push_str(&format!("bits_per_pixel={}\n", var.bits_per_pixel));
    text.push_str(&format!(
        "red_offset={}\nred_length={}\ngreen_offset={}\ngreen_length={}\nblue_offset={}\nblue_length={}\n",
        var.red.offset, var.red.length, var.green.offset, var.green.length, var.blue.offset, var.blue.length
    ));
    text.push_str(&format!(
        "vc_cols={cols}\nvc_rows={rows}\nprofile={profile}\n"
    ));
    text
}

fn framebuffer_profile(width: u32, height: u32, rows: usize, cols: usize) -> Option<&'static str> {
    if rows == 0 || cols == 0 {
        return None;
    }
    let cell_w = width / cols as u32;
    let cell_h = height / rows as u32;
    if cell_w >= 12 && cell_h >= 24 {
        Some("Terminus12x24")
    } else if cell_w >= 10 && cell_h >= 20 {
        Some("Terminus10x20")
    } else {
        None
    }
}

fn bytes_per_pixel(bits: u32) -> u32 {
    bits.div_ceil(8)
}

fn normalized_rgb(raw: &[u8], var: &FbVarScreeninfo, fix: &FbFixScreeninfo) -> Vec<u8> {
    let mut out = Vec::with_capacity((var.xres as usize) * (var.yres as usize) * 3);
    let bpp = bytes_per_pixel(var.bits_per_pixel) as usize;
    for y in 0..var.yres as usize {
        let row = y.saturating_mul(fix.line_length as usize);
        for x in 0..var.xres as usize {
            let offset = row + x.saturating_mul(bpp);
            let pixel = read_pixel(raw, offset, bpp);
            out.push(scale_channel(pixel, var.red));
            out.push(scale_channel(pixel, var.green));
            out.push(scale_channel(pixel, var.blue));
        }
    }
    out
}

fn ppm(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = format!("P6\n{width} {height}\n255\n").into_bytes();
    out.extend_from_slice(rgb);
    out
}

fn png(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut filtered = Vec::with_capacity((width as usize * 3 + 1) * height as usize);
    let stride = width as usize * 3;
    for y in 0..height as usize {
        filtered.push(0);
        let start = y.saturating_mul(stride);
        filtered.extend_from_slice(&rgb[start..start + stride]);
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &zlib_stored(&filtered));
    png_chunk(&mut out, b"IEND", &[]);
    out
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(kind.len() + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut remaining = data;
    while !remaining.is_empty() {
        let take = remaining.len().min(65_535);
        let final_block = take == remaining.len();
        out.push(if final_block { 0x01 } else { 0x00 });
        let len = take as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&remaining[..take]);
        remaining = &remaining[take..];
    }
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn read_pixel(raw: &[u8], offset: usize, bpp: usize) -> u32 {
    let mut bytes = [0u8; 4];
    for (idx, byte) in bytes.iter_mut().enumerate().take(bpp.min(4)) {
        *byte = *raw.get(offset + idx).unwrap_or(&0);
    }
    u32::from_le_bytes(bytes)
}

fn scale_channel(pixel: u32, field: FbBitfield) -> u8 {
    if field.length == 0 {
        return 0;
    }
    let mask = if field.length >= 32 {
        u32::MAX
    } else {
        (1u32 << field.length) - 1
    };
    let value = (pixel >> field.offset) & mask;
    ((value * 255) / mask.max(1)) as u8
}
