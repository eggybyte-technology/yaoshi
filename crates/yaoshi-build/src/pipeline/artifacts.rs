fn stage_systemd_boot(tree: &Path, boot: &yaoshi_debian::DebianBootArtifacts) -> YaoshiResult<()> {
    if tree.exists() {
        fs::remove_dir_all(tree).map_err(|e| YaoshiError::image(format!("reset ESP tree: {e}")))?;
    }
    fs::create_dir_all(tree.join("EFI/BOOT"))
        .map_err(|e| YaoshiError::image(format!("create ESP tree: {e}")))?;
    fs::copy(&boot.systemd_boot_efi, tree.join(EFI_BOOT_PATH))
        .map_err(|e| YaoshiError::image(format!("stage systemd-boot fallback: {e}")))?;
    Ok(())
}

fn stage_installed_esp_tree(
    context: &BuildContext,
    config: &EffectiveProductConfig,
    boot: &yaoshi_debian::DebianBootArtifacts,
    dashboard_binary: &Path,
) -> YaoshiResult<()> {
    let paths = Paths::new(context);
    stage_systemd_boot(&paths.installed_esp_tree, boot)?;
    write_bytes(
        &paths.installed_esp_tree.join("loader/loader.conf"),
        render_build_template(
            templates::render_esp_loader_conf(),
            "esp/loader/loader.conf.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &paths.installed_esp_tree.join("loader/entries/yaoshi.conf"),
        render_build_template(
            templates::render_esp_yaoshi_conf(),
            "esp/loader/yaoshi.conf.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    fs::create_dir_all(paths.installed_esp_tree.join("YAOSHI/BOOT"))
        .map_err(|e| YaoshiError::image(format!("create installed boot dir: {e}")))?;
    fs::copy(
        &boot.vmlinuz,
        paths.installed_esp_tree.join(INSTALLED_KERNEL_PATH),
    )
    .map_err(|e| YaoshiError::image(format!("stage Debian kernel: {e}")))?;
    fs::copy(
        &boot.initrd_img,
        paths.installed_esp_tree.join(INSTALLED_INITRD_PATH),
    )
    .map_err(|e| YaoshiError::image(format!("stage Debian initramfs: {e}")))?;
    write_bytes(
        &paths.installed_esp_tree.join(INSTALLED_KERNEL_RELEASE_PATH),
        render_build_template(
            templates::render_esp_kernel_release(&boot.kernel_release),
            "esp/boot/KERNEL-RELEASE.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &paths.installed_esp_tree.join("YAOSHI/CONFIG/HOSTNAME"),
        render_build_template(
            templates::render_esp_hostname(&config.hostname),
            "esp/config/HOSTNAME.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &paths.installed_esp_tree.join("YAOSHI/CONFIG/AUTHKEYS"),
        render_build_template(
            templates::render_esp_authorized_keys(&config.ssh_authorized_keys),
            "esp/config/AUTHKEYS.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &paths.installed_esp_tree.join("YAOSHI/RUNTIME/PREPARE"),
        render_build_template(
            templates::render_esp_prepare(),
            "esp/runtime/PREPARE.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &paths.installed_esp_tree.join("YAOSHI/RUNTIME/FIRST-BOOT"),
        &config.first_boot_script_bytes,
        0o644,
    )?;
    fs::create_dir_all(paths.installed_esp_tree.join("YAOSHI/DASHBOARD"))
        .map_err(|e| YaoshiError::image(format!("create dashboard dir: {e}")))?;
    fs::copy(
        dashboard_binary,
        paths
            .installed_esp_tree
            .join("YAOSHI/DASHBOARD/YAOSHI-DASHBOARD"),
    )
    .map_err(|e| YaoshiError::image(format!("stage dashboard binary: {e}")))?;
    Ok(())
}

fn pack_installed_esp_fat32(
    context: &BuildContext,
    config: &EffectiveProductConfig,
    root_bundle: &RootBundle,
    dashboard_binary: &Path,
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let output = paths.image.join("installed-esp.fat32");
    let files = INSTALLED_ESP_REQUIRED_FILE_SET
        .iter()
        .map(|path| tree_file(&paths.installed_esp_tree, path))
        .collect::<YaoshiResult<Vec<_>>>()?;
    cache_file_phase(
        context,
        "pack-installed-esp-fat32",
        json!({
            "phase": "pack-installed-esp-fat32",
            "grammar": FAT32_GRAMMAR,
            "installed_esp_size_rule_version": "1",
            "installed_esp_size_bytes": INSTALLED_ESP_SIZE_BYTES,
            "required_files": INSTALLED_ESP_REQUIRED_FILE_SET,
            "askama_installed_esp_template_source_digest": template_group_source_digest(templates::TemplateGroup::InstalledEsp),
            "askama_installed_esp_context_grammar": templates::INSTALLED_ESP_CONTEXT_GRAMMAR,
            "debian_package_root_boot_export_digest": &root_bundle.boot_export_digest,
            "yaoshi_dashboard_output_digest": sha256_file_hex(dashboard_binary)?,
            "esp_runtime_prepare_source_digest": sha256_bytes(
                render_build_template(templates::render_esp_prepare(), "esp/runtime/PREPARE.askama")?.as_bytes()
            ),
            "first_boot_script_sha256": sha256_bytes(&fs::read(paths.installed_esp_tree.join("YAOSHI/RUNTIME/FIRST-BOOT"))
                .map_err(|e| YaoshiError::image(format!("read staged first-boot script: {e}")))?),
            "first_boot_runtime_grammar_version": FIRST_BOOT_RUNTIME_GRAMMAR_VERSION,
            "effective_hostname": config.hostname,
            "effective_authorized_keys_bytes_sha256": sha256_bytes(
                render_build_template(
                    templates::render_esp_authorized_keys(&config.ssh_authorized_keys),
                    "esp/config/AUTHKEYS.askama",
                )?.as_bytes()
            ),
            "tree_sha256": tree_sha256_hex(&paths.installed_esp_tree)?,
        }),
        &output,
        |out| yaoshi_image::create_fat32_image(out, INSTALLED_ESP_LABEL, &files),
    )
}

fn pack_installed_system_payload_from_target_graph(
    context: &BuildContext,
    installed_esp: &Path,
    root_bundle: &RootBundle,
) -> YaoshiResult<PayloadArtifact> {
    let paths = Paths::new(context);
    let output = paths.image.join("installed-system.ypayload");
    let layout = InstalledGptLayout::fixed(root_bundle.root_bytes);
    let esp = layout.esp_partition();
    let root = layout.root_partition();
    let meta = yaoshi_payload::PayloadBuildMeta {
        esp_start: esp.start_byte,
        esp_size: esp.byte_size,
        root_start: root.start_byte,
        root_size: root.byte_size,
    };
    let installed_esp_sha256 = sha256_file_hex(installed_esp)?;
    let installed_root_identity = root_bundle.installed_root_ext4_tree_digest.clone();
    let gpt_source_sha256 = sha256_bytes(
        canonical_json_bytes(&json!({
            "grammar": "yaoshi.gpt.v1",
            "layout": "installed-target",
            "logical_sector_size": LOGICAL_SECTOR_SIZE,
            "installed_disk_guid": INSTALLED_DISK_GUID,
            "installed_esp_partition_guid": INSTALLED_ESP_PARTITION_GUID,
            "installed_root_partition_guid": INSTALLED_ROOT_PARTITION_GUID,
            "installed_esp_size_bytes": esp.byte_size,
            "installed_root_size_bytes": root.byte_size,
        }))?
        .as_slice(),
    );
    let object = cache_file_phase(
        context,
        "pack-installed-system-payload-from-target-graph",
        json!({
            "phase": "pack-installed-system-payload-from-target-graph",
            "grammar": PAYLOAD_GRAMMAR,
            "virtual_installed_target_graph_grammar": VIRTUAL_TARGET_GRAPH_GRAMMAR,
            "gpt_writer_grammar": "yaoshi.gpt.v1",
            "logical_sector_size": LOGICAL_SECTOR_SIZE,
            "required_block_size": REQUIRED_BLOCK_SIZE,
            "leading_gap_bytes": LEADING_GAP_BYTES,
            "trailing_gap_bytes": TRAILING_GAP_BYTES,
            "installed_esp_size_bytes": esp.byte_size,
            "installed_root_size_bytes": root.byte_size,
            "installed_esp_fat32": &installed_esp_sha256,
            "installed_root_ext4_tree": &installed_root_identity,
            "installed_disk_guid": INSTALLED_DISK_GUID,
            "installed_esp_partition_guid": INSTALLED_ESP_PARTITION_GUID,
            "installed_root_partition_guid": INSTALLED_ROOT_PARTITION_GUID,
            "zstd_level": PAYLOAD_ZSTD_COMPRESSION_LEVEL,
            "zstd_window_log": ZSTD_WINDOW_LOG,
            "payload_extent_max_uncompressed_bytes": PAYLOAD_EXTENT_MAX_UNCOMPRESSED_BYTES,
            "payload_container_alignment_bytes": PAYLOAD_CONTAINER_ALIGNMENT_BYTES,
        }),
        &output,
        |out| {
            let graph = yaoshi_image::VirtualInstalledTargetGraph::new(
                layout.clone(),
                installed_esp,
                &root_bundle.installed_root_ext4,
            )?;
            let encoding_cache = yaoshi_payload::PayloadEncodingCache {
                root: paths.cache.join("extent/payload-encoding"),
                installed_esp_fat32_sha256: installed_esp_sha256.clone(),
                installed_root_ext4_identity: installed_root_identity.clone(),
                gpt_source_sha256: gpt_source_sha256.clone(),
            };
            let info = yaoshi_payload::build_payload_from_virtual_target_graph_with_cache(
                &graph,
                out,
                &meta,
                Some(&encoding_cache),
            )?;
            let semantic_hex = hex_digest(&info.semantic_required_sha256);
            let graph_path = paths.image.join("installed-target.graph.json");
            yaoshi_image::write_installed_target_layout_graph(
                &graph_path,
                graph.layout(),
                &semantic_hex,
            )?;
            yaoshi_image::validate_installed_target_layout_graph(&graph_path, graph.layout())?;
            Ok(())
        },
    )?;
    let info = yaoshi_payload::validate_payload_metadata(&object)?;
    let digest = digest_from_cache_object_path(&object)?;
    Ok(PayloadArtifact {
        path: object,
        digest,
        info,
    })
}

fn stage_installer_boot_tree(
    context: &BuildContext,
    boot: &yaoshi_debian::DebianBootArtifacts,
    base_initramfs_zstd: &Path,
    app_initramfs_zstd: &Path,
) -> YaoshiResult<()> {
    let paths = Paths::new(context);
    stage_systemd_boot(&paths.installer_boot_tree, boot)?;
    write_bytes(
        &paths.installer_boot_tree.join("loader/loader.conf"),
        render_build_template(
            templates::render_installer_boot_loader_conf(),
            "installer-boot/loader/loader.conf.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    write_bytes(
        &paths
            .installer_boot_tree
            .join("loader/entries/yaoshi-installer.conf"),
        render_build_template(
            templates::render_installer_boot_yaoshi_installer_conf(),
            "installer-boot/loader/yaoshi-installer.conf.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    fs::create_dir_all(paths.installer_boot_tree.join("YAOSHI/BOOT"))
        .map_err(|e| YaoshiError::image(format!("create installer boot dir: {e}")))?;
    fs::copy(
        &boot.vmlinuz,
        paths.installer_boot_tree.join(INSTALLER_KERNEL_PATH),
    )
    .map_err(|e| YaoshiError::image(format!("stage installer kernel: {e}")))?;
    write_bytes(
        &paths
            .installer_boot_tree
            .join(INSTALLER_KERNEL_RELEASE_PATH),
        render_build_template(
            templates::render_installer_boot_kernel_release(&boot.kernel_release),
            "installer-boot/boot/KERNEL-RELEASE.askama",
        )?
        .as_bytes(),
        0o644,
    )?;
    fs::copy(
        base_initramfs_zstd,
        paths
            .installer_boot_tree
            .join(INSTALLER_BASE_INITRAMFS_FAT_PATH),
    )
    .map_err(|e| YaoshiError::image(format!("stage installer base initramfs: {e}")))?;
    fs::copy(
        app_initramfs_zstd,
        paths
            .installer_boot_tree
            .join(INSTALLER_APP_INITRAMFS_FAT_PATH),
    )
    .map_err(|e| YaoshiError::image(format!("stage installer app initramfs: {e}")))?;
    Ok(())
}

fn pack_installer_boot_fat32(
    context: &BuildContext,
    root_bundle: &RootBundle,
    base_initramfs_zstd: &Path,
    app_initramfs_zstd: &Path,
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let output = paths.image.join("installer-boot.fat32");
    let files = [
        tree_file(&paths.installer_boot_tree, EFI_BOOT_PATH)?,
        tree_file(&paths.installer_boot_tree, "loader/loader.conf")?,
        tree_file(
            &paths.installer_boot_tree,
            "loader/entries/yaoshi-installer.conf",
        )?,
        tree_file(&paths.installer_boot_tree, INSTALLER_KERNEL_PATH)?,
        tree_file(&paths.installer_boot_tree, INSTALLER_KERNEL_RELEASE_PATH)?,
        tree_file(
            &paths.installer_boot_tree,
            INSTALLER_BASE_INITRAMFS_FAT_PATH,
        )?,
        tree_file(&paths.installer_boot_tree, INSTALLER_APP_INITRAMFS_FAT_PATH)?,
    ];
    cache_file_phase(
        context,
        "pack-installer-boot-fat32",
        json!({
            "phase": "pack-installer-boot-fat32",
            "grammar": FAT32_GRAMMAR,
            "installer_boot_size_rule_version": "1",
            "installer_boot_size_bytes": INSTALLER_BOOT_SIZE_BYTES,
            "askama_installer_boot_template_source_digest": template_group_source_digest(templates::TemplateGroup::InstallerBoot),
            "askama_installer_boot_context_grammar": templates::INSTALLER_BOOT_CONTEXT_GRAMMAR,
            "debian_package_root_boot_export_digest": &root_bundle.boot_export_digest,
            "installer_base_initramfs_zstd_output_digest": sha256_file_hex(base_initramfs_zstd)?,
            "installer_app_initramfs_zstd_output_digest": sha256_file_hex(app_initramfs_zstd)?,
            "tree_sha256": tree_sha256_hex(&paths.installer_boot_tree)?,
        }),
        &output,
        |out| yaoshi_image::create_fat32_image(out, INSTALLER_BOOT_LABEL, &files),
    )
}

fn pack_final_installer_mbr_composite(
    context: &BuildContext,
    installer_boot: &Path,
    payload: &PayloadArtifact,
) -> YaoshiResult<PathBuf> {
    let layout = InstallerMbrLayout {
        installed_system_bytes: payload.info.total_payload_bytes,
    };
    cache_composite_phase(
        context,
        "pack-final-installer-mbr-composite",
        json!({
            "phase": "pack-final-installer-mbr-composite",
            "grammar": FINAL_INSTALLER_GRAMMAR,
            "composite_file_grammar": COMPOSITE_FILE_GRAMMAR,
            "leading_gap_bytes": LEADING_GAP_BYTES,
            "installer_boot_size_bytes": INSTALLER_BOOT_SIZE_BYTES,
            "trailing_gap_bytes": TRAILING_GAP_BYTES,
            "installer_boot_fat32": sha256_file_hex(installer_boot)?,
            "installed_system_payload": &payload.digest,
            "payload_container_bytes": payload.info.total_payload_bytes,
        }),
        || final_installer_composite_manifest(installer_boot, &payload.path, &payload.digest, &layout),
    )
}

fn publish(
    context: &BuildContext,
    config: &EffectiveProductConfig,
    final_composite: &Path,
) -> YaoshiResult<PathBuf> {
    let paths = Paths::new(context);
    let candidate = paths.image.join("yaoshi.img.candidate");
    materialize_composite_file(final_composite, &context.cache_root, &candidate)?;
    lightweight_final_media_check(&candidate)?;
    let output = context.repo_root.join(OUTPUT_IMAGE);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::publish(format!("create output parent: {e}")))?;
    }
    reject_nonregular_output_path(context)?;
    File::open(&candidate)
        .and_then(|file| file.sync_all())
        .map_err(|e| YaoshiError::publish(format!("fsync installer image candidate: {e}")))?;
    fs::rename(&candidate, &output)
        .map_err(|e| YaoshiError::publish(format!("install published image: {e}")))?;
    if let Some(parent) = output.parent() {
        fsync_publish_dir(parent)?;
    }
    let fingerprints = current_image_fingerprints(&context.repo_root, config)?;
    write_current_image_stamp(context, &fingerprints)?;
    Ok(output)
}

fn materialize_composite_file(
    composite: &Path,
    cache_root: &Path,
    output: &Path,
) -> YaoshiResult<()> {
    let manifest = fs::read(composite)
        .map_err(|e| YaoshiError::publish(format!("read final composite object: {e}")))?;
    validate_composite_manifest_bytes(&manifest, Some(cache_root))?;
    let value: Value = serde_json::from_slice(&manifest)
        .map_err(|e| YaoshiError::publish(format!("parse final composite object: {e}")))?;
    let logical_size = value
        .get("logical_size")
        .and_then(Value::as_u64)
        .ok_or_else(|| YaoshiError::publish("composite logical_size is invalid"))?;
    let segments = value
        .get("segments")
        .and_then(Value::as_array)
        .ok_or_else(|| YaoshiError::publish("composite segments are invalid"))?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::publish(format!("create candidate parent: {e}")))?;
    }
    let _ = fs::remove_file(output);
    let mut out = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(output)
        .map_err(|e| YaoshiError::publish(format!("create installer image candidate: {e}")))?;
    for segment in segments {
        let segment = segment
            .as_object()
            .ok_or_else(|| YaoshiError::publish("composite segment is invalid"))?;
        let kind = segment
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| YaoshiError::publish("composite segment kind is invalid"))?;
        let offset = segment
            .get("offset")
            .and_then(Value::as_u64)
            .ok_or_else(|| YaoshiError::publish("composite segment offset is invalid"))?;
        let len = segment
            .get("len")
            .and_then(Value::as_u64)
            .ok_or_else(|| YaoshiError::publish("composite segment len is invalid"))?;
        out.seek(SeekFrom::Start(offset))
            .map_err(|e| YaoshiError::publish(format!("seek candidate segment: {e}")))?;
        match kind {
            "inline-bytes" => {
                let bytes = hex_bytes(
                    segment
                        .get("bytes_hex")
                        .and_then(Value::as_str)
                        .ok_or_else(|| YaoshiError::publish("composite inline bytes invalid"))?,
                )
                .map_err(|e| YaoshiError::publish(e.to_string()))?;
                out.write_all(&bytes)
                    .map_err(|e| YaoshiError::publish(format!("write inline segment: {e}")))?;
            }
            "zero" => {
                out.seek(SeekFrom::Start(offset + len))
                    .map_err(|e| YaoshiError::publish(format!("seek zero segment: {e}")))?;
            }
            "file-ref" => {
                let digest = segment
                    .get("digest")
                    .and_then(Value::as_str)
                    .ok_or_else(|| YaoshiError::publish("composite file-ref digest invalid"))?;
                let source_offset = segment
                    .get("source_offset")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        YaoshiError::publish("composite file-ref source offset invalid")
                    })?;
                copy_file_range_fallback(
                    &cache_root.join("object/file").join(digest),
                    source_offset,
                    &mut out,
                    offset,
                    len,
                )?;
            }
            _ => return Err(YaoshiError::publish("unsupported composite segment kind")),
        }
    }
    out.set_len(logical_size)
        .map_err(|e| YaoshiError::publish(format!("truncate installer image candidate: {e}")))?;
    let logical_sha256 = value
        .get("logical_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| YaoshiError::publish("composite logical_sha256 invalid"))?;
    if !is_lower_sha256_hex(logical_sha256) {
        return Err(YaoshiError::publish("composite logical_sha256 invalid"));
    }
    Ok(())
}

fn copy_file_range_fallback(
    src: &Path,
    source_offset: u64,
    dst: &mut File,
    dst_offset: u64,
    len: u64,
) -> YaoshiResult<()> {
    let mut src = File::open(src)
        .map_err(|e| YaoshiError::publish(format!("open composite file-ref source: {e}")))?;
    src.seek(SeekFrom::Start(source_offset))
        .map_err(|e| YaoshiError::publish(format!("seek composite file-ref source: {e}")))?;
    dst.seek(SeekFrom::Start(dst_offset))
        .map_err(|e| YaoshiError::publish(format!("seek composite file-ref destination: {e}")))?;
    let copied = std::io::copy(&mut Read::by_ref(&mut src).take(len), dst)
        .map_err(|e| YaoshiError::publish(format!("copy composite file-ref segment: {e}")))?;
    if copied != len {
        return Err(YaoshiError::publish(
            "composite file-ref segment copied short",
        ));
    }
    Ok(())
}

fn qemu_preflight_current_image(image: &Path) -> YaoshiResult<yaoshi_payload::PayloadInfo> {
    let mbr = yaoshi_image::parse_mbr(image)?;
    if mbr.len() != 2 || mbr[0].mbr_type != 0xEF || mbr[1].mbr_type != 0x83 {
        return Err(YaoshiError::image("current image MBR shape mismatch"));
    }
    let payload_offset = u64::from(mbr[1].start_lba) * SECTOR_SIZE;
    let payload_len = u64::from(mbr[1].sector_count) * SECTOR_SIZE;
    yaoshi_payload::validate_payload_region_metadata(image, payload_offset, payload_len)
}

fn lightweight_final_media_check(image: &Path) -> YaoshiResult<()> {
    qemu_preflight_current_image(image).map(|_| ())
}

fn final_installer_composite_manifest(
    installer_boot: &Path,
    payload: &Path,
    payload_digest: &str,
    layout: &InstallerMbrLayout,
) -> YaoshiResult<(String, Vec<u8>)> {
    let boot = layout.boot_partition();
    let payload_part = layout.payload_partition();
    let mbr = yaoshi_image::installer_mbr_sector(layout)?;
    let boot_digest = sha256_file_hex(installer_boot)?;
    let boot_len = fs::metadata(installer_boot)
        .map_err(|e| YaoshiError::image(format!("stat installer boot FAT32: {e}")))?
        .len();
    let payload_len = fs::metadata(payload)
        .map_err(|e| YaoshiError::image(format!("stat installed payload: {e}")))?
        .len();
    if boot_len != boot.byte_size || payload_len != payload_part.byte_size {
        return Err(YaoshiError::image(
            "final installer composite source length mismatch",
        ));
    }
    if read_file_prefix(payload, 16)? != PAYLOAD_MAGIC {
        return Err(YaoshiError::image(
            "final installer composite payload source is not YAOSHI_PAYLOAD_V1",
        ));
    }

    let logical_sha256 = hash_final_installer_logical_stream(
        &mbr,
        installer_boot,
        payload,
        layout.image_bytes(),
        boot.start_byte,
        payload_part.start_byte,
    )?;
    let manifest = json!({
        "grammar": COMPOSITE_FILE_GRAMMAR,
        "kind": "composite-file",
        "logical_sha256": logical_sha256,
        "logical_size": layout.image_bytes(),
        "segments": [
            {
                "kind": "inline-bytes",
                "offset": 0,
                "len": 512,
                "bytes_hex": bytes_hex(&mbr),
                "sha256": sha256_bytes(&mbr),
            },
            {
                "kind": "zero",
                "offset": 512,
                "len": LEADING_GAP_BYTES - 512,
            },
            {
                "kind": "file-ref",
                "offset": boot.start_byte,
                "len": boot.byte_size,
                "digest": boot_digest,
                "source_offset": 0,
            },
            {
                "kind": "file-ref",
                "offset": payload_part.start_byte,
                "len": payload_part.byte_size,
                "digest": payload_digest,
                "source_offset": 0,
            },
            {
                "kind": "zero",
                "offset": payload_part.start_byte + payload_part.byte_size,
                "len": TRAILING_GAP_BYTES,
            }
        ],
    });
    let bytes = canonical_json_bytes(&manifest)?;
    validate_composite_manifest_bytes(&bytes, None)?;
    let logical = manifest["logical_sha256"]
        .as_str()
        .ok_or_else(|| YaoshiError::internal("missing logical sha256 after composite render"))?
        .to_string();
    Ok((logical, bytes))
}

fn hash_final_installer_logical_stream(
    mbr: &[u8; 512],
    installer_boot: &Path,
    payload: &Path,
    logical_size: u64,
    boot_offset: u64,
    payload_offset: u64,
) -> YaoshiResult<String> {
    let mut hasher = Sha256::new();
    hasher.update(mbr);
    hash_zero_bytes(&mut hasher, boot_offset - 512);
    hash_file_bytes(&mut hasher, installer_boot)?;
    let boot_len = fs::metadata(installer_boot)
        .map_err(|e| YaoshiError::image(format!("stat installer boot FAT32: {e}")))?
        .len();
    if payload_offset != boot_offset + boot_len {
        return Err(YaoshiError::image(
            "final installer composite segment gap mismatch",
        ));
    }
    hash_file_bytes(&mut hasher, payload)?;
    let payload_len = fs::metadata(payload)
        .map_err(|e| YaoshiError::image(format!("stat installed payload: {e}")))?
        .len();
    hash_zero_bytes(&mut hasher, logical_size - payload_offset - payload_len);
    Ok(hex_digest(hasher.finalize().as_slice()))
}

fn read_file_prefix(path: &Path, len: usize) -> YaoshiResult<Vec<u8>> {
    let mut file =
        File::open(path).map_err(|e| YaoshiError::image(format!("open file prefix: {e}")))?;
    let mut bytes = vec![0u8; len];
    file.read_exact(&mut bytes)
        .map_err(|e| YaoshiError::image(format!("read file prefix: {e}")))?;
    Ok(bytes)
}

fn hash_file_bytes(hasher: &mut Sha256, path: &Path) -> YaoshiResult<()> {
    let mut file =
        File::open(path).map_err(|e| YaoshiError::image(format!("open hash input: {e}")))?;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| YaoshiError::image(format!("read hash input: {e}")))?;
        if n == 0 {
            return Ok(());
        }
        hasher.update(&buf[..n]);
    }
}

fn hash_zero_bytes(hasher: &mut Sha256, len: u64) {
    let zeros = [0u8; 1024 * 1024];
    let mut remaining = len;
    while remaining > 0 {
        let chunk = remaining.min(zeros.len() as u64) as usize;
        hasher.update(&zeros[..chunk]);
        remaining -= chunk as u64;
    }
}

fn bytes_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hex_bytes(raw: &str) -> YaoshiResult<Vec<u8>> {
    if !raw.len().is_multiple_of(2) {
        return Err(YaoshiError::image("hex byte string has odd length"));
    }
    let mut out = Vec::with_capacity(raw.len() / 2);
    for chunk in raw.as_bytes().chunks_exact(2) {
        let high = hex_nibble(chunk[0])?;
        let low = hex_nibble(chunk[1])?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> YaoshiResult<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(YaoshiError::image("hex byte string is not lowercase hex")),
    }
}

fn reject_nonregular_output_path(context: &BuildContext) -> YaoshiResult<()> {
    let output = context.repo_root.join(OUTPUT_IMAGE);
    if output.exists() && !output.is_file() {
        return Err(YaoshiError::usage(
            "output path exists and is not a regular file",
        ));
    }
    Ok(())
}

fn write_current_image_stamp(
    context: &BuildContext,
    fingerprints: &CurrentImageFingerprints,
) -> YaoshiResult<()> {
    let stamp = context.repo_root.join(CURRENT_IMAGE_STAMP_PATH);
    if let Some(parent) = stamp.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::publish(format!("create current image stamp parent: {e}")))?;
    }
    let tmp = stamp.with_extension("current.tmp");
    fs::write(&tmp, fingerprints.stamp_text())
        .map_err(|e| YaoshiError::publish(format!("write current image stamp temp: {e}")))?;
    File::open(&tmp)
        .and_then(|file| file.sync_all())
        .map_err(|e| YaoshiError::publish(format!("fsync current image stamp temp: {e}")))?;
    fs::rename(&tmp, &stamp)
        .map_err(|e| YaoshiError::publish(format!("install current image stamp: {e}")))?;
    if let Some(parent) = stamp.parent() {
        fsync_publish_dir(parent)?;
    }
    Ok(())
}

fn stamp_matches(stamp: &Path, fingerprints: &CurrentImageFingerprints) -> YaoshiResult<bool> {
    let Ok(text) = fs::read_to_string(stamp) else {
        return Ok(false);
    };
    Ok(parse_current_image_stamp(&text).is_some_and(|parsed| &parsed == fingerprints))
}

fn parse_current_image_stamp(text: &str) -> Option<CurrentImageFingerprints> {
    let mut lines = text.lines();
    let foundation = lines
        .next()?
        .strip_prefix("foundation-fingerprint=")?
        .to_string();
    let installed_root = lines
        .next()?
        .strip_prefix("installed-root-fingerprint=")?
        .to_string();
    let installed_runtime = lines
        .next()?
        .strip_prefix("installed-runtime-fingerprint=")?
        .to_string();
    let installer_envelope = lines
        .next()?
        .strip_prefix("installer-envelope-fingerprint=")?
        .to_string();
    let published_image = lines
        .next()?
        .strip_prefix("published-image-fingerprint=")?
        .to_string();
    if lines.next().is_some() || !text.ends_with('\n') {
        return None;
    }
    if !is_lower_sha256_hex(&foundation)
        || !is_lower_sha256_hex(&installed_root)
        || !is_lower_sha256_hex(&installed_runtime)
        || !is_lower_sha256_hex(&installer_envelope)
        || !is_lower_sha256_hex(&published_image)
    {
        return None;
    }
    Some(CurrentImageFingerprints {
        foundation,
        installed_root,
        installed_runtime,
        installer_envelope,
        published_image,
    })
}
