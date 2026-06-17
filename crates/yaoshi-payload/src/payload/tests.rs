#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_round_trip_writes_planned_data_and_zero_extents() {
        let base = std::env::temp_dir().join(format!(
            "yaoshi-payload-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        let esp = base.join("esp.fat32");
        File::create(&esp)
            .unwrap()
            .set_len(yaoshi_common::INSTALLED_ESP_SIZE_BYTES)
            .unwrap();
        let root = base.join("root.ext4");
        let mut root_bytes = vec![0u8; 16 * 4096];
        root_bytes[0..4096].fill(0x11);
        root_bytes[6 * 4096..7 * 4096].fill(0x22);
        fs::write(&root, &root_bytes).unwrap();
        let layout = yaoshi_common::InstalledGptLayout::fixed(root_bytes.len() as u64);
        let graph =
            yaoshi_image::VirtualInstalledTargetGraph::new(layout.clone(), &esp, &root).unwrap();
        let out = base.join("installed-system.ypayload");
        let esp_part = layout.esp_partition();
        let root_part = layout.root_partition();
        let meta = PayloadBuildMeta {
            esp_start: esp_part.start_byte,
            esp_size: esp_part.byte_size,
            root_start: root_part.start_byte,
            root_size: root_part.byte_size,
        };
        let info = build_payload_from_virtual_target_graph(&graph, &out, &meta).unwrap();
        assert_eq!(info.target_image_bytes, layout.image_bytes());
        assert_eq!(info.planned_extent_bytes, layout.image_bytes());
        assert_eq!(info.omitted_target_bytes, 0);
        let install = base.join("install.raw");
        File::create(&install)
            .unwrap()
            .set_len(layout.image_bytes())
            .unwrap();
        let mut install_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&install)
            .unwrap();
        let mut progress = |_p: PayloadWriteProgress| {};
        write_payload_to_target(&out, &mut install_file, &mut progress).unwrap();
        let installed = fs::read(&install).unwrap();
        assert_eq!(
            &installed
                [root_part.start_byte as usize..root_part.start_byte as usize + root_bytes.len()],
            root_bytes.as_slice()
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn payload_extent_encoding_cache_reuses_valid_nonzero_extent() {
        let base = std::env::temp_dir().join(format!(
            "yaoshi-payload-encoding-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        let cache = PayloadEncodingCache {
            root: base.join("payload-encoding"),
            installed_esp_fat32_sha256: "0".repeat(64),
            installed_root_ext4_identity: "1".repeat(64),
            gpt_source_sha256: "2".repeat(64),
        };
        let meta = PayloadBuildMeta {
            esp_start: 4096,
            esp_size: 4096,
            root_start: 8192,
            root_size: 8192,
        };
        let planned = yaoshi_image::VirtualTargetExtent {
            logical_offset: 8192,
            bytes: vec![0x5a; 4096],
            kind: yaoshi_image::TargetWriteExtentKind::Data,
        };
        let uncompressed_sha256 = sha256_array(&planned.bytes);
        let first =
            encode_extent_cached(&planned, &uncompressed_sha256, &meta, Some(&cache)).unwrap();
        assert!(fs::read_dir(&cache.root).unwrap().next().is_some());
        let second =
            encode_extent_cached(&planned, &uncompressed_sha256, &meta, Some(&cache)).unwrap();
        assert_eq!(first, second);
        fs::remove_dir_all(base).unwrap();
    }
}
