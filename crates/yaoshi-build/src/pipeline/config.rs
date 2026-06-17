#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    system: Option<RawSystem>,
    sources: Option<RawSources>,
    root: Option<RawRoot>,
    installed: Option<RawInstalled>,
    scripts: Option<RawScripts>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSystem {
    hostname: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSources {
    debian: Option<String>,
    debian_security: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoot {
    ssh_public_key_files: Option<Vec<PathBuf>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInstalled {
    packages: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScripts {
    build_system: Option<PathBuf>,
    first_boot: Option<PathBuf>,
}

pub fn load_config(repo: &Path) -> YaoshiResult<EffectiveProductConfig> {
    let path = repo.join(CONFIG_PATH);
    if !path.is_file() {
        return Err(YaoshiError::config(format!(
            "missing product configuration {}",
            CONFIG_PATH
        )));
    }
    let text = fs::read_to_string(&path)
        .map_err(|e| YaoshiError::config(format!("read configuration: {e}")))?;
    let raw: RawConfig = toml::from_str(&text)
        .map_err(|e| YaoshiError::config(format!("parse configuration: {e}")))?;
    let root = raw
        .root
        .ok_or_else(|| YaoshiError::config("missing [root] table"))?;
    let key_files = root
        .ssh_public_key_files
        .ok_or_else(|| YaoshiError::config("root.ssh_public_key_files is required"))?;
    if key_files.is_empty() {
        return Err(YaoshiError::config(
            "root.ssh_public_key_files must be non-empty",
        ));
    }
    let hostname = raw
        .system
        .and_then(|s| s.hostname)
        .unwrap_or_else(|| "yaoshi".to_string());
    validate_hostname(&hostname)?;
    let sources = raw.sources.unwrap_or(RawSources {
        debian: None,
        debian_security: None,
    });
    let debian_mirror = validate_http_url(
        &sources
            .debian
            .unwrap_or_else(|| DEFAULT_DEBIAN_MIRROR.to_string()),
        "sources.debian",
    )?;
    let debian_security_mirror = validate_http_url(
        &sources
            .debian_security
            .unwrap_or_else(|| DEFAULT_DEBIAN_SECURITY_MIRROR.to_string()),
        "sources.debian_security",
    )?;
    let ssh_authorized_keys = read_authorized_keys(repo, &key_files)?;
    let installed_packages = validate_installed_packages(
        raw.installed
            .and_then(|installed| installed.packages)
            .unwrap_or_else(|| {
                DEFAULT_INSTALLED_PACKAGES
                    .iter()
                    .map(|package| (*package).to_string())
                    .collect()
            }),
    )?;
    let scripts = raw.scripts.unwrap_or(RawScripts {
        build_system: None,
        first_boot: None,
    });
    let build_system_script_bytes = read_configured_script(
        repo,
        scripts.build_system.as_deref(),
        "scripts.build_system",
    )?;
    let first_boot_script_bytes =
        read_configured_script(repo, scripts.first_boot.as_deref(), "scripts.first_boot")?;
    Ok(EffectiveProductConfig {
        hostname,
        debian_mirror,
        debian_security_mirror,
        ssh_authorized_keys,
        installed_packages,
        build_system_script_bytes,
        first_boot_script_bytes,
    })
}

pub fn current_image_source_fingerprint(repo: &Path) -> YaoshiResult<String> {
    let repo_root = absolute_repo_root(repo)?;
    let config = load_config(&repo_root)?;
    Ok(current_image_fingerprints(&repo_root, &config)?.published_image)
}

fn current_image_fingerprints(
    repo: &Path,
    config: &EffectiveProductConfig,
) -> YaoshiResult<CurrentImageFingerprints> {
    let foundation = source_fingerprint_for_domain(repo, config, "foundation", &[
        "yaoshi-common",
        "yaoshi-debian",
        "yaoshi-build",
    ])?;
    let installed_root = source_fingerprint_for_domain(repo, config, "installed-root", &[
        "yaoshi-common",
        "yaoshi-build",
    ])?;
    let installed_runtime = source_fingerprint_for_domain(repo, config, "installed-runtime", &[
        "yaoshi-common",
        "yaoshi-image",
        "yaoshi-payload",
        "yaoshi-screen",
        "yaoshi-dashboard",
        "yaoshi-build",
    ])?;
    let installer_envelope = source_fingerprint_for_domain(repo, config, "installer-envelope", &[
        "yaoshi-common",
        "yaoshi-image",
        "yaoshi-initramfs",
        "yaoshi-payload",
        "yaoshi-screen",
        "yaoshi-installer",
        "yaoshi-build",
    ])?;
    let published_image = sha256_bytes(
        canonical_json_bytes(&json!({
            "foundation-fingerprint": foundation,
            "installed-root-fingerprint": installed_root,
            "installed-runtime-fingerprint": installed_runtime,
            "installer-envelope-fingerprint": installer_envelope,
            "final_mbr_image_grammar": FINAL_INSTALLER_GRAMMAR,
            "composite_file_grammar": COMPOSITE_FILE_GRAMMAR,
            "leading_gap_bytes": LEADING_GAP_BYTES,
            "trailing_gap_bytes": TRAILING_GAP_BYTES,
            "logical_sector_size": LOGICAL_SECTOR_SIZE,
        }))?
        .as_slice(),
    );
    Ok(CurrentImageFingerprints {
        foundation,
        installed_root,
        installed_runtime,
        installer_envelope,
        published_image,
    })
}

fn source_fingerprint_for_domain(
    repo: &Path,
    config: &EffectiveProductConfig,
    domain: &str,
    crate_names: &[&str],
) -> YaoshiResult<String> {
    let mut hasher = Sha256::new();
    hash_named_bytes(&mut hasher, "stamp_domain", domain.as_bytes());
    hash_named_bytes(&mut hasher, "version", VERSION.as_bytes());
    if matches!(domain, "installed-root" | "installed-runtime") {
        hash_named_bytes(
            &mut hasher,
            "root_overlay_source",
            root_overlay_source_bytes()?.as_bytes(),
        );
        hash_template_group_source(
            &mut hasher,
            "askama_root_overlay_template_source",
            templates::TemplateGroup::RootOverlay,
        );
        hash_named_bytes(
            &mut hasher,
            "build_system_script_bytes",
            &config.build_system_script_bytes,
        );
        hash_named_bytes(
            &mut hasher,
            "build_system_assets_tree",
            &canonical_json_bytes(&build_system_assets_manifest_value(repo)?)?,
        );
    }
    if domain == "installed-runtime" {
        hash_template_group_source(
            &mut hasher,
            "askama_installed_esp_template_source",
            templates::TemplateGroup::InstalledEsp,
        );
        hash_template_group_source(
            &mut hasher,
            "askama_config_bootstrap_template_source",
            templates::TemplateGroup::ConfigBootstrap,
        );
        hash_named_bytes(
            &mut hasher,
            "askama_config_bootstrap_context_grammar",
            templates::CONFIG_BOOTSTRAP_CONTEXT_GRAMMAR.as_bytes(),
        );
        hash_named_bytes(
            &mut hasher,
            "esp_runtime_prepare_source",
            render_build_template(
                templates::render_esp_prepare(),
                "esp/runtime/PREPARE.askama",
            )?
            .as_bytes(),
        );
        hash_named_bytes(
            &mut hasher,
            "first_boot_launcher_source",
            render_build_template(
                templates::render_overlay_first_boot_launcher_sh(),
                "overlay/first-boot-launcher.sh.askama",
            )?
            .as_bytes(),
        );
        hash_named_bytes(
            &mut hasher,
            "first_boot_service_source",
            render_build_template(
                templates::render_overlay_yaoshi_first_boot_service(),
                "overlay/systemd/yaoshi-first-boot.service.askama",
            )?
            .as_bytes(),
        );
        hash_named_bytes(
            &mut hasher,
            "first_boot_script_bytes",
            &config.first_boot_script_bytes,
        );
        for path in INSTALLED_ESP_REQUIRED_FILE_SET {
            hash_named_bytes(&mut hasher, "installed_esp_required_file", path.as_bytes());
        }
        hash_named_bytes(&mut hasher, "hostname", config.hostname.as_bytes());
        for key in &config.ssh_authorized_keys {
            hash_named_bytes(&mut hasher, "effective_ssh_key", key.as_bytes());
        }
        for path in configured_ssh_key_files(repo)? {
            let bytes = fs::read(&path).map_err(|e| {
                YaoshiError::config(format!("read SSH key fingerprint input: {e}"))
            })?;
            let name = format!(
                "ssh-file:{}",
                path.strip_prefix(repo)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/")
            );
            hash_named_bytes(&mut hasher, &name, &bytes);
        }
    }
    if domain == "installer-envelope" {
        hash_template_group_source(
            &mut hasher,
            "askama_installer_boot_template_source",
            templates::TemplateGroup::InstallerBoot,
        );
    }
    for (name, value) in [
        ("debian_package_root_grammar", DEBIAN_PACKAGE_ROOT_GRAMMAR),
        ("customized_root_grammar", CUSTOMIZED_ROOT_GRAMMAR),
        ("root_bridge_overlay_grammar", ROOT_BRIDGE_OVERLAY_GRAMMAR),
        ("installed_root_source_grammar", INSTALLED_ROOT_SOURCE_GRAMMAR),
        ("installed_root_ext4_grammar", INSTALLED_ROOT_EXT4_GRAMMAR),
        ("runtime_binary_grammar", RUNTIME_BINARY_GRAMMAR),
        ("fat32_grammar", FAT32_GRAMMAR),
        ("payload_grammar", PAYLOAD_GRAMMAR),
        ("virtual_target_graph_grammar", VIRTUAL_TARGET_GRAPH_GRAMMAR),
        ("final_installer_grammar", FINAL_INSTALLER_GRAMMAR),
        ("composite_file_grammar", COMPOSITE_FILE_GRAMMAR),
        (
            "initramfs_zstd_compression_level",
            &INITRAMFS_ZSTD_COMPRESSION_LEVEL.to_string(),
        ),
        (
            "payload_zstd_compression_level",
            &PAYLOAD_ZSTD_COMPRESSION_LEVEL.to_string(),
        ),
        ("zstd_window_log", &ZSTD_WINDOW_LOG.to_string()),
        (
            "payload_extent_max",
            &PAYLOAD_EXTENT_MAX_UNCOMPRESSED_BYTES.to_string(),
        ),
        (
            "payload_alignment",
            &PAYLOAD_CONTAINER_ALIGNMENT_BYTES.to_string(),
        ),
        (
            "installer_boot_size",
            &INSTALLER_BOOT_SIZE_BYTES.to_string(),
        ),
        ("installed_esp_size", &INSTALLED_ESP_SIZE_BYTES.to_string()),
    ] {
        hash_named_bytes(&mut hasher, name, value.as_bytes());
    }
    for package in effective_package_include_list(config) {
        hash_named_bytes(&mut hasher, "effective_debian_package", package.as_bytes());
    }
    for rel in product_fingerprint_files(repo, crate_names)? {
        let path = repo.join(&rel);
        let bytes = fs::read(&path).map_err(|e| {
            YaoshiError::build(format!(
                "read source fingerprint input {}: {e}",
                rel.display()
            ))
        })?;
        hash_named_bytes(
            &mut hasher,
            &rel.to_string_lossy().replace('\\', "/"),
            &bytes,
        );
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

fn root_overlay_source_bytes() -> YaoshiResult<String> {
    Ok([
        render_build_template(
            templates::render_overlay_prepare_launcher_sh(),
            "overlay/prepare-launcher.sh.askama",
        ),
        render_build_template(
            templates::render_overlay_first_boot_launcher_sh(),
            "overlay/first-boot-launcher.sh.askama",
        ),
        render_build_template(templates::render_overlay_fstab(), "overlay/fstab.askama"),
        render_build_template(
            templates::render_overlay_hostname(),
            "overlay/hostname.askama",
        ),
        render_build_template(
            templates::render_overlay_network_dhcp(),
            "overlay/network/20-yaoshi-dhcp.network.askama",
        ),
        render_build_template(
            templates::render_overlay_yaoshi_dashboard_service(),
            "overlay/systemd/yaoshi-dashboard.service.askama",
        ),
        render_build_template(
            templates::render_overlay_yaoshi_root_shell_service(),
            "overlay/systemd/yaoshi-root-shell.service.askama",
        ),
        render_build_template(
            templates::render_overlay_yaoshi_prepare_service(),
            "overlay/systemd/yaoshi-prepare.service.askama",
        ),
        render_build_template(
            templates::render_overlay_yaoshi_first_boot_service(),
            "overlay/systemd/yaoshi-first-boot.service.askama",
        ),
        render_build_template(
            templates::render_overlay_ssh_order_conf(),
            "overlay/systemd/ssh-order.conf.askama",
        ),
        render_build_template(
            templates::render_overlay_sshd_config(),
            "overlay/ssh/10-yaoshi.conf.askama",
        ),
        render_build_template(
            templates::render_overlay_repart_root(),
            "overlay/repart/20-yaoshi-root.conf.askama",
        ),
        Ok("usr/bin/yaoshi-dashboard -> /boot/YAOSHI/DASHBOARD/YAOSHI-DASHBOARD".to_string()),
        Ok("etc/resolv.conf -> /run/systemd/resolve/stub-resolv.conf".to_string()),
        Ok("multi-user.target.wants/systemd-networkd.service -> /lib/systemd/system/systemd-networkd.service".to_string()),
        Ok("multi-user.target.wants/systemd-resolved.service -> /lib/systemd/system/systemd-resolved.service".to_string()),
        Ok("multi-user.target.wants/ssh.service -> /lib/systemd/system/ssh.service".to_string()),
    ]
    .into_iter()
    .collect::<YaoshiResult<Vec<_>>>()?
    .join("\n--yaoshi-overlay-source-boundary--\n"))
}

fn render_build_template(
    rendered: Result<String, askama::Error>,
    name: &'static str,
) -> YaoshiResult<String> {
    rendered.map_err(|e| YaoshiError::build(format!("render {name}: {e}")))
}

fn template_group_source_digest(group: templates::TemplateGroup) -> String {
    let mut hasher = Sha256::new();
    for (name, bytes) in templates::template_source_files(group) {
        hash_named_bytes(&mut hasher, name, bytes);
    }
    hex_digest(hasher.finalize().as_slice())
}

fn hash_template_group_source(
    hasher: &mut Sha256,
    group_name: &str,
    group: templates::TemplateGroup,
) {
    for (name, bytes) in templates::template_source_files(group) {
        let field = format!("{group_name}:{name}");
        hash_named_bytes(hasher, &field, bytes);
    }
}

fn build_system_assets_manifest_value(repo: &Path) -> YaoshiResult<Value> {
    let root = repo.join(BUILD_SYSTEM_ASSETS_PATH);
    if !root.exists() {
        return Ok(json!({
            "state": "absent",
            "path": BUILD_SYSTEM_ASSETS_PATH,
        }));
    }
    if !root.is_dir() {
        return Err(YaoshiError::config(format!(
            "{BUILD_SYSTEM_ASSETS_PATH} must be a directory when present"
        )));
    }
    let tree = read_host_tree(&root)?;
    Ok(json!({
        "state": "present",
        "path": BUILD_SYSTEM_ASSETS_PATH,
        "tree": fs_tree_storage_manifest_value(&tree.manifest)?,
    }))
}

fn hash_named_bytes(hasher: &mut Sha256, name: &str, bytes: &[u8]) {
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(bytes.len().to_string().as_bytes());
    hasher.update([0]);
    hasher.update(bytes);
    hasher.update([0]);
}

fn configured_ssh_key_files(repo: &Path) -> YaoshiResult<Vec<PathBuf>> {
    let text = fs::read_to_string(repo.join(CONFIG_PATH))
        .map_err(|e| YaoshiError::config(format!("read configuration: {e}")))?;
    let raw: RawConfig = toml::from_str(&text)
        .map_err(|e| YaoshiError::config(format!("parse configuration: {e}")))?;
    let root = raw
        .root
        .ok_or_else(|| YaoshiError::config("missing [root] table"))?;
    let key_files = root
        .ssh_public_key_files
        .ok_or_else(|| YaoshiError::config("root.ssh_public_key_files is required"))?;
    Ok(key_files
        .iter()
        .map(|path| resolve_path(repo, path))
        .collect())
}

fn product_fingerprint_files(repo: &Path, crate_names: &[&str]) -> YaoshiResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    for rel in [
        "Cargo.toml",
        "Cargo.lock",
        ".cargo/config.toml",
        ".yaoshi/yaoshi.toml",
    ] {
        let path = PathBuf::from(rel);
        if repo.join(&path).is_file() {
            files.push(path);
        }
    }
    for crate_name in crate_names {
        collect_fingerprint_files(repo, &repo.join("crates").join(crate_name), &mut files)?;
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_fingerprint_files(
    repo: &Path,
    path: &Path,
    files: &mut Vec<PathBuf>,
) -> YaoshiResult<()> {
    if !path.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path)
        .map_err(|e| YaoshiError::build(format!("read fingerprint dir {}: {e}", path.display())))?
    {
        let entry =
            entry.map_err(|e| YaoshiError::build(format!("read fingerprint entry: {e}")))?;
        let path = entry.path();
        let ty = entry
            .file_type()
            .map_err(|e| YaoshiError::build(format!("read fingerprint type: {e}")))?;
        if ty.is_dir() {
            collect_fingerprint_files(repo, &path, files)?;
        } else if ty.is_file() {
            files.push(
                path.strip_prefix(repo)
                    .map_err(|e| YaoshiError::internal(format!("strip fingerprint path: {e}")))?
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}

fn validate_hostname(hostname: &str) -> YaoshiResult<()> {
    let bytes = hostname.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 63
        || bytes[0] == b'-'
        || bytes[bytes.len() - 1] == b'-'
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
    {
        return Err(YaoshiError::config("invalid system.hostname"));
    }
    Ok(())
}

fn validate_http_url(raw: &str, field: &str) -> YaoshiResult<Url> {
    let url = Url::parse(raw)
        .map_err(|e| YaoshiError::config(format!("{field} must be an HTTP or HTTPS URL: {e}")))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(YaoshiError::config(format!(
            "{field} must be an HTTP or HTTPS URL"
        )));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(YaoshiError::config(format!("{field} must have a host")));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(YaoshiError::config(format!(
            "{field} must not contain query string or fragment"
        )));
    }
    Ok(url)
}

fn read_authorized_keys(repo: &Path, files: &[PathBuf]) -> YaoshiResult<Vec<String>> {
    let mut keys = Vec::new();
    let mut seen = BTreeSet::new();
    for rel in files {
        let path = resolve_path(repo, rel);
        if !path.is_file() {
            return Err(YaoshiError::config(format!(
                "SSH public key file is not a regular file: {}",
                path.display()
            )));
        }
        let text = fs::read_to_string(&path)
            .map_err(|e| YaoshiError::config(format!("read SSH public key file: {e}")))?;
        for line in text.lines() {
            let trimmed = line.trim_matches(|c: char| c.is_ascii_whitespace());
            if trimmed.is_empty() {
                continue;
            }
            ssh_key::PublicKey::from_openssh(trimmed)
                .map_err(|e| YaoshiError::config(format!("parse SSH public key: {e}")))?;
            if seen.insert(trimmed.to_string()) {
                keys.push(trimmed.to_string());
            }
        }
    }
    if keys.is_empty() {
        return Err(YaoshiError::config(
            "effective SSH authorized key set is empty",
        ));
    }
    Ok(keys)
}

fn validate_installed_packages(packages: Vec<String>) -> YaoshiResult<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for package in packages {
        if !valid_debian_package_name(&package) {
            return Err(YaoshiError::config(format!(
                "invalid installed package name: {package}"
            )));
        }
        if seen.insert(package.clone()) {
            out.push(package);
        }
    }
    Ok(out)
}

fn valid_debian_package_name(package: &str) -> bool {
    let bytes = package.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 128
        || !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit()
    {
        return false;
    }
    bytes[1..].iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'+' | b'.' | b'-')
    })
}

fn read_configured_script(repo: &Path, path: Option<&Path>, field: &str) -> YaoshiResult<Vec<u8>> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    if path.extension().and_then(|ext| ext.to_str()) != Some("sh") {
        return Err(YaoshiError::config(format!("{field} must end with .sh")));
    }
    let resolved = resolve_path(repo, path);
    let meta = fs::metadata(&resolved).map_err(|e| {
        YaoshiError::config(format!(
            "{field} must resolve to a regular file {}: {e}",
            resolved.display()
        ))
    })?;
    if !meta.is_file() {
        return Err(YaoshiError::config(format!(
            "{field} must resolve to a regular file: {}",
            resolved.display()
        )));
    }
    if meta.len() > 1_048_576 {
        return Err(YaoshiError::config(format!(
            "{field} script exceeds 1048576 bytes"
        )));
    }
    let bytes = fs::read(&resolved)
        .map_err(|e| YaoshiError::config(format!("read {field} script: {e}")))?;
    if bytes.contains(&0) {
        return Err(YaoshiError::config(format!(
            "{field} script must not contain NUL bytes"
        )));
    }
    Ok(bytes)
}

fn effective_package_include_list(config: &EffectiveProductConfig) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for package in DEBIAN_PACKAGES {
        seen.insert((*package).to_string());
        out.push((*package).to_string());
    }
    for package in &config.installed_packages {
        if seen.insert(package.clone()) {
            out.push(package.clone());
        }
    }
    out
}

fn hard_package_list() -> Vec<String> {
    DEBIAN_PACKAGES
        .iter()
        .map(|package| (*package).to_string())
        .collect()
}

fn check_environment(context: &BuildContext) -> YaoshiResult<()> {
    check_executable_command("cargo")?;
    check_writable_directory(&context.repo_root, "repository root")?;
    let state_parent = context
        .state_root
        .parent()
        .ok_or_else(|| YaoshiError::environment("resolve StateRoot parent"))?;
    check_writable_directory(state_parent, "StateRoot parent directory")
}

fn check_writable_directory(path: &Path, label: &str) -> YaoshiResult<()> {
    if !path.is_dir() {
        return Err(YaoshiError::environment(format!(
            "{label} is not a directory"
        )));
    }
    let probe = path.join(".yaoshi-write-check.tmp");
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe)
        .map_err(|e| YaoshiError::environment(format!("{label} is not writable: {e}")))?;
    file.write_all(b"yaoshi\n")
        .and_then(|_| file.sync_all())
        .map_err(|e| YaoshiError::environment(format!("{label} write check failed: {e}")))?;
    fs::remove_file(&probe)
        .map_err(|e| YaoshiError::environment(format!("remove {label} write check: {e}")))?;
    Ok(())
}

fn check_executable_command(cmd: &str) -> YaoshiResult<()> {
    let path = find_command(cmd)
        .ok_or_else(|| YaoshiError::environment(format!("required command not found: {cmd}")))?;
    let meta = fs::metadata(&path)
        .map_err(|e| YaoshiError::environment(format!("stat command {cmd}: {e}")))?;
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        return Err(YaoshiError::environment(format!(
            "required command is not executable: {cmd}"
        )));
    }
    Ok(())
}

fn find_command(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    find_command_in_path(name, &paths)
}

fn find_command_in_path(name: &str, paths: &OsStr) -> Option<PathBuf> {
    for dir in std::env::split_paths(paths) {
        let candidate = dir.join(name);
        let Ok(meta) = candidate.metadata() else {
            continue;
        };
        if meta.is_file() && meta.permissions().mode() & 0o111 != 0 {
            return Some(candidate);
        }
    }
    None
}

fn prepare_work_root(context: &BuildContext) -> YaoshiResult<()> {
    let paths = Paths::new(context);
    if paths.work.exists() {
        if !paths.work.is_dir() {
            return Err(YaoshiError::build(
                "work root exists and is not a directory",
            ));
        }
        fs::remove_dir_all(&paths.work)
            .map_err(|e| YaoshiError::build(format!("remove work root: {e}")))?;
    }
    for dir in [
        &paths.runtime,
        &paths.root_helper,
        &paths.debian_package_root,
        &paths.customized_root,
        &paths.root_bridge_overlay,
        paths.installed_root_source_file
            .parent()
            .ok_or_else(|| YaoshiError::internal("installed root source path has no parent"))?,
        &paths.installed_root_ext4,
        &paths.installer_modules,
        &paths.initramfs_base_tree,
        &paths.initramfs_app_tree,
        &paths.initramfs,
        &paths.installed_esp_tree,
        &paths.installer_boot_tree,
        &paths.image,
    ] {
        fs::create_dir_all(dir).map_err(|e| {
            YaoshiError::build(format!("create work directory {}: {e}", dir.display()))
        })?;
    }
    Ok(())
}

fn prepare_cache_root(context: &BuildContext) -> YaoshiResult<()> {
    let paths = Paths::new(context);
    for dir in [
        paths.cache.clone(),
        paths.cache.join("index/foundation"),
        paths.cache.join("index/root"),
        paths.cache.join("index/runtime"),
        paths.cache.join("index/image"),
        paths.cache.join("object/file"),
        paths.cache.join("object/tree"),
        paths.cache.join("object/composite-file"),
        paths.cache.join("extent/payload-encoding"),
        paths.debian_download_cache.clone(),
    ] {
        ensure_dir(&dir, "build")?;
    }
    Ok(())
}
