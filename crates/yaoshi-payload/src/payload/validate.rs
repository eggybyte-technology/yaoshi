pub fn validate_payload(path: &Path) -> YaoshiResult<PayloadInfo> {
    let mut file =
        File::open(path).map_err(|e| YaoshiError::image(format!("open payload: {e}")))?;
    let len = file
        .metadata()
        .map_err(|e| YaoshiError::image(format!("stat payload: {e}")))?
        .len();
    validate_payload_region_reader(&mut file, 0, len)
}

pub fn validate_payload_metadata(path: &Path) -> YaoshiResult<PayloadInfo> {
    let mut file =
        File::open(path).map_err(|e| YaoshiError::image(format!("open payload: {e}")))?;
    let len = file
        .metadata()
        .map_err(|e| YaoshiError::image(format!("stat payload: {e}")))?
        .len();
    validate_payload_region_metadata_reader(&mut file, 0, len)
}

pub fn validate_payload_region(path: &Path, offset: u64, len: u64) -> YaoshiResult<PayloadInfo> {
    let mut file =
        File::open(path).map_err(|e| YaoshiError::image(format!("open payload media: {e}")))?;
    validate_payload_region_reader(&mut file, offset, len)
}

pub fn validate_payload_region_metadata(
    path: &Path,
    offset: u64,
    len: u64,
) -> YaoshiResult<PayloadInfo> {
    let mut file =
        File::open(path).map_err(|e| YaoshiError::image(format!("open payload media: {e}")))?;
    validate_payload_region_metadata_reader(&mut file, offset, len)
}

fn validate_payload_region_reader(
    file: &mut File,
    base_offset: u64,
    len: u64,
) -> YaoshiResult<PayloadInfo> {
    if len == 0 || !len.is_multiple_of(PAYLOAD_CONTAINER_ALIGNMENT_BYTES) {
        return Err(YaoshiError::image(
            "payload size is not a positive 512-byte multiple",
        ));
    }
    let mut prelude = [0u8; PRELUDE_LEN];
    file.seek(SeekFrom::Start(base_offset))
        .and_then(|_| file.read_exact(&mut prelude))
        .map_err(|e| YaoshiError::image(format!("read payload prelude: {e}")))?;
    let parsed = parse_prelude(&prelude, len)?;
    let manifest = read_region(
        file,
        base_offset + parsed.manifest_offset,
        parsed.manifest_len,
    )?;
    if sha256_array(&manifest) != parsed.manifest_sha256 {
        return Err(YaoshiError::image("payload manifest sha256 mismatch"));
    }
    validate_manifest(&manifest, &parsed)?;
    let table = read_region(
        file,
        base_offset + parsed.extent_table_offset,
        parsed.extent_table_len,
    )?;
    if sha256_array(&table) != parsed.extent_table_sha256 {
        return Err(YaoshiError::image("payload extent table sha256 mismatch"));
    }
    let blob = read_region(
        file,
        base_offset + parsed.blob_area_offset,
        parsed.blob_area_len,
    )?;
    if sha256_array(&blob) != parsed.blob_area_sha256 {
        return Err(YaoshiError::image("payload blob area sha256 mismatch"));
    }
    let entries = parse_extent_table(&table, &parsed)?;
    validate_entries(&entries, &blob, &parsed)?;
    Ok(PayloadInfo {
        total_payload_bytes: parsed.total_payload_bytes,
        target_image_bytes: parsed.target_image_bytes,
        target_minimum_bytes: parsed.target_minimum_bytes,
        planned_extent_bytes: parsed.planned_extent_bytes,
        planned_zero_bytes: parsed.planned_zero_bytes,
        omitted_target_bytes: parsed.omitted_target_bytes,
        extent_count: parsed.extent_count,
        blob_area_offset: parsed.blob_area_offset,
        blob_area_len: parsed.blob_area_len,
        semantic_required_sha256: parsed.semantic_required_sha256,
    })
}

fn validate_payload_region_metadata_reader(
    file: &mut File,
    base_offset: u64,
    len: u64,
) -> YaoshiResult<PayloadInfo> {
    if len == 0 || !len.is_multiple_of(PAYLOAD_CONTAINER_ALIGNMENT_BYTES) {
        return Err(YaoshiError::image(
            "payload size is not a positive 512-byte multiple",
        ));
    }
    let mut prelude = [0u8; PRELUDE_LEN];
    file.seek(SeekFrom::Start(base_offset))
        .and_then(|_| file.read_exact(&mut prelude))
        .map_err(|e| YaoshiError::image(format!("read payload prelude: {e}")))?;
    let parsed = parse_prelude(&prelude, len)?;
    let manifest = read_region(
        file,
        base_offset + parsed.manifest_offset,
        parsed.manifest_len,
    )?;
    if sha256_array(&manifest) != parsed.manifest_sha256 {
        return Err(YaoshiError::image("payload manifest sha256 mismatch"));
    }
    validate_manifest(&manifest, &parsed)?;
    let table = read_region(
        file,
        base_offset + parsed.extent_table_offset,
        parsed.extent_table_len,
    )?;
    if sha256_array(&table) != parsed.extent_table_sha256 {
        return Err(YaoshiError::image("payload extent table sha256 mismatch"));
    }
    let entries = parse_extent_table(&table, &parsed)?;
    validate_entries_metadata(&entries, &parsed)?;
    Ok(PayloadInfo {
        total_payload_bytes: parsed.total_payload_bytes,
        target_image_bytes: parsed.target_image_bytes,
        target_minimum_bytes: parsed.target_minimum_bytes,
        planned_extent_bytes: parsed.planned_extent_bytes,
        planned_zero_bytes: parsed.planned_zero_bytes,
        omitted_target_bytes: parsed.omitted_target_bytes,
        extent_count: parsed.extent_count,
        blob_area_offset: parsed.blob_area_offset,
        blob_area_len: parsed.blob_area_len,
        semantic_required_sha256: parsed.semantic_required_sha256,
    })
}

#[derive(Clone)]
struct Prelude {
    manifest_offset: u64,
    manifest_len: u64,
    extent_table_offset: u64,
    extent_table_len: u64,
    blob_area_offset: u64,
    blob_area_len: u64,
    total_payload_bytes: u64,
    target_image_bytes: u64,
    target_minimum_bytes: u64,
    manifest_sha256: [u8; 32],
    extent_table_sha256: [u8; 32],
    blob_area_sha256: [u8; 32],
    semantic_required_sha256: [u8; 32],
    extent_count: u64,
    blob_count: u64,
    planned_extent_bytes: u64,
    planned_zero_bytes: u64,
    omitted_target_bytes: u64,
}

fn parse_prelude(prelude: &[u8; PRELUDE_LEN], file_len: u64) -> YaoshiResult<Prelude> {
    if &prelude[0..16] != PAYLOAD_MAGIC {
        return Err(YaoshiError::image("payload magic mismatch"));
    }
    if get_u32(prelude, 16) != FORMAT_VERSION
        || get_u32(prelude, 20) != PRELUDE_LEN as u32
        || get_u32(prelude, 104) != SECTOR_SIZE as u32
        || get_u32(prelude, 108) != REQUIRED_BLOCK_SIZE as u32
        || get_u32(prelude, 112) != EXTENT_ENTRY_SIZE as u32
        || get_u32(prelude, 116) != COMPRESSION_ZSTD
        || get_i32(prelude, 120) != ZSTD_LEVEL
        || get_u32(prelude, 124) != ZSTD_WINDOW_LOG
    {
        return Err(YaoshiError::image("payload prelude fixed field mismatch"));
    }
    let manifest_offset = get_u64(prelude, 24);
    let manifest_len = get_u64(prelude, 32);
    let manifest_padded_len = get_u64(prelude, 40);
    let extent_table_offset = get_u64(prelude, 48);
    let extent_table_len = get_u64(prelude, 56);
    let blob_area_offset = get_u64(prelude, 64);
    let blob_area_len = get_u64(prelude, 72);
    let total_payload_bytes = get_u64(prelude, 80);
    let target_image_bytes = get_u64(prelude, 88);
    let target_minimum_bytes = get_u64(prelude, 96);
    let extent_count = get_u64(prelude, 256);
    let blob_count = get_u64(prelude, 264);
    let planned_extent_bytes = get_u64(prelude, 272);
    let planned_zero_bytes = get_u64(prelude, 280);
    let encoded_blob_bytes = get_u64(prelude, 288);
    let omitted_target_bytes = get_u64(prelude, 296);
    if manifest_offset != PRELUDE_LEN as u64
        || manifest_padded_len != round_up(manifest_len, PAYLOAD_CONTAINER_ALIGNMENT_BYTES)
        || extent_table_offset != manifest_offset + manifest_padded_len
        || extent_table_len != extent_count * EXTENT_ENTRY_SIZE as u64
        || blob_area_offset != extent_table_offset + extent_table_len
        || blob_area_len != encoded_blob_bytes
        || total_payload_bytes != file_len
        || total_payload_bytes
            != round_up(
                blob_area_offset + blob_area_len,
                PAYLOAD_CONTAINER_ALIGNMENT_BYTES,
            )
        || target_image_bytes != target_minimum_bytes
        || omitted_target_bytes != target_image_bytes.saturating_sub(planned_extent_bytes)
        || omitted_target_bytes != 0
        || planned_zero_bytes > planned_extent_bytes
        || prelude[304..512].iter().any(|b| *b != 0)
    {
        return Err(YaoshiError::image("payload prelude derived field mismatch"));
    }
    Ok(Prelude {
        manifest_offset,
        manifest_len,
        extent_table_offset,
        extent_table_len,
        blob_area_offset,
        blob_area_len,
        total_payload_bytes,
        target_image_bytes,
        target_minimum_bytes,
        manifest_sha256: arr32(&prelude[128..160]),
        extent_table_sha256: arr32(&prelude[160..192]),
        blob_area_sha256: arr32(&prelude[192..224]),
        semantic_required_sha256: arr32(&prelude[224..256]),
        extent_count,
        blob_count,
        planned_extent_bytes,
        planned_zero_bytes,
        omitted_target_bytes,
    })
}

pub fn payload_extent_summaries(path: &Path) -> YaoshiResult<Vec<PayloadExtentSummary>> {
    let mut file =
        File::open(path).map_err(|e| YaoshiError::image(format!("open payload: {e}")))?;
    let len = file
        .metadata()
        .map_err(|e| YaoshiError::image(format!("stat payload: {e}")))?
        .len();
    payload_extent_summaries_region_reader(&mut file, 0, len)
}

pub fn payload_extent_summaries_region(
    path: &Path,
    offset: u64,
    len: u64,
) -> YaoshiResult<Vec<PayloadExtentSummary>> {
    let mut file =
        File::open(path).map_err(|e| YaoshiError::image(format!("open payload media: {e}")))?;
    payload_extent_summaries_region_reader(&mut file, offset, len)
}

fn payload_extent_summaries_region_reader(
    file: &mut File,
    base_offset: u64,
    len: u64,
) -> YaoshiResult<Vec<PayloadExtentSummary>> {
    let mut prelude_raw = [0u8; PRELUDE_LEN];
    file.seek(SeekFrom::Start(base_offset))
        .and_then(|_| file.read_exact(&mut prelude_raw))
        .map_err(|e| YaoshiError::image(format!("read payload prelude: {e}")))?;
    let prelude = parse_prelude(&prelude_raw, len)?;
    let table = read_region(
        file,
        base_offset + prelude.extent_table_offset,
        prelude.extent_table_len,
    )?;
    if sha256_array(&table) != prelude.extent_table_sha256 {
        return Err(YaoshiError::image("payload extent table sha256 mismatch"));
    }
    let entries = parse_extent_table(&table, &prelude)?;
    Ok(entries
        .into_iter()
        .map(|entry| PayloadExtentSummary {
            logical_offset: entry.logical_offset,
            uncompressed_len: entry.uncompressed_len,
            kind: entry.kind,
        })
        .collect())
}

pub fn verify_target_matches_payload_region(
    payload_path: &Path,
    payload_offset: u64,
    payload_len: u64,
    target_path: &Path,
) -> YaoshiResult<()> {
    let mut payload = File::open(payload_path)
        .map_err(|e| YaoshiError::image(format!("open payload media: {e}")))?;
    let mut target = File::open(target_path)
        .map_err(|e| YaoshiError::image(format!("open written target: {e}")))?;
    verify_target_matches_payload_region_reader(
        &mut payload,
        payload_offset,
        payload_len,
        &mut target,
    )
}

fn verify_target_matches_payload_region_reader(
    payload: &mut File,
    base_offset: u64,
    len: u64,
    target: &mut File,
) -> YaoshiResult<()> {
    let mut prelude_raw = [0u8; PRELUDE_LEN];
    payload
        .seek(SeekFrom::Start(base_offset))
        .and_then(|_| payload.read_exact(&mut prelude_raw))
        .map_err(|e| YaoshiError::image(format!("read payload prelude: {e}")))?;
    let prelude = parse_prelude(&prelude_raw, len)?;
    let table = read_region(
        payload,
        base_offset + prelude.extent_table_offset,
        prelude.extent_table_len,
    )?;
    if sha256_array(&table) != prelude.extent_table_sha256 {
        return Err(YaoshiError::image("payload extent table sha256 mismatch"));
    }
    let entries = parse_extent_table(&table, &prelude)?;
    for entry in entries {
        let expected = match entry.kind {
            KIND_ZERO => vec![0u8; entry.uncompressed_len as usize],
            KIND_RAW | KIND_ZSTD => {
                let encoded_offset = prelude
                    .blob_area_offset
                    .checked_add(entry.blob_offset)
                    .ok_or_else(|| YaoshiError::image("payload encoded offset overflows"))?;
                let encoded = read_region(
                    payload,
                    base_offset + encoded_offset,
                    u64::from(entry.encoded_len),
                )?;
                if sha256_array(&encoded) != entry.encoded_sha256 {
                    return Err(YaoshiError::image("payload encoded sha256 mismatch"));
                }
                if entry.kind == KIND_RAW {
                    encoded
                } else {
                    zstd::bulk::decompress(&encoded, entry.uncompressed_len as usize).map_err(
                        |e| YaoshiError::image(format!("decompress payload extent: {e}")),
                    )?
                }
            }
            _ => return Err(YaoshiError::image("payload extent kind unsupported")),
        };
        if sha256_array(&expected) != entry.uncompressed_sha256 {
            return Err(YaoshiError::image("payload uncompressed sha256 mismatch"));
        }
        let actual = read_region(
            target,
            entry.logical_offset,
            u64::from(entry.uncompressed_len),
        )?;
        if actual != expected {
            return Err(YaoshiError::image(format!(
                "written target extent mismatch at byte {}",
                entry.logical_offset
            )));
        }
    }
    Ok(())
}

fn validate_manifest(bytes: &[u8], prelude: &Prelude) -> YaoshiResult<()> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| YaoshiError::image(format!("parse payload manifest: {e}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| YaoshiError::image("payload manifest root is not an object"))?;
    let keys = object.keys().map(String::as_str).collect::<Vec<_>>();
    if keys
        != [
            "format",
            "hashes",
            "partitions",
            "payload",
            "target",
            "version",
            "zstd",
        ]
    {
        return Err(YaoshiError::image("payload manifest root keys mismatch"));
    }
    if object.get("format").and_then(Value::as_str) != Some("yaoshi.payload.v1")
        || object.get("version").and_then(Value::as_str) != Some(VERSION)
    {
        return Err(YaoshiError::image(
            "payload manifest format/version mismatch",
        ));
    }
    expect_u64(&value, "/target/image_bytes", prelude.target_image_bytes)?;
    expect_u64(
        &value,
        "/target/minimum_bytes",
        prelude.target_minimum_bytes,
    )?;
    expect_u64(&value, "/target/logical_sector_size", SECTOR_SIZE)?;
    expect_u64(&value, "/target/required_block_size", REQUIRED_BLOCK_SIZE)?;
    expect_u64(&value, "/payload/extent_count", prelude.extent_count)?;
    expect_u64(&value, "/payload/blob_count", prelude.blob_count)?;
    expect_u64(&value, "/payload/encoded_blob_bytes", prelude.blob_area_len)?;
    expect_u64(
        &value,
        "/payload/planned_extent_bytes",
        prelude.planned_extent_bytes,
    )?;
    expect_u64(
        &value,
        "/payload/planned_zero_bytes",
        prelude.planned_zero_bytes,
    )?;
    expect_u64(
        &value,
        "/payload/omitted_target_bytes",
        prelude.omitted_target_bytes,
    )?;
    expect_u64(&value, "/zstd/window_log", ZSTD_WINDOW_LOG as u64)?;
    expect_u64(&value, "/zstd/level", ZSTD_LEVEL as u64)?;
    expect_str(
        &value,
        "/hashes/semantic_required_sha256",
        &hex(&prelude.semantic_required_sha256),
    )?;
    expect_str(
        &value,
        "/hashes/extent_table_sha256",
        &hex(&prelude.extent_table_sha256),
    )?;
    expect_str(
        &value,
        "/hashes/blob_area_sha256",
        &hex(&prelude.blob_area_sha256),
    )?;
    expect_str(&value, "/zstd/dictionary", "none")?;
    expect_bool(&value, "/zstd/long_distance_matching", false)?;
    expect_bool(&value, "/zstd/content_checksum", false)?;
    if canonical_json(&value)? != bytes {
        return Err(YaoshiError::image("payload manifest is not canonical JSON"));
    }
    Ok(())
}

fn expect_u64(value: &Value, pointer: &str, expected: u64) -> YaoshiResult<()> {
    if value.pointer(pointer).and_then(Value::as_u64) == Some(expected) {
        Ok(())
    } else {
        Err(YaoshiError::image(format!(
            "payload manifest {pointer} mismatch"
        )))
    }
}

fn expect_str(value: &Value, pointer: &str, expected: &str) -> YaoshiResult<()> {
    if value.pointer(pointer).and_then(Value::as_str) == Some(expected) {
        Ok(())
    } else {
        Err(YaoshiError::image(format!(
            "payload manifest {pointer} mismatch"
        )))
    }
}

fn expect_bool(value: &Value, pointer: &str, expected: bool) -> YaoshiResult<()> {
    if value.pointer(pointer).and_then(Value::as_bool) == Some(expected) {
        Ok(())
    } else {
        Err(YaoshiError::image(format!(
            "payload manifest {pointer} mismatch"
        )))
    }
}

fn parse_extent_table(table: &[u8], prelude: &Prelude) -> YaoshiResult<Vec<PayloadExtent>> {
    if table.len() as u64 != prelude.extent_table_len
        || !table.len().is_multiple_of(EXTENT_ENTRY_SIZE)
    {
        return Err(YaoshiError::image("payload extent table length mismatch"));
    }
    let mut entries = Vec::new();
    for chunk in table.chunks_exact(EXTENT_ENTRY_SIZE) {
        if chunk[89] != 0 || chunk[90..128].iter().any(|b| *b != 0) {
            return Err(YaoshiError::image("payload extent reserved bytes mismatch"));
        }
        entries.push(PayloadExtent {
            logical_offset: get_u64(chunk, 0),
            uncompressed_len: get_u32(chunk, 8),
            encoded_len: get_u32(chunk, 12),
            blob_offset: get_u64(chunk, 16),
            uncompressed_sha256: arr32(&chunk[24..56]),
            encoded_sha256: arr32(&chunk[56..88]),
            kind: chunk[88],
        });
    }
    Ok(entries)
}

fn validate_entries(entries: &[PayloadExtent], blob: &[u8], prelude: &Prelude) -> YaoshiResult<()> {
    let mut previous_end = 0u64;
    let mut required = 0u64;
    let mut zero = 0u64;
    let mut blob_ranges = BTreeSet::new();
    let mut previous_blob_end = 0u64;
    let mut semantic = Sha256::new();
    for entry in entries {
        let len = u64::from(entry.uncompressed_len);
        let end = entry
            .logical_offset
            .checked_add(len)
            .ok_or_else(|| YaoshiError::image("payload extent logical range overflows"))?;
        if entry.logical_offset % REQUIRED_BLOCK_SIZE != 0
            || len == 0
            || len % REQUIRED_BLOCK_SIZE != 0
            || len > PAYLOAD_EXTENT_MAX_UNCOMPRESSED_BYTES
            || entry.logical_offset < previous_end
            || end > prelude.target_image_bytes
        {
            return Err(YaoshiError::image("payload extent logical fields invalid"));
        }
        previous_end = end;
        required += len;
        let decoded = match entry.kind {
            KIND_ZERO => {
                if entry.encoded_len != 0 || entry.blob_offset != u64::MAX {
                    return Err(YaoshiError::image("payload zero extent fields invalid"));
                }
                zero += len;
                Vec::new()
            }
            KIND_RAW | KIND_ZSTD => {
                let blob_start = usize::try_from(entry.blob_offset)
                    .map_err(|_| YaoshiError::image("payload blob offset too large"))?;
                let blob_end = blob_start
                    .checked_add(entry.encoded_len as usize)
                    .ok_or_else(|| YaoshiError::image("payload blob range overflows"))?;
                if blob_end > blob.len() {
                    return Err(YaoshiError::image(
                        "payload blob reference outside blob area",
                    ));
                }
                if u64::try_from(blob_start).unwrap() != previous_blob_end
                    || !blob_ranges.insert((blob_start, blob_end))
                {
                    return Err(YaoshiError::image(
                        "payload nonzero blob ranges are not exclusive and ascending",
                    ));
                }
                previous_blob_end = blob_end as u64;
                let encoded = &blob[blob_start..blob_end];
                if sha256_array(encoded) != entry.encoded_sha256 {
                    return Err(YaoshiError::image("payload encoded sha256 mismatch"));
                }
                if entry.kind == KIND_RAW {
                    if entry.encoded_len != entry.uncompressed_len {
                        return Err(YaoshiError::image("payload raw extent length mismatch"));
                    }
                    encoded.to_vec()
                } else {
                    if entry.encoded_len >= entry.uncompressed_len.saturating_sub(32) {
                        return Err(YaoshiError::image("payload zstd extent not smaller enough"));
                    }
                    zstd::bulk::decompress(encoded, entry.uncompressed_len as usize).map_err(
                        |e| YaoshiError::image(format!("decompress payload zstd extent: {e}")),
                    )?
                }
            }
            _ => return Err(YaoshiError::image("payload extent kind unsupported")),
        };
        let uncompressed = if entry.kind == KIND_ZERO {
            vec![0u8; entry.uncompressed_len as usize]
        } else {
            decoded
        };
        if uncompressed.len() != entry.uncompressed_len as usize
            || sha256_array(&uncompressed) != entry.uncompressed_sha256
        {
            return Err(YaoshiError::image("payload uncompressed sha256 mismatch"));
        }
        semantic.update(entry.logical_offset.to_le_bytes());
        semantic.update(entry.uncompressed_len.to_le_bytes());
        semantic.update([entry.kind]);
        semantic.update(&uncompressed);
    }
    if required != prelude.planned_extent_bytes
        || zero != prelude.planned_zero_bytes
        || blob_ranges.len() as u64 != prelude.blob_count
        || sha256_finalize_array(semantic) != prelude.semantic_required_sha256
    {
        return Err(YaoshiError::image("payload extent aggregate mismatch"));
    }
    Ok(())
}

fn validate_entries_metadata(entries: &[PayloadExtent], prelude: &Prelude) -> YaoshiResult<()> {
    let mut previous_end = 0u64;
    let mut required = 0u64;
    let mut zero = 0u64;
    let mut blob_ranges = BTreeSet::new();
    let mut previous_blob_end = 0u64;
    for entry in entries {
        let len = u64::from(entry.uncompressed_len);
        let end = entry
            .logical_offset
            .checked_add(len)
            .ok_or_else(|| YaoshiError::image("payload extent logical range overflows"))?;
        if entry.logical_offset % REQUIRED_BLOCK_SIZE != 0
            || len == 0
            || len % REQUIRED_BLOCK_SIZE != 0
            || len > PAYLOAD_EXTENT_MAX_UNCOMPRESSED_BYTES
            || entry.logical_offset < previous_end
            || end > prelude.target_image_bytes
        {
            return Err(YaoshiError::image("payload extent logical fields invalid"));
        }
        previous_end = end;
        required += len;
        match entry.kind {
            KIND_ZERO => {
                if entry.encoded_len != 0 || entry.blob_offset != u64::MAX {
                    return Err(YaoshiError::image("payload zero extent fields invalid"));
                }
                zero += len;
            }
            KIND_RAW | KIND_ZSTD => {
                let blob_start = usize::try_from(entry.blob_offset)
                    .map_err(|_| YaoshiError::image("payload blob offset too large"))?;
                let blob_end = blob_start
                    .checked_add(entry.encoded_len as usize)
                    .ok_or_else(|| YaoshiError::image("payload blob range overflows"))?;
                if blob_end > prelude.blob_area_len as usize {
                    return Err(YaoshiError::image(
                        "payload blob reference outside blob area",
                    ));
                }
                if u64::try_from(blob_start).unwrap() != previous_blob_end
                    || !blob_ranges.insert((blob_start, blob_end))
                {
                    return Err(YaoshiError::image(
                        "payload nonzero blob ranges are not exclusive and ascending",
                    ));
                }
                previous_blob_end = blob_end as u64;
                if entry.kind == KIND_RAW {
                    if entry.encoded_len != entry.uncompressed_len {
                        return Err(YaoshiError::image("payload raw extent length mismatch"));
                    }
                } else if entry.encoded_len >= entry.uncompressed_len.saturating_sub(32) {
                    return Err(YaoshiError::image("payload zstd extent not smaller enough"));
                }
            }
            _ => return Err(YaoshiError::image("payload extent kind unsupported")),
        }
    }
    if required != prelude.planned_extent_bytes
        || zero != prelude.planned_zero_bytes
        || blob_ranges.len() as u64 != prelude.blob_count
    {
        return Err(YaoshiError::image("payload extent aggregate mismatch"));
    }
    Ok(())
}
