fn tree_file(tree: &Path, rel: &str) -> YaoshiResult<yaoshi_image::TreeFile> {
    Ok(yaoshi_image::TreeFile {
        path: PathBuf::from(rel),
        bytes: fs::read(tree.join(rel))
            .map_err(|e| YaoshiError::image(format!("read staged FAT32 file {rel}: {e}")))?,
    })
}

fn sha256_file_hex(path: &Path) -> YaoshiResult<String> {
    let metadata = fs::metadata(path).map_err(|e| {
        YaoshiError::build(format!("stat artifact for sha256 {}: {e}", path.display()))
    })?;
    let key = FileHashMemoKey {
        path: path.to_path_buf(),
        len: metadata.len(),
        mtime_sec: metadata.mtime(),
        mtime_nsec: metadata.mtime_nsec(),
    };
    if let Some(digest) = file_hash_memo()
        .lock()
        .map_err(|_| YaoshiError::internal("file hash memo lock poisoned"))?
        .get(&key)
        .cloned()
    {
        return Ok(digest);
    }
    let mut file = File::open(path).map_err(|e| {
        YaoshiError::build(format!("open artifact for sha256 {}: {e}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| YaoshiError::build(format!("read artifact for sha256: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hex_digest(hasher.finalize().as_slice());
    file_hash_memo()
        .lock()
        .map_err(|_| YaoshiError::internal("file hash memo lock poisoned"))?
        .insert(key, digest.clone());
    Ok(digest)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FileHashMemoKey {
    path: PathBuf,
    len: u64,
    mtime_sec: i64,
    mtime_nsec: i64,
}

fn file_hash_memo() -> &'static std::sync::Mutex<BTreeMap<FileHashMemoKey, String>> {
    static MEMO: std::sync::OnceLock<std::sync::Mutex<BTreeMap<FileHashMemoKey, String>>> =
        std::sync::OnceLock::new();
    MEMO.get_or_init(|| std::sync::Mutex::new(BTreeMap::new()))
}

fn digest_from_cache_object_path(path: &Path) -> YaoshiResult<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| is_lower_sha256_hex(name))
        .map(ToOwned::to_owned)
        .ok_or_else(|| YaoshiError::build("cache object path is not digest-addressed"))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_digest(hasher.finalize().as_slice())
}

fn command_stdout_lossy(program: &str, args: &[&str]) -> YaoshiResult<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| YaoshiError::build(format!("invoke {program}: {e}")))?;
    if !output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let text = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    Ok(String::from_utf8_lossy(text).trim().to_string())
}

fn run_e2fsck(path: &Path) -> YaoshiResult<()> {
    let status = Command::new("e2fsck")
        .args([OsStr::new("-fn"), path.as_os_str()])
        .status()
        .map_err(|e| YaoshiError::build(format!("invoke e2fsck: {e}")))?;
    if !matches!(status.code(), Some(0)) {
        return Err(YaoshiError::build("e2fsck installed-root.ext4 failed"));
    }
    Ok(())
}

fn validate_ext4_label(path: &Path) -> YaoshiResult<()> {
    let output = Command::new("e2label")
        .arg(path)
        .output()
        .map_err(|e| YaoshiError::build(format!("invoke e2label: {e}")))?;
    if !output.status.success() || output.stdout != b"YAOSHI_ROOT\n" {
        return Err(YaoshiError::build(
            "installed-root.ext4 label validation failed",
        ));
    }
    Ok(())
}

fn validate_ext4_overlay_paths(path: &Path) -> YaoshiResult<()> {
    for required in [
        "/etc/fstab",
        "/usr/lib/yaoshi/prepare-launcher",
        "/usr/bin/yaoshi-dashboard",
        "/var/lib/dpkg/status",
    ] {
        let status = Command::new("debugfs")
            .arg("-R")
            .arg(format!("stat {required}"))
            .arg(path)
            .status()
            .map_err(|e| YaoshiError::build(format!("invoke read-only debugfs: {e}")))?;
        if !status.success() {
            return Err(YaoshiError::build(format!(
                "installed-root.ext4 missing required path {required}"
            )));
        }
    }
    Ok(())
}

fn package_manifest_is_valid(path: &Path, effective_packages: &[String]) -> YaoshiResult<bool> {
    let text = fs::read_to_string(path)
        .map_err(|e| YaoshiError::build(format!("read installed-root package index: {e}")))?;
    if !text.ends_with('\n') {
        return Ok(false);
    }
    let mut packages = BTreeSet::new();
    let mut previous = "";
    for line in text.lines() {
        let cols = line.split(' ').collect::<Vec<_>>();
        if cols.len() != 4 || cols.iter().any(|col| col.is_empty()) || cols[3] != "installed" {
            return Ok(false);
        }
        if !previous.is_empty() && cols[0] <= previous {
            return Ok(false);
        }
        previous = cols[0];
        packages.insert(cols[0].to_string());
    }
    for required in effective_packages {
        if !packages.contains(required) {
            return Ok(false);
        }
    }
    for package in &packages {
        if package.starts_with("grub-")
            || package == "dracut"
            || package == "dracut-core"
            || package == "cloud-init"
            || package == "kbd"
            || package == "network-manager"
            || package.starts_with("shim-")
            || package == "dkms"
            || package.starts_with("linux-headers-")
        {
            return Ok(false);
        }
    }
    Ok(!packages.is_empty())
}

fn write_build_system_runner(dir: &Path) -> YaoshiResult<PathBuf> {
    fs::create_dir_all(dir)
        .map_err(|e| YaoshiError::build(format!("create build-system helper dir: {e}")))?;
    let path = dir.join("RUN-BUILD-SYSTEM.sh");
    write_exec(
        &path,
        &render_build_template(
            templates::render_debian_build_system_runner_sh(),
            "debian/build-system-runner.sh.askama",
        )?,
    )?;
    Ok(path)
}

fn write_build_system_script(dir: &Path, config: &EffectiveProductConfig) -> YaoshiResult<PathBuf> {
    fs::create_dir_all(dir)
        .map_err(|e| YaoshiError::build(format!("create build-system script dir: {e}")))?;
    let path = dir.join("BUILD-SYSTEM.sh");
    write_bytes(&path, &config.build_system_script_bytes, 0o644)?;
    Ok(path)
}

fn prepare_build_system_assets(context: &BuildContext, dir: &Path) -> YaoshiResult<PathBuf> {
    fs::create_dir_all(dir)
        .map_err(|e| YaoshiError::build(format!("create build-system asset dir: {e}")))?;
    let staged = dir.join("assets");
    if staged.exists() {
        fs::remove_dir_all(&staged)
            .map_err(|e| YaoshiError::build(format!("reset build-system assets: {e}")))?;
    }
    fs::create_dir_all(&staged)
        .map_err(|e| YaoshiError::build(format!("create build-system assets: {e}")))?;
    let source = context.repo_root.join(BUILD_SYSTEM_ASSETS_PATH);
    if source.exists() {
        if !source.is_dir() {
            return Err(YaoshiError::config(format!(
                "{BUILD_SYSTEM_ASSETS_PATH} must be a directory when present"
            )));
        }
        read_host_tree(&source)?;
        copy_dir_recursive(&source, &staged)?;
    }
    Ok(staged)
}

fn last_log_lines(path: &Path, limit: usize) -> YaoshiResult<String> {
    let text = fs::read_to_string(path)
        .map_err(|e| YaoshiError::build(format!("read build-system script log: {e}")))?;
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    let mut out = lines[start..].join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

fn packages_index_from_status(status: &Path) -> YaoshiResult<String> {
    let text = fs::read_to_string(status)
        .map_err(|e| YaoshiError::build(format!("read dpkg status: {e}")))?;
    let mut rows = BTreeMap::<String, (String, String)>::new();
    for para in text.split("\n\n") {
        let mut package = None;
        let mut version = None;
        let mut arch = None;
        let mut installed = false;
        for line in para.lines() {
            if let Some(value) = line.strip_prefix("Package: ") {
                package = Some(value.to_string());
            } else if let Some(value) = line.strip_prefix("Version: ") {
                version = Some(value.to_string());
            } else if let Some(value) = line.strip_prefix("Architecture: ") {
                arch = Some(value.to_string());
            } else if line == "Status: install ok installed" {
                installed = true;
            }
        }
        if installed && let (Some(package), Some(version), Some(arch)) = (package, version, arch) {
            rows.insert(package, (version, arch));
        }
    }
    let mut out = String::new();
    for (package, (version, arch)) in rows {
        out.push_str(&format!("{package} {version} {arch} installed\n"));
    }
    Ok(out)
}

fn write_mke2fs_conf(dir: &Path) -> YaoshiResult<PathBuf> {
    fs::create_dir_all(dir)
        .map_err(|e| YaoshiError::build(format!("create mke2fs config dir: {e}")))?;
    let path = dir.join("mke2fs.conf");
    write_bytes(
        &path,
        render_build_template(
            templates::render_debian_mke2fs_conf(),
            "debian/mke2fs.conf.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    Ok(path)
}

fn write_root_export_helper(dir: &Path) -> YaoshiResult<PathBuf> {
    fs::create_dir_all(dir)
        .map_err(|e| YaoshiError::build(format!("create root export helper dir: {e}")))?;
    let path = dir.join("root-export-helper");
    write_exec(
        &path,
        &render_build_template(
            templates::render_debian_root_export_helper_sh(),
            "debian/root-export-helper.sh.askama",
        )?,
    )?;
    Ok(path)
}

fn mke2fs_tar_input_preflight(mke2fs_conf: &Path) -> YaoshiResult<()> {
    let base = std::env::temp_dir().join(format!(
        "yaoshi-mke2fs-tar-preflight-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir_all(&base)
        .map_err(|e| YaoshiError::environment(format!("create mke2fs preflight dir: {e}")))?;
    let tree = base.join("tree");
    fs::create_dir_all(&tree)
        .map_err(|e| YaoshiError::environment(format!("create mke2fs preflight tree: {e}")))?;
    fs::write(tree.join("file"), b"x")
        .map_err(|e| YaoshiError::environment(format!("write mke2fs preflight file: {e}")))?;
    let tar = base.join("root.tar");
    write_ustar_from_tree(&tree, &tar)?;
    let img = base.join("root.ext4");
    let status = Command::new("mke2fs")
        .env("MKE2FS_CONFIG", mke2fs_conf)
        .arg("-q")
        .arg("-F")
        .arg("-t")
        .arg("ext4")
        .arg("-b")
        .arg("4096")
        .arg("-I")
        .arg("256")
        .arg("-d")
        .arg(&tar)
        .arg(&img)
        .arg("8192")
        .status()
        .map_err(|e| YaoshiError::environment(format!("invoke mke2fs tar preflight: {e}")))?;
    let _ = fs::remove_dir_all(&base);
    if status.success() {
        Ok(())
    } else {
        Err(YaoshiError::environment(
            "active e2fsprogs mke2fs does not accept tarball input through -d",
        ))
    }
}

fn normalize_export_dir(export_out: &Path) -> YaoshiResult<PathBuf> {
    if export_out.join("dpkg.status").is_file() {
        return Ok(export_out.to_path_buf());
    }
    if export_out.join("YAOSHI-EXPORT/dpkg.status").is_file() {
        return Ok(export_out.join("YAOSHI-EXPORT"));
    }
    Err(YaoshiError::build(
        "mmdebstrap root export is missing dpkg.status",
    ))
}

fn tar_regular_file_bytes(tar: &Path) -> YaoshiResult<u64> {
    let mut file = File::open(tar)
        .map_err(|e| YaoshiError::build(format!("open root tar for sizing: {e}")))?;
    let mut total = 0u64;
    loop {
        let mut header = [0u8; 512];
        file.read_exact(&mut header)
            .map_err(|e| YaoshiError::build(format!("read tar header: {e}")))?;
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let size = tar_octal(&header[124..136])?;
        let typeflag = header[156];
        if typeflag == b'0' || typeflag == 0 {
            total = total
                .checked_add(size)
                .ok_or_else(|| YaoshiError::build("root tar regular-file size overflows"))?;
        }
        let skip = round_up(size, 512);
        file.seek(SeekFrom::Current(skip as i64))
            .map_err(|e| YaoshiError::build(format!("seek tar body: {e}")))?;
    }
    Ok(total)
}

fn tar_octal(field: &[u8]) -> YaoshiResult<u64> {
    let text = String::from_utf8_lossy(field);
    let trimmed = text.trim_matches(char::from(0)).trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(trimmed, 8).map_err(|_| YaoshiError::build("invalid tar octal field"))
}

fn write_ustar_from_tree(root: &Path, tar: &Path) -> YaoshiResult<()> {
    let mut entries = Vec::new();
    collect_tar_entries(root, root, &mut entries)?;
    entries.sort();
    if let Some(parent) = tar.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::build(format!("create tar parent: {e}")))?;
    }
    let mut file = File::create(tar).map_err(|e| YaoshiError::build(format!("create tar: {e}")))?;
    for rel in entries {
        write_tar_entry(root, &rel, &mut file)?;
    }
    file.write_all(&[0u8; 1024])
        .and_then(|_| file.sync_all())
        .map_err(|e| YaoshiError::build(format!("finalize tar: {e}")))?;
    Ok(())
}

fn collect_tar_entries(root: &Path, current: &Path, out: &mut Vec<PathBuf>) -> YaoshiResult<()> {
    for entry in fs::read_dir(current)
        .map_err(|e| YaoshiError::build(format!("read tar tree {}: {e}", current.display())))?
    {
        let entry = entry.map_err(|e| YaoshiError::build(format!("read tar entry: {e}")))?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map_err(|e| YaoshiError::internal(format!("strip tar path: {e}")))?
            .to_path_buf();
        out.push(rel.clone());
        if entry
            .file_type()
            .map_err(|e| YaoshiError::build(format!("read tar type: {e}")))?
            .is_dir()
        {
            collect_tar_entries(root, &path, out)?;
        }
    }
    Ok(())
}

fn write_tar_entry(root: &Path, rel: &Path, out: &mut File) -> YaoshiResult<()> {
    let path = root.join(rel);
    let meta = fs::symlink_metadata(&path)
        .map_err(|e| YaoshiError::build(format!("stat tar entry {}: {e}", path.display())))?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let name = if meta.is_dir() && !rel_str.ends_with('/') {
        format!("{rel_str}/")
    } else {
        rel_str
    };
    let (typeflag, size, linkname) = if meta.is_dir() {
        (b'5', 0, String::new())
    } else if meta.file_type().is_symlink() {
        (
            b'2',
            0,
            fs::read_link(&path)
                .map_err(|e| YaoshiError::build(format!("read tar symlink: {e}")))?
                .to_string_lossy()
                .into_owned(),
        )
    } else if meta.is_file() {
        (b'0', meta.len(), String::new())
    } else {
        return Err(YaoshiError::build("unsupported overlay tar entry type"));
    };
    let mode = meta.permissions().mode() & 0o7777;
    let mut header = [0u8; 512];
    tar_write_path(&mut header, &name)?;
    tar_write_octal(&mut header[100..108], mode as u64)?;
    tar_write_octal(&mut header[108..116], 0)?;
    tar_write_octal(&mut header[116..124], 0)?;
    tar_write_octal(&mut header[124..136], size)?;
    tar_write_octal(&mut header[136..148], SOURCE_DATE_EPOCH)?;
    header[148..156].fill(b' ');
    header[156] = typeflag;
    tar_write_string(&mut header[157..257], &linkname)?;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: u32 = header.iter().map(|b| u32::from(*b)).sum();
    tar_write_checksum(&mut header[148..156], checksum);
    out.write_all(&header)
        .map_err(|e| YaoshiError::build(format!("write tar header: {e}")))?;
    if meta.is_file() {
        let mut input =
            File::open(&path).map_err(|e| YaoshiError::build(format!("open tar file: {e}")))?;
        std::io::copy(&mut input, out)
            .map_err(|e| YaoshiError::build(format!("write tar file body: {e}")))?;
        let pad = round_up(size, 512) - size;
        if pad > 0 {
            out.write_all(&vec![0u8; pad as usize])
                .map_err(|e| YaoshiError::build(format!("write tar padding: {e}")))?;
        }
    }
    Ok(())
}

fn tar_write_path(header: &mut [u8; 512], value: &str) -> YaoshiResult<()> {
    if value.len() <= 100 {
        tar_write_string(&mut header[0..100], value)?;
        return Ok(());
    }
    let mut split = None;
    for (idx, ch) in value.char_indices() {
        if ch != '/' {
            continue;
        }
        let prefix = &value[..idx];
        let name = &value[idx + 1..];
        if !prefix.is_empty()
            && !name.is_empty()
            && prefix.len() <= 155
            && name.len() <= 100
        {
            split = Some((prefix, name));
        }
    }
    let Some((prefix, name)) = split else {
        return Err(YaoshiError::build(format!("tar path too long: {value}")));
    };
    tar_write_string(&mut header[0..100], name)?;
    tar_write_string(&mut header[345..500], prefix)
}

fn tar_write_string(field: &mut [u8], value: &str) -> YaoshiResult<()> {
    let bytes = value.as_bytes();
    if bytes.len() > field.len() {
        return Err(YaoshiError::build(format!("tar path too long: {value}")));
    }
    field.fill(0);
    field[..bytes.len()].copy_from_slice(bytes);
    Ok(())
}

fn tar_write_octal(field: &mut [u8], value: u64) -> YaoshiResult<()> {
    let width = field.len();
    let text = format!("{value:0width$o}", width = width - 1);
    if text.len() > width - 1 {
        return Err(YaoshiError::build("tar octal field overflows"));
    }
    field.fill(0);
    field[..text.len()].copy_from_slice(text.as_bytes());
    Ok(())
}

fn tar_write_checksum(field: &mut [u8], value: u32) {
    let text = format!("{value:06o}\0 ");
    field.copy_from_slice(text.as_bytes());
}

fn create_dir_mode(path: &Path, mode: u32) -> YaoshiResult<()> {
    fs::create_dir_all(path)
        .map_err(|e| YaoshiError::build(format!("create directory {}: {e}", path.display())))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| YaoshiError::build(format!("chmod directory {}: {e}", path.display())))
}

fn write_exec(path: &Path, text: &str) -> YaoshiResult<()> {
    write_bytes(path, text.as_bytes(), 0o755)
}

fn write_bytes(path: &Path, bytes: &[u8], mode: u32) -> YaoshiResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::build(format!("create parent {}: {e}", parent.display())))?;
    }
    let mut file =
        File::create(path).map_err(|e| YaoshiError::build(format!("write file: {e}")))?;
    file.write_all(bytes)
        .map_err(|e| YaoshiError::build(format!("write file bytes: {e}")))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| YaoshiError::build(format!("chmod file: {e}")))?;
    Ok(())
}

fn ensure_bootstrap_dir(path: &Path) -> YaoshiResult<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(YaoshiError::config(format!(
            "bootstrap path is not a directory: {}",
            path.display()
        ))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)
            .map_err(|e| YaoshiError::config(format!("create bootstrap directory: {e}"))),
        Err(err) => Err(YaoshiError::config(format!(
            "stat bootstrap directory {}: {err}",
            path.display()
        ))),
    }
}

fn bootstrap_rendered_file(path: &Path, text: String) -> YaoshiResult<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => return Ok(()),
        Ok(_) => {
            return Err(YaoshiError::config(format!(
                "bootstrap path is not a regular file: {}",
                path.display()
            )));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(YaoshiError::config(format!(
                "stat bootstrap file {}: {err}",
                path.display()
            )));
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| YaoshiError::config("bootstrap file has no parent directory"))?;
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| YaoshiError::config("bootstrap file name is not UTF-8"))?;
    let tmp = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let result = (|| -> YaoshiResult<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| YaoshiError::config(format!("create bootstrap temp file: {e}")))?;
        file.write_all(text.as_bytes())
            .map_err(|e| YaoshiError::config(format!("write bootstrap temp file: {e}")))?;
        file.set_permissions(fs::Permissions::from_mode(0o644))
            .map_err(|e| YaoshiError::config(format!("chmod bootstrap temp file: {e}")))?;
        file.sync_all()
            .map_err(|e| YaoshiError::config(format!("fsync bootstrap temp file: {e}")))?;
        drop(file);
        fs::rename(&tmp, path)
            .map_err(|e| YaoshiError::config(format!("install bootstrap file: {e}")))?;
        fsync_config_dir(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn fsync_config_dir(path: &Path) -> YaoshiResult<()> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| YaoshiError::config(format!("fsync directory {}: {e}", path.display())))
}

fn symlink_force(target: &str, link: &Path) -> YaoshiResult<()> {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::build(format!("create symlink parent: {e}")))?;
    }
    let _ = fs::remove_file(link);
    symlink(target, link).map_err(|e| YaoshiError::build(format!("create symlink: {e}")))
}

fn reset_dir(path: &Path) -> YaoshiResult<()> {
    if path.exists() {
        fs::remove_dir_all(path)
            .map_err(|e| YaoshiError::build(format!("reset directory {}: {e}", path.display())))?;
    }
    fs::create_dir_all(path)
        .map_err(|e| YaoshiError::build(format!("create directory {}: {e}", path.display())))
}

fn extract_tar_to_tree(tar: &Path, tree: &Path) -> YaoshiResult<()> {
    reset_dir(tree)?;
    let status = Command::new("tar")
        .arg("--numeric-owner")
        .arg("--no-same-owner")
        .arg("--no-same-permissions")
        .arg("--exclude=./dev/*")
        .arg("--exclude=dev/*")
        .arg("-C")
        .arg(tree)
        .arg("-xf")
        .arg(tar)
        .status()
        .map_err(|e| YaoshiError::build(format!("invoke tar extract: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(YaoshiError::build("extract root.tar failed"))
    }
}

fn cleanup_build_identity(root: &Path) -> YaoshiResult<()> {
    for entry in fs::read_dir(root.join("etc/ssh")).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("ssh_host_") && (name.ends_with("_key") || name.ends_with("_key.pub")) {
            let _ = fs::remove_file(entry.path());
        }
    }
    write_bytes(&root.join("etc/machine-id"), b"", 0o644)?;
    let _ = fs::remove_file(root.join("var/lib/dbus/machine-id"));
    Ok(())
}

fn run_root_export_helper(helper: &Path, root: &Path, export_out: &Path) -> YaoshiResult<()> {
    let _ = fs::remove_dir_all(export_out);
    let status = Command::new(helper)
        .arg(root)
        .status()
        .map_err(|e| YaoshiError::build(format!("invoke root export helper: {e}")))?;
    if !status.success() {
        return Err(YaoshiError::build("root export helper failed"));
    }
    let export = root.join("YAOSHI-EXPORT");
    copy_dir_recursive(&export, export_out)?;
    fs::remove_dir_all(export)
        .map_err(|e| YaoshiError::build(format!("remove root export staging: {e}")))
}

fn merge_tree_over(dst_root: &Path, src_root: &Path) -> YaoshiResult<()> {
    for entry in fs::read_dir(src_root)
        .map_err(|e| YaoshiError::build(format!("read overlay tree {}: {e}", src_root.display())))?
    {
        let entry = entry.map_err(|e| YaoshiError::build(format!("read overlay entry: {e}")))?;
        let src = entry.path();
        let dst = dst_root.join(entry.file_name());
        let meta = fs::symlink_metadata(&src)
            .map_err(|e| YaoshiError::build(format!("stat overlay entry: {e}")))?;
        if meta.is_dir() {
            fs::create_dir_all(&dst)
                .map_err(|e| YaoshiError::build(format!("create merged directory: {e}")))?;
            merge_tree_over(&dst, &src)?;
            fs::set_permissions(&dst, fs::Permissions::from_mode(meta.permissions().mode()))
                .map_err(|e| YaoshiError::build(format!("chmod merged directory: {e}")))?;
        } else {
            let _ = fs::remove_file(&dst);
            if dst.is_dir() {
                fs::remove_dir_all(&dst)
                    .map_err(|e| YaoshiError::build(format!("remove merge target dir: {e}")))?;
            }
            if meta.file_type().is_symlink() {
                let target = fs::read_link(&src)
                    .map_err(|e| YaoshiError::build(format!("read overlay symlink: {e}")))?;
                symlink_force(&target.to_string_lossy(), &dst)?;
            } else if meta.is_file() {
                if let Some(parent) = dst.parent() {
                    fs::create_dir_all(parent).map_err(|e| {
                        YaoshiError::build(format!("create merged file parent: {e}"))
                    })?;
                }
                fs::copy(&src, &dst)
                    .map_err(|e| YaoshiError::build(format!("copy overlay file: {e}")))?;
                fs::set_permissions(&dst, fs::Permissions::from_mode(meta.permissions().mode()))
                    .map_err(|e| YaoshiError::build(format!("chmod overlay file: {e}")))?;
            } else {
                return Err(YaoshiError::build("unsupported overlay entry type"));
            }
        }
    }
    Ok(())
}

fn enforce_customized_root_predicates(debian: &Path, customized: &Path) -> YaoshiResult<()> {
    if fs::read(debian.join("packages.index"))
        .map_err(|e| YaoshiError::build(format!("read Debian packages.index: {e}")))?
        != fs::read(customized.join("packages.index"))
            .map_err(|e| YaoshiError::build(format!("read customized packages.index: {e}")))?
    {
        return Err(YaoshiError::build(
            "customized-root packages.index differs from Debian package-root",
        ));
    }
    let debian_boot = yaoshi_debian::DebianBootArtifacts::discover(&debian.join("boot"))?;
    let customized_boot = yaoshi_debian::DebianBootArtifacts::discover(&customized.join("boot"))?;
    if debian_boot.kernel_release != customized_boot.kernel_release {
        return Err(YaoshiError::build(
            "customized-root selected kernel release differs from Debian package-root",
        ));
    }
    for (name, left, right) in [
        ("boot/vmlinuz", debian_boot.vmlinuz, customized_boot.vmlinuz),
        ("boot/initrd.img", debian_boot.initrd_img, customized_boot.initrd_img),
        ("boot/config", debian_boot.config, customized_boot.config),
        (
            "boot/systemd-bootx64.efi.signed",
            debian_boot.systemd_boot_efi,
            customized_boot.systemd_boot_efi,
        ),
    ] {
        if sha256_file_hex(&left)? != sha256_file_hex(&right)? {
            return Err(YaoshiError::build(format!(
                "customized-root {name} differs from Debian package-root"
            )));
        }
    }
    if tree_sha256_hex(&debian_boot.modules)? != tree_sha256_hex(&customized_boot.modules)? {
        return Err(YaoshiError::build(
            "customized-root module tree differs from Debian package-root",
        ));
    }
    Ok(())
}

fn absolute_repo_root(repo: &Path) -> YaoshiResult<PathBuf> {
    if repo.is_absolute() {
        Ok(repo.to_path_buf())
    } else {
        std::env::current_dir()
            .map_err(|e| YaoshiError::internal(format!("resolve current directory: {e}")))?
            .join(repo)
            .canonicalize()
            .map_err(|e| YaoshiError::config(format!("canonicalize repository root: {e}")))
    }
}

fn absolute_path(path: &Path) -> YaoshiResult<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map_err(|e| YaoshiError::internal(format!("resolve current directory: {e}")))
            .map(|cwd| cwd.join(path))
    }
}

fn resolve_path(repo: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo.join(path)
    }
}

fn shell_single_quote(path: &Path) -> String {
    let raw = path.as_os_str().to_string_lossy();
    let mut out = String::from("'");
    for ch in raw.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn fsync_publish_dir(path: &Path) -> YaoshiResult<()> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| {
            YaoshiError::publish(format!("fsync publish directory {}: {e}", path.display()))
        })
}

fn module_output_path(module: &str) -> String {
    module
        .strip_suffix(".ko.xz")
        .map(|prefix| format!("{prefix}.ko"))
        .or_else(|| {
            module
                .strip_suffix(".ko.zst")
                .map(|prefix| format!("{prefix}.ko"))
        })
        .unwrap_or_else(|| module.to_string())
}

#[derive(Clone, Copy)]
struct InstallerModuleSeed {
    name: &'static str,
    required: bool,
}

const INSTALLER_MODULE_SEEDS: &[InstallerModuleSeed] = &[
    required_module_seed("xhci_pci"),
    required_module_seed("xhci_hcd"),
    optional_module_seed("ehci_pci"),
    optional_module_seed("ehci_hcd"),
    optional_module_seed("uhci_hcd"),
    optional_module_seed("ohci_pci"),
    optional_module_seed("ohci_hcd"),
    required_module_seed("hid_generic"),
    required_module_seed("usbhid"),
    required_module_seed("virtio_pci"),
    required_module_seed("virtio_blk"),
    required_module_seed("virtio_scsi"),
    required_module_seed("nvme"),
    required_module_seed("sd_mod"),
    required_module_seed("ahci"),
    required_module_seed("usb_storage"),
    required_module_seed("uas"),
];

const fn required_module_seed(name: &'static str) -> InstallerModuleSeed {
    InstallerModuleSeed {
        name,
        required: true,
    }
}

const fn optional_module_seed(name: &'static str) -> InstallerModuleSeed {
    InstallerModuleSeed {
        name,
        required: false,
    }
}

fn normalized_module_name(path: &str) -> String {
    let basename = path.rsplit('/').next().unwrap_or(path);
    basename
        .strip_suffix(".ko.xz")
        .or_else(|| basename.strip_suffix(".ko.zst"))
        .or_else(|| basename.strip_suffix(".ko"))
        .unwrap_or(basename)
        .replace('-', "_")
}

fn include_module(
    module: &str,
    by_path: &BTreeMap<String, Vec<String>>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<String>,
) -> YaoshiResult<()> {
    if seen.contains(module) {
        return Ok(());
    }
    let deps = by_path
        .get(module)
        .ok_or_else(|| YaoshiError::build(format!("module dependency missing: {module}")))?;
    for dep in deps {
        include_module(dep, by_path, seen, out)?;
    }
    seen.insert(module.to_string());
    out.push(module.to_string());
    Ok(())
}

fn reject_forbidden_installer_module(module: &str) -> YaoshiResult<()> {
    for forbidden in [
        "/kernel/net/",
        "/kernel/fs/",
        "/kernel/sound/",
        "/kernel/drivers/gpu/",
        "/kernel/drivers/net/",
        "/kernel/drivers/media/",
        "/kernel/drivers/bluetooth/",
        "/kernel/drivers/wireless/",
    ] {
        let with_leading = format!("/{module}");
        if with_leading.contains(forbidden) {
            return Err(YaoshiError::build(format!(
                "installer module closure includes forbidden module path: {module}"
            )));
        }
    }
    Ok(())
}
