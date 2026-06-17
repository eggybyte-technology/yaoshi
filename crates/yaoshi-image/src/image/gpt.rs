pub fn write_installed_gpt_image(
    path: &Path,
    esp_image: &Path,
    root_image: &Path,
    layout: &InstalledGptLayout,
) -> YaoshiResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| YaoshiError::image(format!("create GPT image: {e}")))?;
    file.set_len(layout.image_bytes())
        .map_err(|e| YaoshiError::image(format!("size GPT image: {e}")))?;
    write_protective_mbr(&mut file, layout.image_bytes())?;
    let entries = gpt_entries(layout);
    let entry_bytes = encode_gpt_entries(&entries);
    let last_lba = layout.image_bytes() / SECTOR_SIZE - 1;
    let backup_entries_lba = last_lba - 32;
    write_gpt_header_at(
        &mut file,
        1,
        last_lba,
        2,
        last_lba,
        &layout.disk_guid,
        &entry_bytes,
    )?;
    file.seek(SeekFrom::Start(2 * SECTOR_SIZE))
        .map_err(|e| YaoshiError::image(format!("seek primary GPT entries: {e}")))?;
    file.write_all(&entry_bytes)
        .map_err(|e| YaoshiError::image(format!("write primary GPT entries: {e}")))?;
    file.seek(SeekFrom::Start(backup_entries_lba * SECTOR_SIZE))
        .map_err(|e| YaoshiError::image(format!("seek backup GPT entries: {e}")))?;
    file.write_all(&entry_bytes)
        .map_err(|e| YaoshiError::image(format!("write backup GPT entries: {e}")))?;
    write_gpt_header_at(
        &mut file,
        last_lba,
        1,
        backup_entries_lba,
        last_lba,
        &layout.disk_guid,
        &entry_bytes,
    )?;
    copy_file_exact_at(
        esp_image,
        &mut file,
        layout.esp_partition().start_byte,
        layout.esp_partition().byte_size,
    )?;
    copy_file_exact_at(
        root_image,
        &mut file,
        layout.root_partition().start_byte,
        layout.root_partition().byte_size,
    )?;
    Ok(())
}

fn write_protective_mbr(file: &mut File, image_bytes: u64) -> YaoshiResult<()> {
    let sector = protective_mbr_bytes(image_bytes);
    file.seek(SeekFrom::Start(0))
        .map_err(|e| YaoshiError::image(format!("seek PMBR: {e}")))?;
    file.write_all(&sector)
        .map_err(|e| YaoshiError::image(format!("write PMBR: {e}")))
}

fn protective_mbr_bytes(image_bytes: u64) -> [u8; 512] {
    let mut sector = [0u8; 512];
    sector[446 + 4] = 0xEE;
    sector[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    let sectors = (image_bytes / SECTOR_SIZE - 1).min(u32::MAX as u64) as u32;
    sector[446 + 12..446 + 16].copy_from_slice(&sectors.to_le_bytes());
    sector[510] = 0x55;
    sector[511] = 0xaa;
    sector
}

fn gpt_entries(layout: &InstalledGptLayout) -> Vec<GptPartition> {
    let esp = layout.esp_partition();
    let root = layout.root_partition();
    vec![
        GptPartition {
            number: 1,
            type_guid: Uuid::parse_str(ESP_TYPE_GUID).unwrap(),
            unique_guid: layout.esp_guid,
            start_lba: esp.start_lba(),
            end_lba: esp.start_lba() + esp.sector_count() - 1,
            name: INSTALLED_ESP_NAME.to_string(),
        },
        GptPartition {
            number: 2,
            type_guid: Uuid::parse_str(X86_64_ROOT_TYPE_GUID).unwrap(),
            unique_guid: layout.root_guid,
            start_lba: root.start_lba(),
            end_lba: root.start_lba() + root.sector_count() - 1,
            name: INSTALLED_ROOT_NAME.to_string(),
        },
    ]
}

fn encode_gpt_entries(entries: &[GptPartition]) -> Vec<u8> {
    let mut out = vec![0u8; 128 * 128];
    for entry in entries {
        let offset = (entry.number as usize - 1) * 128;
        out[offset..offset + 16].copy_from_slice(&entry.type_guid.to_bytes_le());
        out[offset + 16..offset + 32].copy_from_slice(&entry.unique_guid.to_bytes_le());
        out[offset + 32..offset + 40].copy_from_slice(&entry.start_lba.to_le_bytes());
        out[offset + 40..offset + 48].copy_from_slice(&entry.end_lba.to_le_bytes());
        for (i, unit) in entry.name.encode_utf16().take(36).enumerate() {
            out[offset + 56 + i * 2..offset + 58 + i * 2].copy_from_slice(&unit.to_le_bytes());
        }
    }
    out
}

fn encode_gpt_header(
    current_lba: u64,
    alternate_lba: u64,
    entries_lba: u64,
    last_lba: u64,
    disk_guid: &Uuid,
    entry_bytes: &[u8],
) -> [u8; 512] {
    let mut header = [0u8; 512];
    header[0..8].copy_from_slice(b"EFI PART");
    header[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    header[12..16].copy_from_slice(&92u32.to_le_bytes());
    header[24..32].copy_from_slice(&current_lba.to_le_bytes());
    header[32..40].copy_from_slice(&alternate_lba.to_le_bytes());
    header[40..48].copy_from_slice(&34u64.to_le_bytes());
    header[48..56].copy_from_slice(&(last_lba - 33).to_le_bytes());
    header[56..72].copy_from_slice(&disk_guid.to_bytes_le());
    header[72..80].copy_from_slice(&entries_lba.to_le_bytes());
    header[80..84].copy_from_slice(&128u32.to_le_bytes());
    header[84..88].copy_from_slice(&128u32.to_le_bytes());
    let mut hasher = Hasher::new();
    hasher.update(entry_bytes);
    header[88..92].copy_from_slice(&hasher.finalize().to_le_bytes());
    let mut h = header;
    h[16..20].fill(0);
    let mut hasher = Hasher::new();
    hasher.update(&h[..92]);
    header[16..20].copy_from_slice(&hasher.finalize().to_le_bytes());
    header
}

fn write_gpt_header_at(
    file: &mut File,
    current_lba: u64,
    alternate_lba: u64,
    entries_lba: u64,
    last_lba: u64,
    disk_guid: &Uuid,
    entry_bytes: &[u8],
) -> YaoshiResult<()> {
    let header = encode_gpt_header(
        current_lba,
        alternate_lba,
        entries_lba,
        last_lba,
        disk_guid,
        entry_bytes,
    );
    file.seek(SeekFrom::Start(current_lba * SECTOR_SIZE))
        .map_err(|e| YaoshiError::image(format!("seek GPT header: {e}")))?;
    file.write_all(&header)
        .map_err(|e| YaoshiError::image(format!("write GPT header: {e}")))?;
    Ok(())
}

pub fn gpt_required_extents(layout: &InstalledGptLayout) -> YaoshiResult<Vec<GptRequiredExtent>> {
    Ok(gpt_required_extent_bytes(layout)?
        .into_iter()
        .map(|extent| GptRequiredExtent {
            name: extent.name,
            target_logical_offset: extent.target_logical_offset,
            length: extent.bytes.len() as u64,
            sha256: Sha256Hex::digest_bytes(&extent.bytes).to_string(),
        })
        .collect())
}

pub fn gpt_required_extent_bytes(
    layout: &InstalledGptLayout,
) -> YaoshiResult<Vec<GptRequiredExtentBytes>> {
    let entry_bytes = encode_gpt_entries(&gpt_entries(layout));
    let image_bytes = layout.image_bytes();
    let last_lba = image_bytes / SECTOR_SIZE - 1;
    let backup_entries_lba = last_lba - 32;

    let mut primary = vec![0u8; 5 * 4096];
    primary[0..512].copy_from_slice(&protective_mbr_bytes(image_bytes));
    primary[512..1024].copy_from_slice(&encode_gpt_header(
        1,
        last_lba,
        2,
        last_lba,
        &layout.disk_guid,
        &entry_bytes,
    ));
    primary[1024..1024 + entry_bytes.len()].copy_from_slice(&entry_bytes);

    let backup_start = round_down(backup_entries_lba * SECTOR_SIZE, 4096);
    let backup_len = image_bytes - backup_start;
    let mut backup = vec![
        0u8;
        usize::try_from(backup_len)
            .map_err(|_| YaoshiError::image("backup GPT extent too large"))?
    ];
    let entries_offset = usize::try_from(backup_entries_lba * SECTOR_SIZE - backup_start)
        .map_err(|_| YaoshiError::image("backup GPT entries offset too large"))?;
    backup[entries_offset..entries_offset + entry_bytes.len()].copy_from_slice(&entry_bytes);
    let header_offset = usize::try_from(last_lba * SECTOR_SIZE - backup_start)
        .map_err(|_| YaoshiError::image("backup GPT header offset too large"))?;
    backup[header_offset..header_offset + 512].copy_from_slice(&encode_gpt_header(
        last_lba,
        1,
        backup_entries_lba,
        last_lba,
        &layout.disk_guid,
        &entry_bytes,
    ));

    Ok(vec![
        GptRequiredExtentBytes {
            name: "primary-gpt",
            target_logical_offset: 0,
            bytes: primary,
        },
        GptRequiredExtentBytes {
            name: "backup-gpt",
            target_logical_offset: backup_start,
            bytes: backup,
        },
    ])
}

fn ranges_overlap(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start < b_end && b_start < a_end
}

fn overlay_range(
    block: &mut [u8],
    block_start: u64,
    block_end: u64,
    source_start: u64,
    source: &[u8],
) -> YaoshiResult<()> {
    let source_end = source_start
        .checked_add(source.len() as u64)
        .ok_or_else(|| YaoshiError::image("virtual target source range overflows"))?;
    if !ranges_overlap(block_start, block_end, source_start, source_end) {
        return Ok(());
    }
    let copy_start = block_start.max(source_start);
    let copy_end = block_end.min(source_end);
    let dst = usize::try_from(copy_start - block_start)
        .map_err(|_| YaoshiError::image("virtual target block offset too large"))?;
    let src = usize::try_from(copy_start - source_start)
        .map_err(|_| YaoshiError::image("virtual target source offset too large"))?;
    let len = usize::try_from(copy_end - copy_start)
        .map_err(|_| YaoshiError::image("virtual target copy length too large"))?;
    block[dst..dst + len].copy_from_slice(&source[src..src + len]);
    Ok(())
}

fn overlay_file_range(
    block: &mut [u8],
    block_start: u64,
    block_end: u64,
    file_target_start: u64,
    file: &mut File,
) -> YaoshiResult<()> {
    let copy_start = block_start.max(file_target_start);
    let copy_end = block_end;
    let dst = usize::try_from(copy_start - block_start)
        .map_err(|_| YaoshiError::image("virtual target file block offset too large"))?;
    let len = usize::try_from(copy_end - copy_start)
        .map_err(|_| YaoshiError::image("virtual target file copy length too large"))?;
    file.seek(SeekFrom::Start(copy_start - file_target_start))
        .and_then(|_| file.read_exact(&mut block[dst..dst + len]))
        .map_err(|e| YaoshiError::image(format!("read virtual target file range: {e}")))
}

pub fn installed_target_layout_graph_bytes(
    layout: &InstalledGptLayout,
    semantic_required_sha256: &str,
) -> YaoshiResult<Vec<u8>> {
    Sha256Hex::parse(semantic_required_sha256.to_string())?;
    let gpt_extents = gpt_required_extents(layout)?;
    let value = json!({
        "byte_sources": [
            "gpt",
            "installed-esp-fat32",
            "installed-root-ext4",
            "zero"
        ],
        "gpt_extents": gpt_extents.iter().map(|extent| json!({
            "length": extent.length,
            "name": extent.name,
            "sha256": extent.sha256,
            "target_logical_offset": extent.target_logical_offset
        })).collect::<Vec<_>>(),
        "grammar": "yaoshi.virtual-installed-target-graph.v1",
        "installed_target_image_bytes": layout.image_bytes(),
        "logical_sector_size": 512,
        "partitions": [
            {
                "byte_size": layout.esp_partition().byte_size,
                "filesystem": "FAT32",
                "filesystem_label": INSTALLED_ESP_LABEL,
                "mountpoint": "/boot",
                "name": INSTALLED_ESP_NAME,
                "number": 1,
                "start_byte_offset": layout.esp_partition().start_byte,
                "type_guid": ESP_TYPE_GUID,
                "unique_partition_guid": INSTALLED_ESP_PARTITION_GUID
            },
            {
                "byte_size": layout.root_partition().byte_size,
                "filesystem": "ext4",
                "filesystem_label": INSTALLED_ROOT_LABEL,
                "mountpoint": "/",
                "name": INSTALLED_ROOT_NAME,
                "number": 2,
                "start_byte_offset": layout.root_partition().start_byte,
                "type_guid": X86_64_ROOT_TYPE_GUID,
                "unique_partition_guid": INSTALLED_ROOT_PARTITION_GUID
            }
        ],
        "required_block_size": 4096,
        "semantic_required_sha256": semantic_required_sha256
    });
    canonical_json_bytes(&value)
}

pub fn write_installed_target_layout_graph(
    path: &Path,
    layout: &InstalledGptLayout,
    semantic_required_sha256: &str,
) -> YaoshiResult<String> {
    let bytes = installed_target_layout_graph_bytes(layout, semantic_required_sha256)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::image(format!("create layout graph parent: {e}")))?;
    }
    fs::write(path, &bytes).map_err(|e| YaoshiError::image(format!("write layout graph: {e}")))?;
    Ok(Sha256Hex::digest_bytes(&bytes).to_string())
}

pub fn validate_installed_target_layout_graph(
    path: &Path,
    layout: &InstalledGptLayout,
) -> YaoshiResult<String> {
    let bytes = fs::read(path)
        .map_err(|e| YaoshiError::image(format!("read installed target layout graph: {e}")))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| YaoshiError::image(format!("parse installed target layout graph: {e}")))?;
    if canonical_json_bytes(&value)? != bytes {
        return Err(YaoshiError::image(
            "installed target layout graph is not canonical JSON",
        ));
    }
    let object = value
        .as_object()
        .ok_or_else(|| YaoshiError::image("installed target layout graph root is not an object"))?;
    let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = [
        "byte_sources",
        "gpt_extents",
        "grammar",
        "installed_target_image_bytes",
        "logical_sector_size",
        "partitions",
        "required_block_size",
        "semantic_required_sha256",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    if keys != expected {
        return Err(YaoshiError::image(
            "installed target layout graph root keys mismatch",
        ));
    }
    expect_str(
        object,
        "grammar",
        "yaoshi.virtual-installed-target-graph.v1",
    )?;
    expect_u64(object, "logical_sector_size", SECTOR_SIZE)?;
    expect_u64(
        object,
        "required_block_size",
        yaoshi_common::REQUIRED_BLOCK_SIZE,
    )?;
    expect_u64(object, "installed_target_image_bytes", layout.image_bytes())?;
    Sha256Hex::parse(expect_string(object, "semantic_required_sha256")?.to_string())?;

    let byte_sources = object
        .get("byte_sources")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| YaoshiError::image("target graph byte_sources are invalid"))?;
    let names = byte_sources
        .iter()
        .map(|component| component.as_str().unwrap_or(""))
        .collect::<Vec<_>>();
    if names != ["gpt", "installed-esp-fat32", "installed-root-ext4", "zero"] {
        return Err(YaoshiError::image("target graph byte source set mismatch"));
    }

    validate_layout_graph_partitions(object, layout)?;
    validate_layout_graph_gpt_extents(object, layout)?;
    Ok(Sha256Hex::digest_bytes(&bytes).to_string())
}

fn validate_layout_graph_partitions(
    object: &serde_json::Map<String, serde_json::Value>,
    layout: &InstalledGptLayout,
) -> YaoshiResult<()> {
    let partitions = object
        .get("partitions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| YaoshiError::image("layout graph partitions are invalid"))?;
    if partitions.len() != 2 {
        return Err(YaoshiError::image("layout graph partition count mismatch"));
    }
    let esp = partitions[0]
        .as_object()
        .ok_or_else(|| YaoshiError::image("layout graph ESP partition is invalid"))?;
    let root = partitions[1]
        .as_object()
        .ok_or_else(|| YaoshiError::image("layout graph root partition is invalid"))?;
    let esp_layout = layout.esp_partition();
    let root_layout = layout.root_partition();
    expect_u64(esp, "number", u64::from(esp_layout.number))?;
    expect_str(esp, "name", INSTALLED_ESP_NAME)?;
    expect_str(esp, "filesystem", "FAT32")?;
    expect_str(esp, "filesystem_label", INSTALLED_ESP_LABEL)?;
    expect_u64(esp, "start_byte_offset", esp_layout.start_byte)?;
    expect_u64(esp, "byte_size", esp_layout.byte_size)?;
    expect_u64(root, "number", u64::from(root_layout.number))?;
    expect_str(root, "name", INSTALLED_ROOT_NAME)?;
    expect_str(root, "filesystem", "ext4")?;
    expect_str(root, "filesystem_label", INSTALLED_ROOT_LABEL)?;
    expect_u64(root, "start_byte_offset", root_layout.start_byte)?;
    expect_u64(root, "byte_size", root_layout.byte_size)?;
    Ok(())
}

fn validate_layout_graph_gpt_extents(
    object: &serde_json::Map<String, serde_json::Value>,
    layout: &InstalledGptLayout,
) -> YaoshiResult<()> {
    let actual = object
        .get("gpt_extents")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| YaoshiError::image("layout graph GPT extents are invalid"))?;
    let expected = gpt_required_extents(layout)?;
    if actual.len() != expected.len() {
        return Err(YaoshiError::image("layout graph GPT extent count mismatch"));
    }
    for (actual, expected) in actual.iter().zip(expected.iter()) {
        let actual = actual
            .as_object()
            .ok_or_else(|| YaoshiError::image("layout graph GPT extent entry is invalid"))?;
        expect_str(actual, "name", expected.name)?;
        expect_u64(
            actual,
            "target_logical_offset",
            expected.target_logical_offset,
        )?;
        expect_u64(actual, "length", expected.length)?;
        expect_str(actual, "sha256", &expected.sha256)?;
    }
    Ok(())
}

fn expect_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> YaoshiResult<&'a str> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| YaoshiError::image(format!("layout graph field {key} is not a string")))
}

fn expect_str(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    expected: &str,
) -> YaoshiResult<()> {
    if expect_string(object, key)? != expected {
        return Err(YaoshiError::image(format!(
            "layout graph field {key} mismatch"
        )));
    }
    Ok(())
}

fn expect_u64(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    expected: u64,
) -> YaoshiResult<()> {
    if object
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| YaoshiError::image(format!("layout graph field {key} is not a number")))?
        != expected
    {
        return Err(YaoshiError::image(format!(
            "layout graph field {key} mismatch"
        )));
    }
    Ok(())
}

fn round_down(value: u64, multiple: u64) -> u64 {
    value / multiple * multiple
}

pub fn parse_gpt(path: &Path, image_span_bytes: Option<u64>) -> YaoshiResult<GptDisk> {
    let physical_len = fs::metadata(path)
        .map_err(|e| YaoshiError::image(format!("stat GPT image: {e}")))?
        .len();
    let Some(span) = image_span_bytes else {
        return parse_gpt_region(path, 0, physical_len, false);
    };
    parse_gpt_region(path, 0, span, true)
}

pub fn parse_gpt_physical(path: &Path, physical_span_bytes: u64) -> YaoshiResult<GptDisk> {
    parse_gpt_region(path, 0, physical_span_bytes, false)
}

fn parse_gpt_region(
    path: &Path,
    image_start: u64,
    image_span_bytes: u64,
    span_is_explicit: bool,
) -> YaoshiResult<GptDisk> {
    if image_span_bytes == 0 || !image_span_bytes.is_multiple_of(SECTOR_SIZE) {
        return Err(YaoshiError::image("invalid GPT image span"));
    }
    let metadata =
        fs::metadata(path).map_err(|e| YaoshiError::image(format!("stat GPT image: {e}")))?;
    let image_end = image_start
        .checked_add(image_span_bytes)
        .ok_or_else(|| YaoshiError::image("GPT image span overflows"))?;
    if metadata.is_file() && image_end > metadata.len() {
        return Err(YaoshiError::image("GPT image span exceeds media size"));
    }
    let mut file = File::open(path).map_err(|e| YaoshiError::image(format!("open GPT: {e}")))?;
    let mut header = [0u8; 512];
    file.seek(SeekFrom::Start(region_offset(
        image_start,
        SECTOR_SIZE,
        "GPT header",
    )?))
    .map_err(|e| YaoshiError::image(format!("seek GPT header: {e}")))?;
    file.read_exact(&mut header)
        .map_err(|e| YaoshiError::image(format!("read GPT header: {e}")))?;
    verify_gpt_header(&header)?;
    let primary_header = parse_gpt_header_fields(&header);
    if primary_header.current_lba != 1 {
        return Err(YaoshiError::image("primary GPT header is not at LBA 1"));
    }
    let alternate_lba = primary_header.alternate_lba;
    let physical_last_lba = image_span_bytes / SECTOR_SIZE - 1;
    if alternate_lba > physical_last_lba {
        return Err(YaoshiError::image(
            "GPT alternate_lba exceeds physical media",
        ));
    }
    if span_is_explicit && alternate_lba != physical_last_lba {
        return Err(YaoshiError::image(
            "GPT alternate_lba does not match explicit image span",
        ));
    }
    let accepted_last_lba = if span_is_explicit {
        physical_last_lba
    } else {
        alternate_lba
    };
    let accepted_span_bytes = accepted_last_lba
        .checked_add(1)
        .and_then(|lba| lba.checked_mul(SECTOR_SIZE))
        .ok_or_else(|| YaoshiError::image("GPT accepted image span overflows"))?;
    let mut backup_header = [0u8; 512];
    file.seek(SeekFrom::Start(region_offset(
        image_start,
        lba_byte_offset(alternate_lba, "backup GPT header")?,
        "backup GPT header",
    )?))
    .map_err(|e| YaoshiError::image(format!("seek backup GPT header: {e}")))?;
    file.read_exact(&mut backup_header)
        .map_err(|e| YaoshiError::image(format!("read backup GPT header: {e}")))?;
    verify_gpt_header(&backup_header)?;
    let backup_header = parse_gpt_header_fields(&backup_header);
    if backup_header.current_lba != alternate_lba || backup_header.alternate_lba != 1 {
        return Err(YaoshiError::image("backup GPT header location mismatch"));
    }
    if backup_header.disk_guid != primary_header.disk_guid {
        return Err(YaoshiError::image("backup GPT disk GUID mismatch"));
    }
    if primary_header.first_usable_lba > primary_header.last_usable_lba
        || backup_header.first_usable_lba != primary_header.first_usable_lba
        || backup_header.last_usable_lba != primary_header.last_usable_lba
    {
        return Err(YaoshiError::image("GPT usable LBA range mismatch"));
    }
    let entry_lba = primary_header.partition_entries_lba;
    let count = primary_header.partition_entry_count;
    let size = primary_header.partition_entry_size;
    if count != 128 || size != 128 {
        return Err(YaoshiError::image("unexpected GPT partition array shape"));
    }
    if entry_lba != 2 {
        return Err(YaoshiError::image(
            "primary GPT partition entry array is not at LBA 2",
        ));
    }
    if backup_header.partition_entry_count != count || backup_header.partition_entry_size != size {
        return Err(YaoshiError::image(
            "backup GPT partition array shape mismatch",
        ));
    }
    let entry_bytes = u64::from(count)
        .checked_mul(u64::from(size))
        .ok_or_else(|| YaoshiError::image("GPT partition entry byte count overflows"))?;
    let entry_start = lba_byte_offset(entry_lba, "GPT entries")?;
    if entry_start
        .checked_add(entry_bytes)
        .is_none_or(|end| end > accepted_span_bytes)
    {
        return Err(YaoshiError::image(
            "primary GPT partition entry array exceeds accepted image span",
        ));
    }
    let mut entries = vec![0u8; entry_bytes as usize];
    file.seek(SeekFrom::Start(region_offset(
        image_start,
        entry_start,
        "GPT entries",
    )?))
    .map_err(|e| YaoshiError::image(format!("seek GPT entries: {e}")))?;
    file.read_exact(&mut entries)
        .map_err(|e| YaoshiError::image(format!("read GPT entries: {e}")))?;
    let mut hasher = Hasher::new();
    hasher.update(&entries);
    if hasher.finalize() != primary_header.partition_entries_crc32 {
        return Err(YaoshiError::image(
            "invalid primary GPT partition array CRC",
        ));
    }
    let backup_entries_lba = backup_header.partition_entries_lba;
    let backup_entries_bytes = entry_bytes;
    let backup_entries_start = lba_byte_offset(backup_entries_lba, "backup GPT entries")?;
    let backup_header_start = lba_byte_offset(alternate_lba, "backup GPT header")?;
    if backup_entries_start.checked_add(backup_entries_bytes) != Some(backup_header_start) {
        return Err(YaoshiError::image(
            "backup GPT entry array is not immediately before backup header",
        ));
    }
    let mut backup_entries = vec![0u8; backup_entries_bytes as usize];
    file.seek(SeekFrom::Start(region_offset(
        image_start,
        backup_entries_start,
        "backup GPT entries",
    )?))
    .map_err(|e| YaoshiError::image(format!("seek backup GPT entries: {e}")))?;
    file.read_exact(&mut backup_entries)
        .map_err(|e| YaoshiError::image(format!("read backup GPT entries: {e}")))?;
    let mut hasher = Hasher::new();
    hasher.update(&backup_entries);
    if hasher.finalize() != backup_header.partition_entries_crc32 {
        return Err(YaoshiError::image("invalid backup GPT partition array CRC"));
    }
    if backup_entries != entries {
        return Err(YaoshiError::image(
            "backup GPT partition array differs from primary",
        ));
    }
    let mut parsed = Vec::new();
    for i in 0..count as usize {
        let offset = i * 128;
        if entries[offset..offset + 16].iter().all(|b| *b == 0) {
            continue;
        }
        let start_lba = u64::from_le_bytes(entries[offset + 32..offset + 40].try_into().unwrap());
        let end_lba = u64::from_le_bytes(entries[offset + 40..offset + 48].try_into().unwrap());
        if start_lba > end_lba {
            return Err(YaoshiError::image("GPT partition start exceeds end"));
        }
        if start_lba < primary_header.first_usable_lba || end_lba > primary_header.last_usable_lba {
            return Err(YaoshiError::image(
                "GPT partition is outside usable LBA range",
            ));
        }
        let end_exclusive_bytes = end_lba
            .checked_add(1)
            .and_then(|lba| lba.checked_mul(SECTOR_SIZE))
            .ok_or_else(|| YaoshiError::image("GPT partition byte range overflows"))?;
        if end_exclusive_bytes > accepted_span_bytes {
            return Err(YaoshiError::image(
                "GPT partition exceeds accepted image span",
            ));
        }
        let mut units = Vec::new();
        for chunk in entries[offset + 56..offset + 128].chunks_exact(2) {
            let unit = u16::from_le_bytes([chunk[0], chunk[1]]);
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        parsed.push(GptPartition {
            number: i as u32 + 1,
            type_guid: Uuid::from_bytes_le(entries[offset..offset + 16].try_into().unwrap()),
            unique_guid: Uuid::from_bytes_le(entries[offset + 16..offset + 32].try_into().unwrap()),
            start_lba,
            end_lba,
            name: String::from_utf16(&units)
                .map_err(|_| YaoshiError::image("invalid GPT partition name"))?,
        });
    }
    Ok(GptDisk {
        disk_guid: primary_header.disk_guid,
        primary_header,
        backup_header,
        primary_partition_entries: entries,
        backup_partition_entries: backup_entries,
        accepted_image_span: accepted_span_bytes,
        partitions: parsed,
    })
}

fn parse_gpt_header_fields(header: &[u8; 512]) -> GptHeader {
    GptHeader {
        current_lba: u64::from_le_bytes(header[24..32].try_into().unwrap()),
        alternate_lba: u64::from_le_bytes(header[32..40].try_into().unwrap()),
        first_usable_lba: u64::from_le_bytes(header[40..48].try_into().unwrap()),
        last_usable_lba: u64::from_le_bytes(header[48..56].try_into().unwrap()),
        disk_guid: Uuid::from_bytes_le(header[56..72].try_into().unwrap()),
        partition_entries_lba: u64::from_le_bytes(header[72..80].try_into().unwrap()),
        partition_entry_count: u32::from_le_bytes(header[80..84].try_into().unwrap()),
        partition_entry_size: u32::from_le_bytes(header[84..88].try_into().unwrap()),
        partition_entries_crc32: u32::from_le_bytes(header[88..92].try_into().unwrap()),
    }
}

fn region_offset(base: u64, relative: u64, context: &str) -> YaoshiResult<u64> {
    base.checked_add(relative)
        .ok_or_else(|| YaoshiError::image(format!("{context} offset overflows")))
}

fn lba_byte_offset(lba: u64, context: &str) -> YaoshiResult<u64> {
    lba.checked_mul(SECTOR_SIZE)
        .ok_or_else(|| YaoshiError::image(format!("{context} LBA offset overflows")))
}

fn verify_gpt_header(header: &[u8; 512]) -> YaoshiResult<()> {
    if &header[0..8] != b"EFI PART" {
        return Err(YaoshiError::image("missing GPT signature"));
    }
    let header_size = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
    if !(92..=512).contains(&header_size) {
        return Err(YaoshiError::image("invalid GPT header size"));
    }
    let expected = u32::from_le_bytes(header[16..20].try_into().unwrap());
    let mut copy = *header;
    copy[16..20].fill(0);
    let mut hasher = Hasher::new();
    hasher.update(&copy[..header_size]);
    if hasher.finalize() != expected {
        return Err(YaoshiError::image("invalid GPT header CRC"));
    }
    Ok(())
}
