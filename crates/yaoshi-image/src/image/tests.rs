#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tempfile_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn write_fake_ext4(path: &Path, bytes: u64) {
        let mut file = File::create(path).unwrap();
        file.set_len(bytes).unwrap();
        file.seek(SeekFrom::Start(1024 + 0x38)).unwrap();
        file.write_all(&0xef53u16.to_le_bytes()).unwrap();
        file.seek(SeekFrom::Start(1024 + 120)).unwrap();
        let mut label = [0u8; 16];
        label[..INSTALLED_ROOT_LABEL.len()].copy_from_slice(INSTALLED_ROOT_LABEL.as_bytes());
        file.write_all(&label).unwrap();
    }

    fn fake_elf64_x86_64(program_type: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 64 + 56];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&program_type.to_le_bytes());
        bytes[68..72].copy_from_slice(&5u32.to_le_bytes());
        bytes[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
        let len = bytes.len() as u64;
        bytes[88..96].copy_from_slice(&len.to_le_bytes());
        bytes[96..104].copy_from_slice(&len.to_le_bytes());
        bytes[104..112].copy_from_slice(&0x1000u64.to_le_bytes());
        bytes
    }

    #[test]
    fn embedded_runtime_elf_rejects_pt_interp() {
        assert!(validate_embedded_runtime_elf(&fake_elf64_x86_64(1)).is_ok());
        assert!(validate_embedded_runtime_elf(&fake_elf64_x86_64(3)).is_err());
    }

    fn update_gpt_crcs(path: &Path, layout: &InstalledGptLayout) {
        let mut image = fs::read(path).unwrap();
        let primary_entries_offset = (2 * SECTOR_SIZE) as usize;
        let entry_bytes = 128 * 128usize;
        let backup_header_offset = (layout.image_bytes() - SECTOR_SIZE) as usize;
        let backup_entries_offset = backup_header_offset - entry_bytes;
        let mut hasher = Hasher::new();
        hasher.update(&image[primary_entries_offset..primary_entries_offset + entry_bytes]);
        let entries_crc = hasher.finalize();
        for header_offset in [SECTOR_SIZE as usize, backup_header_offset] {
            image[header_offset + 88..header_offset + 92]
                .copy_from_slice(&entries_crc.to_le_bytes());
            image[header_offset + 16..header_offset + 20].fill(0);
            let header_size = u32::from_le_bytes(
                image[header_offset + 12..header_offset + 16]
                    .try_into()
                    .unwrap(),
            ) as usize;
            let mut hasher = Hasher::new();
            hasher.update(&image[header_offset..header_offset + header_size]);
            let header_crc = hasher.finalize();
            image[header_offset + 16..header_offset + 20]
                .copy_from_slice(&header_crc.to_le_bytes());
        }
        let primary_entries =
            image[primary_entries_offset..primary_entries_offset + entry_bytes].to_vec();
        image[backup_entries_offset..backup_entries_offset + entry_bytes]
            .copy_from_slice(&primary_entries);
        let mut hasher = Hasher::new();
        hasher.update(&image[backup_entries_offset..backup_entries_offset + entry_bytes]);
        let backup_entries_crc = hasher.finalize();
        image[backup_header_offset + 88..backup_header_offset + 92]
            .copy_from_slice(&backup_entries_crc.to_le_bytes());
        image[backup_header_offset + 16..backup_header_offset + 20].fill(0);
        let header_size = u32::from_le_bytes(
            image[backup_header_offset + 12..backup_header_offset + 16]
                .try_into()
                .unwrap(),
        ) as usize;
        let mut hasher = Hasher::new();
        hasher.update(&image[backup_header_offset..backup_header_offset + header_size]);
        let header_crc = hasher.finalize();
        image[backup_header_offset + 16..backup_header_offset + 20]
            .copy_from_slice(&header_crc.to_le_bytes());
        fs::write(path, image).unwrap();
    }

    #[test]
    fn mbr_round_trips_two_partition_layout() {
        let dir = tempfile_path("yaoshi-image-test");
        fs::create_dir_all(&dir).unwrap();
        let boot = dir.join("boot.fat32");
        let payload = dir.join("installed-system.img");
        let out = dir.join("yaoshi.img");
        File::create(&boot)
            .unwrap()
            .set_len(INSTALLER_BOOT_SIZE_BYTES)
            .unwrap();
        let mut payload_file = File::create(&payload).unwrap();
        payload_file.write_all(&vec![0x5a; 1024 * 1024]).unwrap();
        let layout = InstallerMbrLayout {
            installed_system_bytes: 1024 * 1024,
        };
        write_mbr_installer_image(&out, &boot, &payload, &layout).unwrap();
        let parts = parse_mbr(&out).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].mbr_type, 0xEF);
        assert_eq!(parts[1].mbr_type, 0x83);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn fat32_validation_rejects_extra_empty_directory() {
        let dir = tempfile_path("yaoshi-fat-extra-dir-test");
        fs::create_dir_all(&dir).unwrap();
        let image = dir.join("esp.fat32");
        let files = [TreeFile {
            path: PathBuf::from(EFI_BOOT_PATH),
            bytes: b"MZkernel".to_vec(),
        }];
        create_fat32_image(&image, INSTALLED_ESP_LABEL, &files).unwrap();
        {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&image)
                .unwrap();
            let fs = FileSystem::new(BufStream::new(file), FsOptions::new()).unwrap();
            fs.root_dir().create_dir("loader").unwrap();
        }
        assert!(validate_fat32_tree(&image, INSTALLED_ESP_LABEL, &files).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn mbr_parser_rejects_nonzero_unused_entries() {
        let dir = tempfile_path("yaoshi-image-mbr-unused-test");
        fs::create_dir_all(&dir).unwrap();
        let boot = dir.join("boot.fat32");
        let payload = dir.join("installed-system.img");
        let out = dir.join("yaoshi.img");
        File::create(&boot)
            .unwrap()
            .set_len(INSTALLER_BOOT_SIZE_BYTES)
            .unwrap();
        File::create(&payload)
            .unwrap()
            .write_all(&vec![0x5a; 1024 * 1024])
            .unwrap();
        let layout = InstallerMbrLayout {
            installed_system_bytes: 1024 * 1024,
        };
        write_mbr_installer_image(&out, &boot, &payload, &layout).unwrap();
        let mut file = OpenOptions::new().write(true).open(&out).unwrap();
        file.seek(SeekFrom::Start(446 + 2 * 16 + 4)).unwrap();
        file.write_all(&[0x83]).unwrap();
        assert!(parse_mbr(&out).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn gpt_round_trips_partition_names() {
        let dir = tempfile_path("yaoshi-gpt-test");
        fs::create_dir_all(&dir).unwrap();
        let esp = dir.join("esp.fat32");
        let root = dir.join("root.ext4");
        let out = dir.join("installed-system.img");
        File::create(&esp)
            .unwrap()
            .set_len(INSTALLED_ESP_SIZE_BYTES)
            .unwrap();
        File::create(&root)
            .unwrap()
            .set_len(2 * 1024 * 1024)
            .unwrap();
        let layout = InstalledGptLayout::new(2 * 1024 * 1024);
        write_installed_gpt_image(&out, &esp, &root, &layout).unwrap();
        let parts = parse_gpt(&out, Some(layout.image_bytes()))
            .unwrap()
            .partitions;
        assert_eq!(parts[0].name, INSTALLED_ESP_NAME);
        assert_eq!(parts[1].name, INSTALLED_ROOT_NAME);
        assert_eq!(parts[1].unique_guid, layout.root_guid);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn installed_target_layout_graph_is_canonical_and_uses_gpt_extents() {
        let layout = InstalledGptLayout::fixed(yaoshi_common::INSTALLED_ROOT_MINIMUM_BYTES);
        let bytes = installed_target_layout_graph_bytes(&layout, &"66".repeat(32)).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains(r#""grammar":"yaoshi.virtual-installed-target-graph.v1""#));
        assert!(text.contains(
            r#""byte_sources":["gpt","installed-esp-fat32","installed-root-ext4","zero"]"#
        ));
        assert!(!text.contains(&["installed_target", "_raw_sha256"].concat()));
        assert!(!text.contains(&["target_write", "_plan_sha256"].concat()));
        let path = tempfile_path("yaoshi-layout-graph-json");
        fs::write(&path, text.as_bytes()).unwrap();
        assert_eq!(
            validate_installed_target_layout_graph(&path, &layout).unwrap(),
            Sha256Hex::digest_bytes(text.as_bytes()).to_string()
        );
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("extra".to_string(), json!(true));
        fs::write(&path, canonical_json_bytes(&value).unwrap()).unwrap();
        assert!(validate_installed_target_layout_graph(&path, &layout).is_err());
        let _ = fs::remove_file(&path);

        let extents = gpt_required_extents(&layout).unwrap();
        assert_eq!(extents.len(), 2);
        assert!(
            extents
                .iter()
                .all(|extent| extent.target_logical_offset % 4096 == 0
                    && extent.length % 4096 == 0)
        );
    }

    #[test]
    fn virtual_target_graph_covers_complete_image_with_zero_extents() {
        let dir = tempfile_path("yaoshi-virtual-target-graph");
        fs::create_dir_all(&dir).unwrap();
        let esp = dir.join("esp.fat32");
        let root = dir.join("root.ext4");
        File::create(&esp)
            .unwrap()
            .set_len(INSTALLED_ESP_SIZE_BYTES)
            .unwrap();
        let mut root_bytes = vec![0u8; 8 * 4096];
        root_bytes[0..4096].fill(0x11);
        root_bytes[4 * 4096..5 * 4096].fill(0x22);
        fs::write(&root, &root_bytes).unwrap();
        let layout = InstalledGptLayout::fixed(root_bytes.len() as u64);
        let graph = VirtualInstalledTargetGraph::new(layout.clone(), &esp, &root).unwrap();
        let mut extents = Vec::new();
        graph
            .for_each_extent(|extent| {
                extents.push(extent);
                Ok(())
            })
            .unwrap();
        assert_eq!(graph.target_image_bytes(), layout.image_bytes());
        assert_eq!(extents.first().unwrap().logical_offset, 0);
        assert_eq!(
            extents.last().unwrap().logical_offset + extents.last().unwrap().bytes.len() as u64,
            layout.image_bytes()
        );
        assert!(
            extents
                .iter()
                .any(|extent| extent.kind == TargetWriteExtentKind::Zero)
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn gpt_parser_accepts_generated_image_on_larger_media() {
        let dir = tempfile_path("yaoshi-gpt-larger-media-test");
        fs::create_dir_all(&dir).unwrap();
        let esp = dir.join("esp.fat32");
        let root = dir.join("root.ext4");
        let out = dir.join("target.img");
        File::create(&esp)
            .unwrap()
            .set_len(INSTALLED_ESP_SIZE_BYTES)
            .unwrap();
        File::create(&root)
            .unwrap()
            .set_len(2 * 1024 * 1024)
            .unwrap();
        let layout = InstalledGptLayout::new(2 * 1024 * 1024);
        write_installed_gpt_image(&out, &esp, &root, &layout).unwrap();
        OpenOptions::new()
            .write(true)
            .open(&out)
            .unwrap()
            .set_len(layout.image_bytes() + 16 * 1024 * 1024)
            .unwrap();
        let disk = parse_gpt(&out, None).unwrap();
        assert_eq!(disk.accepted_image_span, layout.image_bytes());
        assert_eq!(disk.partitions.len(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn gpt_parser_rejects_corrupt_partition_range_without_overflow() {
        let dir = tempfile_path("yaoshi-gpt-corrupt-range-test");
        fs::create_dir_all(&dir).unwrap();
        let esp = dir.join("esp.fat32");
        let root = dir.join("root.ext4");
        let out = dir.join("installed-system.img");
        File::create(&esp)
            .unwrap()
            .set_len(INSTALLED_ESP_SIZE_BYTES)
            .unwrap();
        File::create(&root)
            .unwrap()
            .set_len(2 * 1024 * 1024)
            .unwrap();
        let layout = InstalledGptLayout::fixed(2 * 1024 * 1024);
        write_installed_gpt_image(&out, &esp, &root, &layout).unwrap();
        let mut file = OpenOptions::new().write(true).open(&out).unwrap();
        file.seek(SeekFrom::Start(2 * SECTOR_SIZE + 40)).unwrap();
        file.write_all(&u64::MAX.to_le_bytes()).unwrap();
        drop(file);
        update_gpt_crcs(&out, &layout);
        assert!(parse_gpt(&out, Some(layout.image_bytes())).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn strict_installed_payload_rejects_guid_mismatch() {
        let dir = tempfile_path("yaoshi-strict-payload-guid-test");
        fs::create_dir_all(&dir).unwrap();
        let esp = dir.join("esp.fat32");
        let root = dir.join("root.ext4");
        let out = dir.join("installed-system.img");
        let layout = InstalledGptLayout::fixed(2 * 1024 * 1024);
        let cmdline = format!(
            "{}{}{}",
            INSTALLED_CMDLINE_PREFIX,
            layout
                .root_guid
                .hyphenated()
                .to_string()
                .to_ascii_lowercase(),
            INSTALLED_CMDLINE_SUFFIX
        );
        create_fat32_image(
            &esp,
            INSTALLED_ESP_LABEL,
            &[
                TreeFile {
                    path: PathBuf::from(EFI_BOOT_PATH),
                    bytes: b"MZ systemd-boot".to_vec(),
                },
                TreeFile {
                    path: PathBuf::from("loader/loader.conf"),
                    bytes: b"default yaoshi.conf\ntimeout 0\neditor no\n".to_vec(),
                },
                TreeFile {
                    path: PathBuf::from("loader/entries/yaoshi.conf"),
                    bytes: format!(
                        "title Yaoshi Debian trixie\nlinux /YAOSHI/BOOT/VMLINUZ\ninitrd /YAOSHI/BOOT/INITRD.IMG\noptions {cmdline}\n"
                    )
                    .into_bytes(),
                },
                TreeFile {
                    path: PathBuf::from(INSTALLED_KERNEL_PATH),
                    bytes: b"MZ debian kernel".to_vec(),
                },
                TreeFile {
                    path: PathBuf::from(INSTALLED_INITRD_PATH),
                    bytes: b"initrd".to_vec(),
                },
                TreeFile {
                    path: PathBuf::from(INSTALLED_KERNEL_RELEASE_PATH),
                    bytes: b"6.12.0-test\n".to_vec(),
                },
                TreeFile {
                    path: PathBuf::from("YAOSHI/CONFIG/HOSTNAME"),
                    bytes: b"yaoshi\n".to_vec(),
                },
                TreeFile {
                    path: PathBuf::from("YAOSHI/CONFIG/AUTHKEYS"),
                    bytes: b"ssh-ed25519 AAAA test\n".to_vec(),
                },
                TreeFile {
                    path: PathBuf::from("YAOSHI/RUNTIME/PREPARE"),
                    bytes: b"#!/bin/bash\nexit 0\n".to_vec(),
                },
                TreeFile {
                    path: PathBuf::from("YAOSHI/RUNTIME/FIRST-BOOT"),
                    bytes: b"#!/bin/bash\nexit 0\n".to_vec(),
                },
                TreeFile {
                    path: PathBuf::from("YAOSHI/DASHBOARD/YAOSHI-DASHBOARD"),
                    bytes: fake_elf64_x86_64(1),
                },
            ],
        )
        .unwrap();
        write_fake_ext4(&root, layout.root_bytes);
        write_installed_gpt_image(&out, &esp, &root, &layout).unwrap();
        let payload = PartitionLayout {
            number: 2,
            start_byte: 0,
            byte_size: layout.image_bytes(),
        };
        check_installed_system_partition_strict(&out, &payload).unwrap();

        let mut file = OpenOptions::new().write(true).open(&out).unwrap();
        file.seek(SeekFrom::Start(2 * SECTOR_SIZE + 16)).unwrap();
        file.write_all(&Uuid::nil().to_bytes_le()).unwrap();
        drop(file);
        update_gpt_crcs(&out, &layout);
        assert!(check_installed_system_partition_strict(&out, &payload).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
