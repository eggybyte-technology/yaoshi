fn resolve_build_intents() -> YaoshiResult<()> {
    for invalid in [
        ".yaoshi/artifacts",
        ".yaoshi/work/kernel",
        ".yaoshi/work/installed-root-tree",
        ".yaoshi/work/installed-root-overlay-tree",
        ".yaoshi/work/image/installed-target.raw",
        ".yaoshi/work/image/installed-target.write-plan",
        ".yaoshi/work/image/installed-root.base.ext4",
        ".yaoshi/work/image/installed-root.composed.ext4",
        ".yaoshi/work/image/installed-root.minimized.ext4",
    ] {
        let path = Path::new(invalid);
        if path.exists() {
            return Err(YaoshiError::environment(format!(
                "invalid v0.0.1 generated path exists: {invalid}"
            )));
        }
    }
    Ok(())
}

fn resolve_cache_index_graph(context: &BuildContext) -> YaoshiResult<()> {
    prepare_cache_root(context)
}

fn resolve_root_branch(
    context: &BuildContext,
    config: &EffectiveProductConfig,
    debian_package_root: &Path,
) -> YaoshiResult<RootBundle> {
    build_installed_root_ext4_bundle(context, config, debian_package_root)
}

fn build_runtime_binary(context: &BuildContext, package: &str) -> YaoshiResult<RuntimeBinary> {
    let paths = Paths::new(context);
    let output = paths.runtime.join(package);
    let input = json!({
        "phase": format!("build-{package}-musl"),
        "grammar": RUNTIME_BINARY_GRAMMAR,
        "target": "x86_64-unknown-linux-musl",
        "profile": "debug",
        "cargo_toml": sha256_file_hex(&context.repo_root.join("Cargo.toml"))?,
        "cargo_lock": sha256_file_hex(&context.repo_root.join("Cargo.lock"))?,
        "cargo_config": sha256_file_hex(&context.repo_root.join(".cargo/config.toml"))?,
        "source": runtime_source_digest(&context.repo_root, package)?,
        "rustc": command_stdout_lossy("rustc", &["-Vv"])?,
        "cargo": command_stdout_lossy("cargo", &["-V"])?,
        "rustflags": std::env::var("RUSTFLAGS").unwrap_or_default(),
    });
    let phase = format!("build-{package}-musl");
    let object = cache_file_phase(context, &phase, input, &output, |out| {
        check_executable_command("sccache")?;
        check_executable_command("x86_64-unknown-linux-musl-gcc")?;
        let status = Command::new("cargo")
            .arg("build")
            .arg("--locked")
            .arg("--target")
            .arg("x86_64-unknown-linux-musl")
            .arg("-p")
            .arg(package)
            .current_dir(&context.repo_root)
            .status()
            .map_err(|e| YaoshiError::build(format!("invoke cargo for {package}: {e}")))?;
        if !status.success() {
            return Err(YaoshiError::build(format!("{package} musl build failed")));
        }
        let built = context
            .repo_root
            .join("target/x86_64-unknown-linux-musl/debug")
            .join(package);
        runtime_binary::check_elf64_static_x86_64(&built)?;
        fs::copy(&built, out)
            .map_err(|e| YaoshiError::build(format!("stage runtime binary {package}: {e}")))?;
        Ok(())
    })?;
    Ok(RuntimeBinary { path: object })
}

fn runtime_source_digest(repo: &Path, package: &str) -> YaoshiResult<String> {
    let crates = match package {
        "yaoshi-installer" => [
            "yaoshi-common",
            "yaoshi-image",
            "yaoshi-payload",
            "yaoshi-screen",
            "yaoshi-installer",
        ]
        .as_slice(),
        "yaoshi-dashboard" => ["yaoshi-common", "yaoshi-screen", "yaoshi-dashboard"].as_slice(),
        _ => return Err(YaoshiError::internal("unknown runtime package")),
    };
    let mut files = Vec::new();
    for krate in crates {
        collect_fingerprint_files(repo, &repo.join("crates").join(krate), &mut files)?;
    }
    files.sort();
    let mut hasher = Sha256::new();
    for rel in files {
        hash_named_bytes(
            &mut hasher,
            &rel.to_string_lossy(),
            &fs::read(repo.join(&rel)).map_err(|e| {
                YaoshiError::build(format!("read runtime source {}: {e}", rel.display()))
            })?,
        );
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

fn render_installed_overlay_tree(root: &Path) -> YaoshiResult<()> {
    create_dir_mode(&root.join("boot"), 0o755)?;
    symlink_force(
        "/boot/YAOSHI/DASHBOARD/YAOSHI-DASHBOARD",
        &root.join("usr/bin/yaoshi-dashboard"),
    )?;
    write_exec(
        &root.join("usr/lib/yaoshi/prepare-launcher"),
        &render_build_template(
            templates::render_overlay_prepare_launcher_sh(),
            "overlay/prepare-launcher.sh.askama",
        )?,
    )?;
    write_exec(
        &root.join("usr/lib/yaoshi/first-boot-launcher"),
        &render_build_template(
            templates::render_overlay_first_boot_launcher_sh(),
            "overlay/first-boot-launcher.sh.askama",
        )?,
    )?;
    write_bytes(
        &root.join("etc/hostname"),
        render_build_template(
            templates::render_overlay_hostname(),
            "overlay/hostname.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    symlink_force(
        "/run/systemd/resolve/stub-resolv.conf",
        &root.join("etc/resolv.conf"),
    )?;
    write_bytes(&root.join("etc/machine-id"), b"", 0o644)?;
    write_bytes(
        &root.join("etc/fstab"),
        render_build_template(templates::render_overlay_fstab(), "overlay/fstab.askama")?
            .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &root.join("etc/systemd/network/20-yaoshi-dhcp.network"),
        render_build_template(
            templates::render_overlay_network_dhcp(),
            "overlay/network/20-yaoshi-dhcp.network.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &root.join("etc/systemd/system/yaoshi-dashboard.service"),
        render_build_template(
            templates::render_overlay_yaoshi_dashboard_service(),
            "overlay/systemd/yaoshi-dashboard.service.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &root.join("etc/systemd/system/yaoshi-root-shell.service"),
        render_build_template(
            templates::render_overlay_yaoshi_root_shell_service(),
            "overlay/systemd/yaoshi-root-shell.service.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &root.join("etc/systemd/system/yaoshi-prepare.service"),
        render_build_template(
            templates::render_overlay_yaoshi_prepare_service(),
            "overlay/systemd/yaoshi-prepare.service.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &root.join("etc/systemd/system/yaoshi-first-boot.service"),
        render_build_template(
            templates::render_overlay_yaoshi_first_boot_service(),
            "overlay/systemd/yaoshi-first-boot.service.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    symlink_force(
        "/dev/null",
        &root.join("etc/systemd/system/getty@tty1.service"),
    )?;
    symlink_force(
        "/dev/null",
        &root.join("etc/systemd/system/getty@tty2.service"),
    )?;
    symlink_force(
        "/dev/null",
        &root.join("etc/systemd/system/getty-static.service"),
    )?;
    symlink_force(
        "/dev/null",
        &root.join("etc/systemd/system/serial-getty@ttyS0.service"),
    )?;
    write_bytes(
        &root.join("etc/systemd/system/ssh.service.d/10-yaoshi-order.conf"),
        render_build_template(
            templates::render_overlay_ssh_order_conf(),
            "overlay/systemd/ssh-order.conf.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &root.join("etc/ssh/sshd_config.d/10-yaoshi.conf"),
        render_build_template(
            templates::render_overlay_sshd_config(),
            "overlay/ssh/10-yaoshi.conf.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &root.join("usr/lib/yaoshi/repart.d/20-yaoshi-root.conf"),
        render_build_template(
            templates::render_overlay_repart_root(),
            "overlay/repart/20-yaoshi-root.conf.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    create_dir_mode(&root.join("root/.ssh"), 0o700)?;
    let wants = root.join("etc/systemd/system/multi-user.target.wants");
    create_dir_mode(&wants, 0o755)?;
    symlink_force(
        "../yaoshi-dashboard.service",
        &wants.join("yaoshi-dashboard.service"),
    )?;
    symlink_force(
        "../yaoshi-root-shell.service",
        &wants.join("yaoshi-root-shell.service"),
    )?;
    symlink_force(
        "../yaoshi-prepare.service",
        &wants.join("yaoshi-prepare.service"),
    )?;
    symlink_force(
        "../yaoshi-first-boot.service",
        &wants.join("yaoshi-first-boot.service"),
    )?;
    symlink_force(
        "/lib/systemd/system/systemd-networkd.service",
        &wants.join("systemd-networkd.service"),
    )?;
    symlink_force(
        "/lib/systemd/system/systemd-resolved.service",
        &wants.join("systemd-resolved.service"),
    )?;
    symlink_force(
        "/lib/systemd/system/ssh.service",
        &wants.join("ssh.service"),
    )?;
    Ok(())
}

fn build_installed_root_ext4_bundle(
    context: &BuildContext,
    config: &EffectiveProductConfig,
    debian_package_root: &Path,
) -> YaoshiResult<RootBundle> {
    let customized_root = resolve_customized_root(context, config, debian_package_root)?;
    let root_bridge_overlay = resolve_root_bridge_overlay(context)?;
    let installed_root_source =
        resolve_installed_root_source(context, &customized_root, &root_bridge_overlay)?;
    let installed_root_ext4 =
        resolve_installed_root_ext4(context, config, &installed_root_source, &customized_root)?;
    root_bundle_from_outputs(debian_package_root, &installed_root_ext4, config)
}

fn resolve_debian_package_root(
    context: &BuildContext,
    config: &EffectiveProductConfig,
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let package_include_list = effective_package_include_list(config);
    let input = json!({
        "phase": "resolve-debian-package-root",
        "grammar": DEBIAN_PACKAGE_ROOT_GRAMMAR,
        "suite": DEBIAN_SUITE,
        "security_suite": DEBIAN_SECURITY_SUITE,
        "architecture": DEBIAN_ARCH,
        "components": DEBIAN_COMPONENTS,
        "package_request_contract": "yaoshi.debian-package-request.v1",
        "package_include_list": package_include_list,
        "hard_package_list": hard_package_list(),
        "mmdebstrap_mode": "unshare",
        "mmdebstrap_format": "tar",
        "mmdebstrap_variant": "minbase",
        "apt_options": [
            MMDEBSTRAP_APT_RETRIES,
            MMDEBSTRAP_APT_HTTP_TIMEOUT,
            MMDEBSTRAP_APT_HTTPS_TIMEOUT,
            MMDEBSTRAP_APT_NO_RECOMMENDS,
            MMDEBSTRAP_APT_NO_SUGGESTS
        ],
        "package_index_contract": "yaoshi.packages-index.v1",
        "boot_export_contract": "yaoshi.debian-boot-export.v1",
        "root_tar_normalization_contract": "mmdebstrap-tar-with-yaoshi-export-cleanup.v1",
        "askama_debian_helper_template_source_digest": template_group_source_digest(templates::TemplateGroup::DebianHelper),
        "askama_debian_helper_context_grammar": templates::DEBIAN_HELPER_CONTEXT_GRAMMAR,
        "root_export_helper_digest": sha256_bytes(
            render_build_template(
                templates::render_debian_root_export_helper_sh(),
                "debian/root-export-helper.sh.askama",
            )?.as_bytes()
        ),
        "build_time_identity_cleanup_rule_version": BUILD_TIME_IDENTITY_CLEANUP_RULE_VERSION,
        "source_date_epoch": SOURCE_DATE_EPOCH,
    });
    cache_tree_phase(
        context,
        "resolve-debian-package-root",
        input,
        &paths.debian_package_root,
        |out| resolve_debian_package_root_miss(context, config, out),
    )
}

fn resolve_debian_package_root_miss(
    context: &BuildContext,
    config: &EffectiveProductConfig,
    out: &Path,
) -> YaoshiResult<()> {
    check_executable_command("mmdebstrap")?;
    let paths = Paths::new(context);
    let export_helper = write_root_export_helper(&paths.root_helper)?;
    fs::create_dir_all(&paths.debian_download_cache)
        .map_err(|e| YaoshiError::build(format!("create Debian archive cache: {e}")))?;
    fs::create_dir_all(out)
        .map_err(|e| YaoshiError::build(format!("create Debian package-root output: {e}")))?;
    let root_tar = out.join("root.tar");
    let export_out = paths.root_helper.join("root-export");
    let mmdebstrap_tmp = paths.root_helper.join("tmp");
    let _ = fs::remove_dir_all(&export_out);
    let _ = fs::remove_dir_all(&mmdebstrap_tmp);
    fs::create_dir_all(&export_out)
        .map_err(|e| YaoshiError::build(format!("create root export dir: {e}")))?;
    fs::create_dir_all(&mmdebstrap_tmp)
        .map_err(|e| YaoshiError::build(format!("create mmdebstrap temp dir: {e}")))?;
    let include = effective_package_include_list(config).join(",");
    let cache = absolute_path(&paths.debian_download_cache)?;
    let status = Command::new("mmdebstrap")
        .env("SOURCE_DATE_EPOCH", SOURCE_DATE_EPOCH.to_string())
        .env("TMPDIR", absolute_path(&mmdebstrap_tmp)?)
        .arg("--mode=unshare")
        .arg("--format=tar")
        .arg("--variant=minbase")
        .arg("--architectures=amd64")
        .arg("--components=main,non-free-firmware")
        .arg(format!("--include={include}"))
        .arg("--skip=essential/unlink")
        .arg(format!("--aptopt={MMDEBSTRAP_APT_RETRIES}"))
        .arg(format!("--aptopt={MMDEBSTRAP_APT_HTTP_TIMEOUT}"))
        .arg(format!("--aptopt={MMDEBSTRAP_APT_HTTPS_TIMEOUT}"))
        .arg(format!("--aptopt={MMDEBSTRAP_APT_NO_RECOMMENDS}"))
        .arg(format!("--aptopt={MMDEBSTRAP_APT_NO_SUGGESTS}"))
        .arg(format!(
            "--setup-hook=mkdir -p {} \"$1/var/cache/apt/archives\"",
            shell_single_quote(&cache)
        ))
        .arg(format!(
            "--setup-hook=sync-in {} /var/cache/apt/archives",
            shell_single_quote(&cache)
        ))
        .arg("--customize-hook=rm -f \"$1\"/etc/ssh/ssh_host_*_key \"$1\"/etc/ssh/ssh_host_*_key.pub; : > \"$1/etc/machine-id\"; rm -f \"$1/var/lib/dbus/machine-id\"")
        .arg(format!(
            "--customize-hook={} \"$1\"",
            shell_single_quote(&export_helper)
        ))
        .arg(format!(
            "--customize-hook=copy-out /YAOSHI-EXPORT {}",
            shell_single_quote(&export_out)
        ))
        .arg("--customize-hook=rm -rf \"$1/YAOSHI-EXPORT\"")
        .arg(format!(
            "--customize-hook=sync-out /var/cache/apt/archives {}",
            shell_single_quote(&cache)
        ))
        .arg(DEBIAN_SUITE)
        .arg(&root_tar)
        .arg(format!(
            "deb [arch={DEBIAN_ARCH}] {} {DEBIAN_SUITE} main non-free-firmware",
            config.debian_mirror.as_str()
        ))
        .arg(format!(
            "deb [arch={DEBIAN_ARCH}] {} {DEBIAN_SECURITY_SUITE} main non-free-firmware",
            config.debian_security_mirror.as_str()
        ))
        .status()
        .map_err(|e| YaoshiError::build(format!("invoke mmdebstrap: {e}")))?;
    if !status.success() {
        return Err(YaoshiError::build("mmdebstrap Debian package-root construction failed"));
    }
    let export = normalize_export_dir(&export_out)?;
    let dpkg_status = export.join("dpkg.status");
    let packages_index = packages_index_from_status(&dpkg_status)?;
    fs::write(out.join("packages.index"), packages_index.as_bytes())
        .map_err(|e| YaoshiError::build(format!("write packages.index: {e}")))?;
    let effective_packages = effective_package_include_list(config);
    if !package_manifest_is_valid(&out.join("packages.index"), &effective_packages)? {
        return Err(YaoshiError::build("generated packages.index is invalid"));
    }
    copy_dir_recursive(&export.join("boot"), &out.join("boot"))?;
    let boot = yaoshi_debian::DebianBootArtifacts::discover(&out.join("boot"))?;
    boot.validate()?;
    validate_debian_package_root_tree(out, &effective_packages)
}

fn validate_debian_package_root_tree(root: &Path, effective_packages: &[String]) -> YaoshiResult<()> {
    let root_tar = root.join("root.tar");
    let packages_index = root.join("packages.index");
    for required in [&root_tar, &packages_index, &root.join("boot")] {
        if !required.exists() {
            return Err(YaoshiError::build(format!(
                "Debian package-root missing {}",
                required.display()
            )));
        }
    }
    if !package_manifest_is_valid(&packages_index, effective_packages)? {
        return Err(YaoshiError::build("Debian package-root package index is invalid"));
    }
    let boot = yaoshi_debian::DebianBootArtifacts::discover(&root.join("boot"))?;
    boot.validate()
}

fn resolve_customized_root(
    context: &BuildContext,
    config: &EffectiveProductConfig,
    debian_package_root: &Path,
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let input = json!({
        "phase": "resolve-customized-root",
        "grammar": CUSTOMIZED_ROOT_GRAMMAR,
        "debian_package_root_digest": tree_sha256_hex(debian_package_root)?,
        "build_system_script_sha256": sha256_bytes(&config.build_system_script_bytes),
        "build_system_assets": build_system_assets_manifest_value(&context.repo_root)?,
        "generated_build_system_runner_source_digest": sha256_bytes(
            render_build_template(
                templates::render_debian_build_system_runner_sh(),
                "debian/build-system-runner.sh.askama",
            )?.as_bytes()
        ),
        "build_time_identity_cleanup_rule_version": BUILD_TIME_IDENTITY_CLEANUP_RULE_VERSION,
        "root_tar_normalization_rule_version": "1",
        "source_date_epoch": SOURCE_DATE_EPOCH,
    });
    cache_tree_phase(
        context,
        "resolve-customized-root",
        input,
        &paths.customized_root,
        |out| {
            resolve_customized_root_miss(context, config, debian_package_root, out)
        },
    )
}

fn resolve_customized_root_miss(
    context: &BuildContext,
    config: &EffectiveProductConfig,
    debian_package_root: &Path,
    out: &Path,
) -> YaoshiResult<()> {
    for cmd in ["tar", "chroot", "unshare"] {
        check_executable_command(cmd)?;
    }
    let paths = Paths::new(context);
    let build_system_runner = write_build_system_runner(&paths.root_helper)?;
    let build_system_script = write_build_system_script(&paths.root_helper, config)?;
    let tree = paths.root_helper.join("customized-root.tree");
    reset_dir(&tree)?;
    extract_tar_to_tree(&debian_package_root.join("root.tar"), &tree)?;
    let custom = tree.join("YAOSHI-CUSTOM");
    fs::create_dir_all(custom.join("assets"))
        .map_err(|e| YaoshiError::build(format!("create build-system staging: {e}")))?;
    fs::copy(build_system_runner, custom.join("RUN-BUILD-SYSTEM.sh"))
        .map_err(|e| YaoshiError::build(format!("stage build-system runner: {e}")))?;
    fs::copy(build_system_script, custom.join("BUILD-SYSTEM.sh"))
        .map_err(|e| YaoshiError::build(format!("stage build-system script: {e}")))?;
    let assets = prepare_build_system_assets(context, &paths.root_helper)?;
    copy_dir_recursive(&assets, &custom.join("assets"))?;
    let build_system_log = paths.work.join("debian/build-system-script.log");
    let status = Command::new("unshare")
        .arg("--map-root-user")
        .arg("--user")
        .arg("--mount")
        .arg("chroot")
        .arg(&tree)
        .arg("/YAOSHI-CUSTOM/RUN-BUILD-SYSTEM.sh")
        .stdout(File::create(&build_system_log).map_err(|e| {
            YaoshiError::build(format!("create build-system script log: {e}"))
        })?)
        .stderr(std::process::Stdio::from(File::options().append(true).open(&build_system_log).map_err(|e| {
            YaoshiError::build(format!("open build-system script log: {e}"))
        })?))
        .status()
        .map_err(|e| YaoshiError::build(format!("invoke build-system chroot: {e}")))?;
    if !status.success() {
        let context = last_log_lines(&build_system_log, 120).unwrap_or_default();
        return Err(YaoshiError::build(format!(
            "build-system script failed; log tail:\n{context}"
        )));
    }
    fs::remove_dir_all(&custom)
        .map_err(|e| YaoshiError::build(format!("remove YAOSHI-CUSTOM: {e}")))?;
    cleanup_build_identity(&tree)?;
    let export_helper = write_root_export_helper(&paths.root_helper)?;
    run_root_export_helper(&export_helper, &tree, &paths.root_helper.join("customized-export"))?;
    reset_dir(out)?;
    write_ustar_from_tree(&tree, &out.join("root.tar"))?;
    fs::copy(
        debian_package_root.join("packages.index"),
        out.join("packages.index"),
    )
    .map_err(|e| YaoshiError::build(format!("copy customized packages.index: {e}")))?;
    copy_dir_recursive(&debian_package_root.join("boot"), &out.join("boot"))?;
    enforce_customized_root_predicates(debian_package_root, out)?;
    Ok(())
}

fn resolve_root_bridge_overlay(context: &BuildContext) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    cache_tree_phase(
        context,
        "resolve-root-bridge-overlay",
        json!({
            "phase": "resolve-root-bridge-overlay",
            "grammar": ROOT_BRIDGE_OVERLAY_GRAMMAR,
            "askama_root_overlay_template_source_digest": template_group_source_digest(templates::TemplateGroup::RootOverlay),
            "askama_root_overlay_context_grammar": templates::ROOT_OVERLAY_CONTEXT_GRAMMAR,
            "generated_fixed_root_bridge_overlay_source_digest": sha256_bytes(root_overlay_source_bytes()?.as_bytes()),
        }),
        &paths.root_bridge_overlay,
        render_installed_overlay_tree,
    )
}

fn resolve_installed_root_source(
    context: &BuildContext,
    customized_root: &Path,
    root_bridge_overlay: &Path,
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    cache_file_phase(
        context,
        "resolve-installed-root-source",
        json!({
            "phase": "resolve-installed-root-source",
            "grammar": INSTALLED_ROOT_SOURCE_GRAMMAR,
            "customized_root_digest": tree_sha256_hex(customized_root)?,
            "root_bridge_overlay_digest": tree_sha256_hex(root_bridge_overlay)?,
            "merge_rule": "root-bridge-overwrites-customized-root.v1",
            "root_tar_normalization_rule_version": "1",
        }),
        &paths.installed_root_source_file,
        |out| resolve_installed_root_source_miss(context, customized_root, root_bridge_overlay, out),
    )
}

fn resolve_installed_root_source_miss(
    context: &BuildContext,
    customized_root: &Path,
    root_bridge_overlay: &Path,
    out: &Path,
) -> YaoshiResult<()> {
    for cmd in ["tar", "unshare"] {
        check_executable_command(cmd)?;
    }
    let paths = Paths::new(context);
    let tree = paths.root_helper.join("installed-root-source.tree");
    reset_dir(&tree)?;
    extract_tar_to_tree(&customized_root.join("root.tar"), &tree)?;
    merge_tree_over(&tree, root_bridge_overlay)?;
    if !tree.join("var/lib/dpkg/status").is_file() {
        return Err(YaoshiError::build(
            "installed-root source missing /var/lib/dpkg/status",
        ));
    }
    if tree.join("root/.ssh/authorized_keys").exists() || tree.join("boot/YAOSHI").exists() {
        return Err(YaoshiError::build(
            "installed-root source contains runtime-only SSH keys or /boot/YAOSHI",
        ));
    }
    write_ustar_from_tree(&tree, out)
}

fn resolve_installed_root_ext4(
    context: &BuildContext,
    config: &EffectiveProductConfig,
    installed_root_source: &Path,
    customized_root: &Path,
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let mke2fs_conf = write_mke2fs_conf(&paths.root_helper)?;
    cache_tree_phase(
        context,
        "resolve-installed-root-ext4",
        json!({
            "phase": "resolve-installed-root-ext4",
            "grammar": INSTALLED_ROOT_EXT4_GRAMMAR,
            "installed_root_source_sha256": sha256_file_hex(installed_root_source)?,
            "mke2fs_conf_digest": sha256_file_hex(&mke2fs_conf)?,
            "installed_root_size_rule_version": "1",
            "installed_root_ext4_uuid": INSTALLED_ROOT_EXT4_UUID,
            "installed_root_ext4_label": INSTALLED_ROOT_LABEL,
            "installed_root_ext4_block_size": EXT4_BLOCK_SIZE_BYTES,
            "installed_root_ext4_inode_size": EXT4_INODE_SIZE_BYTES,
        }),
        &paths.installed_root_ext4,
        |out| {
            resolve_installed_root_ext4_miss(
                context,
                config,
                installed_root_source,
                customized_root,
                &mke2fs_conf,
                out,
            )
        },
    )
}

fn resolve_installed_root_ext4_miss(
    _context: &BuildContext,
    config: &EffectiveProductConfig,
    installed_root_source: &Path,
    customized_root: &Path,
    mke2fs_conf: &Path,
    out: &Path,
) -> YaoshiResult<()> {
    for cmd in ["mke2fs", "e2fsck", "e2label", "debugfs"] {
        check_executable_command(cmd)?;
    }
    mke2fs_tar_input_preflight(mke2fs_conf)?;
    reset_dir(out)?;
    let root_tar_regular_file_bytes = tar_regular_file_bytes(installed_root_source)?;
    let root_safety_bytes =
        (root_tar_regular_file_bytes.div_ceil(4)).max(INSTALLED_ROOT_SAFETY_MINIMUM_BYTES);
    let installed_root_size_bytes =
        round_up(root_tar_regular_file_bytes + root_safety_bytes, 67_108_864);
    let installed_root_ext4 = out.join("installed-root.ext4");
    let blocks = installed_root_size_bytes / EXT4_BLOCK_SIZE_BYTES;
    let status = Command::new("mke2fs")
        .env("MKE2FS_CONFIG", mke2fs_conf)
        .arg("-q")
        .arg("-F")
        .arg("-t")
        .arg("ext4")
        .arg("-b")
        .arg(EXT4_BLOCK_SIZE_BYTES.to_string())
        .arg("-I")
        .arg(EXT4_INODE_SIZE_BYTES.to_string())
        .arg("-i")
        .arg(EXT4_INODE_RATIO_BYTES.to_string())
        .arg("-L")
        .arg(INSTALLED_ROOT_LABEL)
        .arg("-U")
        .arg(INSTALLED_ROOT_EXT4_UUID)
        .arg("-E")
        .arg("lazy_itable_init=0,lazy_journal_init=0,nodiscard,root_owner=0:0,root_perms=0755")
        .arg("-d")
        .arg(installed_root_source)
        .arg(&installed_root_ext4)
        .arg(blocks.to_string())
        .status()
        .map_err(|e| YaoshiError::build(format!("invoke mke2fs: {e}")))?;
    if !status.success() {
        return Err(YaoshiError::build(
            "mke2fs installed-root.ext4 creation failed",
        ));
    }
    run_e2fsck(&installed_root_ext4)?;
    validate_ext4_label(&installed_root_ext4)?;
    validate_ext4_overlay_paths(&installed_root_ext4)?;
    fs::copy(customized_root.join("packages.index"), out.join("packages.index"))
        .map_err(|e| YaoshiError::build(format!("copy installed-root packages.index: {e}")))?;
    if !package_manifest_is_valid(&out.join("packages.index"), &effective_package_include_list(config))? {
        return Err(YaoshiError::build("installed-root ext4 package index is invalid"));
    }
    Ok(())
}

fn root_bundle_from_outputs(
    debian_package_root: &Path,
    installed_root_ext4_tree: &Path,
    config: &EffectiveProductConfig,
) -> YaoshiResult<RootBundle> {
    validate_debian_package_root_tree(debian_package_root, &effective_package_include_list(config))?;
    let installed_root_ext4 = installed_root_ext4_tree.join("installed-root.ext4");
    let packages_index = installed_root_ext4_tree.join("packages.index");
    for required in [&installed_root_ext4, &packages_index] {
        if !required.exists() {
            return Err(YaoshiError::build(format!(
                "installed-root ext4 output missing {}",
                required.display()
            )));
        }
    }
    if installed_root_ext4_tree.join("boot").exists() {
        return Err(YaoshiError::build(
            "installed-root ext4 output must not contain boot artifacts",
        ));
    }
    let boot = yaoshi_debian::DebianBootArtifacts::discover(&debian_package_root.join("boot"))?;
    let root_bytes = fs::metadata(&installed_root_ext4)
        .map_err(|e| YaoshiError::build(format!("stat installed-root.ext4: {e}")))?
        .len();
    let installed_root_ext4_tree_digest = digest_from_cache_object_path(installed_root_ext4_tree)?;
    Ok(RootBundle {
        installed_root_ext4,
        installed_root_ext4_tree_digest,
        boot,
        boot_export_digest: tree_sha256_hex(&debian_package_root.join("boot"))?,
        root_bytes,
    })
}

fn compute_installer_module_closure(
    context: &BuildContext,
    boot: &yaoshi_debian::DebianBootArtifacts,
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let input = json!({
        "phase": "compute-installer-module-closure",
        "grammar": "yaoshi.installer-module-closure.v1",
        "kernel_release": boot.kernel_release,
        "modules_dep_sha256": sha256_file_hex(&boot.modules.join("modules.dep"))?,
        "modules_builtin_sha256": sha256_file_hex(&boot.modules.join("modules.builtin"))?,
        "seeds": INSTALLER_MODULE_SEEDS
            .iter()
            .map(|seed| json!({
                "name": seed.name,
                "required": seed.required,
            }))
            .collect::<Vec<_>>(),
    });
    cache_tree_phase(
        context,
        "compute-installer-module-closure",
        input,
        &paths.installer_modules,
        |out| {
            check_executable_command("xz")?;
            compute_installer_module_closure_miss(out, boot)
        },
    )
}

fn compute_installer_module_closure_miss(
    out: &Path,
    boot: &yaoshi_debian::DebianBootArtifacts,
) -> YaoshiResult<()> {
    if out.exists() {
        fs::remove_dir_all(out)
            .map_err(|e| YaoshiError::build(format!("reset installer modules: {e}")))?;
    }
    fs::create_dir_all(out)
        .map_err(|e| YaoshiError::build(format!("create installer modules: {e}")))?;
    boot.validate()?;
    let dep_text = fs::read_to_string(boot.modules.join("modules.dep"))
        .map_err(|e| YaoshiError::build(format!("read modules.dep: {e}")))?;
    let builtin_text = fs::read_to_string(boot.modules.join("modules.builtin"))
        .map_err(|e| YaoshiError::build(format!("read modules.builtin: {e}")))?;
    let mut by_path = BTreeMap::<String, Vec<String>>::new();
    let mut by_name = BTreeMap::<String, String>::new();
    for line in dep_text.lines() {
        let Some((module, deps)) = line.split_once(':') else {
            continue;
        };
        let module = module.trim().to_string();
        if module.is_empty() {
            continue;
        }
        let name = normalized_module_name(&module);
        if by_name.insert(name.clone(), module.clone()).is_some() {
            return Err(YaoshiError::build(format!(
                "module seed name resolves ambiguously: {name}"
            )));
        }
        by_path.insert(
            module,
            deps.split_whitespace().map(str::to_string).collect(),
        );
    }
    let builtin = builtin_text
        .lines()
        .map(normalized_module_name)
        .collect::<BTreeSet<_>>();
    let mut load_order = Vec::<String>::new();
    let mut seen = BTreeSet::<String>::new();
    for seed in INSTALLER_MODULE_SEEDS {
        let normalized_seed = seed.name.replace('-', "_");
        if let Some(module) = by_name.get(&normalized_seed) {
            include_module(module, &by_path, &mut seen, &mut load_order)?;
        } else if builtin.contains(&normalized_seed) || !seed.required {
            continue;
        } else {
            return Err(YaoshiError::build(format!(
                "installer module seed not found: {}",
                seed.name
            )));
        }
    }
    let mut emitted_order = Vec::<String>::new();
    for module in &load_order {
        reject_forbidden_installer_module(module)?;
        let src = boot.modules.join(module);
        let emitted_module = module_output_path(module);
        let dst = out
            .join("lib/modules")
            .join(&boot.kernel_release)
            .join(&emitted_module);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| YaoshiError::build(format!("create module dir: {e}")))?;
        }
        if module.ends_with(".ko.xz") {
            let output = Command::new("xz")
                .arg("-dc")
                .arg(&src)
                .output()
                .map_err(|e| YaoshiError::build(format!("decompress module {module}: {e}")))?;
            if !output.status.success() || !output.stderr.is_empty() {
                return Err(YaoshiError::build(format!(
                    "decompress module {module} failed"
                )));
            }
            fs::write(&dst, output.stdout)
                .map_err(|e| YaoshiError::build(format!("write module {emitted_module}: {e}")))?;
        } else {
            fs::copy(&src, &dst)
                .map_err(|e| YaoshiError::build(format!("copy module {module}: {e}")))?;
        }
        emitted_order.push(emitted_module);
    }
    if emitted_order.is_empty() {
        return Err(YaoshiError::build(
            "installer module closure did not emit loadable modules".to_string(),
        ));
    }
    let modules_list = emitted_order
        .iter()
        .map(|module| format!("lib/modules/{}/{module}", boot.kernel_release))
        .collect::<Vec<_>>()
        .join("\n");
    let modules_list = format!("{modules_list}\n");
    fs::write(out.join("YAOSHI-MODULES"), modules_list)
        .map_err(|e| YaoshiError::build(format!("write YAOSHI-MODULES: {e}")))
}

fn render_installer_base_initramfs_tree(
    context: &BuildContext,
    module_closure: &Path,
) -> YaoshiResult<Vec<yaoshi_initramfs::Entry>> {
    let paths = Paths::new(context);
    yaoshi_initramfs::render_installer_base_tree(&paths.initramfs_base_tree, module_closure)
}

fn render_installer_app_initramfs_tree(
    context: &BuildContext,
    installer_binary: &Path,
) -> YaoshiResult<Vec<yaoshi_initramfs::Entry>> {
    let paths = Paths::new(context);
    yaoshi_initramfs::render_installer_app_tree(&paths.initramfs_app_tree, installer_binary)
}

fn pack_installer_base_initramfs_newc(
    context: &BuildContext,
    entries: &[yaoshi_initramfs::Entry],
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let output = paths.initramfs.join(INSTALLER_BASE_INITRAMFS_NEWC);
    let bytes = yaoshi_initramfs::newc_archive_bytes(entries)?;
    fs::write(&output, bytes)
        .map_err(|e| YaoshiError::build(format!("write installer base initramfs newc: {e}")))?;
    Ok(output)
}

fn pack_installer_app_initramfs_newc(
    context: &BuildContext,
    entries: &[yaoshi_initramfs::Entry],
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let output = paths.initramfs.join(INSTALLER_APP_INITRAMFS_NEWC);
    let bytes = yaoshi_initramfs::newc_archive_bytes(entries)?;
    fs::write(&output, bytes)
        .map_err(|e| YaoshiError::build(format!("write installer app initramfs newc: {e}")))?;
    Ok(output)
}

fn compress_installer_base_initramfs_zstd(
    context: &BuildContext,
    newc: &Path,
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let output = paths.initramfs.join(INSTALLER_BASE_INITRAMFS_ZSTD);
    cache_file_phase(
        context,
        "pack-installer-base-initramfs-zstd",
        json!({
            "phase": "pack-installer-base-initramfs-zstd",
            "grammar": "yaoshi.installer-base-initramfs-zstd.v1",
            "newc_sha256": sha256_file_hex(newc)?,
            "level": ZSTD_COMPRESSION_LEVEL,
            "window_log": ZSTD_WINDOW_LOG,
            "content_checksum": false,
            "long_distance_matching": false,
            "dictionary": "none",
        }),
        &output,
        |out| yaoshi_initramfs::compress_zstd_file(newc, out),
    )
}

fn compress_installer_app_initramfs_zstd(
    context: &BuildContext,
    newc: &Path,
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let output = paths.initramfs.join(INSTALLER_APP_INITRAMFS_ZSTD);
    cache_file_phase(
        context,
        "pack-installer-app-initramfs-zstd",
        json!({
            "phase": "pack-installer-app-initramfs-zstd",
            "grammar": "yaoshi.installer-app-initramfs-zstd.v1",
            "newc_sha256": sha256_file_hex(newc)?,
            "level": ZSTD_COMPRESSION_LEVEL,
            "window_log": ZSTD_WINDOW_LOG,
            "content_checksum": false,
            "long_distance_matching": false,
            "dictionary": "none",
        }),
        &output,
        |out| yaoshi_initramfs::compress_zstd_file(newc, out),
    )
}
