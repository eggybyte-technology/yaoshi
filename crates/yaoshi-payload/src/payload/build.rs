#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadPlanExtent {
    pub logical_offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadPlan {
    pub target_image_bytes: u64,
    pub extents: Vec<PayloadPlanExtent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadExtent {
    pub logical_offset: u64,
    pub uncompressed_len: u32,
    pub encoded_len: u32,
    pub blob_offset: u64,
    pub uncompressed_sha256: [u8; 32],
    pub encoded_sha256: [u8; 32],
    pub kind: u8,
}

#[derive(Debug, Clone)]
pub struct PayloadInfo {
    pub total_payload_bytes: u64,
    pub target_image_bytes: u64,
    pub target_minimum_bytes: u64,
    pub planned_extent_bytes: u64,
    pub planned_zero_bytes: u64,
    pub omitted_target_bytes: u64,
    pub extent_count: u64,
    pub blob_area_offset: u64,
    pub blob_area_len: u64,
    pub semantic_required_sha256: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadExtentSummary {
    pub logical_offset: u64,
    pub uncompressed_len: u32,
    pub kind: u8,
}

#[derive(Debug, Clone)]
pub struct PayloadBuildMeta {
    pub esp_start: u64,
    pub esp_size: u64,
    pub root_start: u64,
    pub root_size: u64,
}

#[derive(Debug, Clone)]
pub struct PayloadEncodingCache {
    pub root: PathBuf,
    pub installed_esp_fat32_sha256: String,
    pub installed_root_ext4_identity: String,
    pub gpt_source_sha256: String,
}

pub fn build_payload_from_virtual_target_graph(
    graph: &yaoshi_image::VirtualInstalledTargetGraph,
    out: &Path,
    meta: &PayloadBuildMeta,
) -> YaoshiResult<PayloadInfo> {
    build_payload_from_virtual_target_graph_with_cache(graph, out, meta, None)
}

pub fn build_payload_from_virtual_target_graph_with_cache(
    graph: &yaoshi_image::VirtualInstalledTargetGraph,
    out: &Path,
    meta: &PayloadBuildMeta,
    encoding_cache: Option<&PayloadEncodingCache>,
) -> YaoshiResult<PayloadInfo> {
    let mut planned_extents = Vec::new();
    graph.for_each_extent(|planned| {
        planned_extents.push(planned);
        Ok(())
    })?;
    let encoded_extents = encode_planned_extents(&planned_extents, meta, encoding_cache)?;
    let mut table = Vec::new();
    let mut blob_area = Vec::new();
    let mut entries = Vec::new();
    let mut blob_count = 0u64;
    let mut planned_extent_bytes = 0u64;
    let mut planned_zero_bytes = 0u64;
    let mut semantic_hasher = Sha256::new();

    for encoded_extent in encoded_extents {
        let planned = &encoded_extent.planned;
        let len = u32::try_from(planned.bytes.len())
            .map_err(|_| YaoshiError::image("virtual target extent length exceeds u32"))?;
        if planned.kind == yaoshi_image::TargetWriteExtentKind::Zero {
            planned_zero_bytes += u64::from(len);
        }
        planned_extent_bytes += u64::from(len);
        let uncompressed_sha256 = encoded_extent.uncompressed_sha256;
        let kind = encoded_extent.kind;
        let encoded = encoded_extent.encoded;
        if planned.kind == yaoshi_image::TargetWriteExtentKind::Data && kind == KIND_ZERO {
            return Err(YaoshiError::image(
                "virtual target data extent encoded as zero",
            ));
        }
        let encoded_sha256 = sha256_array(&encoded);
        let blob_offset = if kind == KIND_ZERO {
            u64::MAX
        } else {
            let offset = blob_area.len() as u64;
            blob_area.extend_from_slice(&encoded);
            blob_count += 1;
            offset
        };
        semantic_hasher.update(planned.logical_offset.to_le_bytes());
        semantic_hasher.update(len.to_le_bytes());
        semantic_hasher.update([kind]);
        semantic_hasher.update(&planned.bytes);
        let entry = PayloadExtent {
            logical_offset: planned.logical_offset,
            uncompressed_len: len,
            encoded_len: encoded.len() as u32,
            blob_offset,
            uncompressed_sha256,
            encoded_sha256,
            kind,
        };
        write_extent_entry(&mut table, &entry);
        entries.push(entry);
    }

    let semantic_required_sha256 = sha256_finalize_array(semantic_hasher);
    let target_len = graph.target_image_bytes();
    let info = write_payload_parts(
        out,
        PayloadParts {
            map: PayloadPlan {
                target_image_bytes: target_len,
                extents: entries
                    .iter()
                    .map(|extent| PayloadPlanExtent {
                        logical_offset: extent.logical_offset,
                        len: u64::from(extent.uncompressed_len),
                    })
                    .collect(),
            },
            meta,
            target_len,
            table,
            blob_area,
            extent_count: entries.len() as u64,
            blob_count,
            planned_extent_bytes,
            planned_zero_bytes,
            semantic_required_sha256: &semantic_required_sha256,
        },
    )?;
    if info.planned_extent_bytes != target_len || info.omitted_target_bytes != 0 {
        return Err(YaoshiError::image(
            "payload virtual target graph does not cover complete target image",
        ));
    }
    Ok(info)
}

struct PayloadParts<'a> {
    map: PayloadPlan,
    meta: &'a PayloadBuildMeta,
    target_len: u64,
    table: Vec<u8>,
    blob_area: Vec<u8>,
    extent_count: u64,
    blob_count: u64,
    planned_extent_bytes: u64,
    planned_zero_bytes: u64,
    semantic_required_sha256: &'a [u8; 32],
}

fn write_payload_parts(out: &Path, parts: PayloadParts<'_>) -> YaoshiResult<PayloadInfo> {
    let PayloadParts {
        map,
        meta,
        target_len,
        table,
        blob_area,
        extent_count,
        blob_count,
        planned_extent_bytes,
        planned_zero_bytes,
        semantic_required_sha256,
    } = parts;
    let blob_area_sha256 = sha256_array(&blob_area);
    let extent_table_sha256 = sha256_array(&table);
    let manifest_value = manifest_json(ManifestParts {
        map: &map,
        meta,
        extent_count,
        blob_count,
        planned_extent_bytes,
        planned_zero_bytes,
        encoded_blob_bytes: blob_area.len() as u64,
        omitted_target_bytes: target_len - planned_extent_bytes,
        semantic_hash: semantic_required_sha256,
        extent_hash: &extent_table_sha256,
        blob_hash: &blob_area_sha256,
    });
    let manifest = canonical_json(&manifest_value)?;
    let manifest_sha256 = sha256_array(&manifest);
    let manifest_padded_len = round_up(manifest.len() as u64, PAYLOAD_CONTAINER_ALIGNMENT_BYTES);
    let extent_table_offset = PRELUDE_LEN as u64 + manifest_padded_len;
    let blob_area_offset = extent_table_offset + table.len() as u64;
    let unpadded_total = blob_area_offset + blob_area.len() as u64;
    let total_payload_bytes = round_up(unpadded_total, PAYLOAD_CONTAINER_ALIGNMENT_BYTES);

    let mut prelude = [0u8; PRELUDE_LEN];
    prelude[0..16].copy_from_slice(PAYLOAD_MAGIC);
    put_u32(&mut prelude, 16, FORMAT_VERSION);
    put_u32(&mut prelude, 20, PRELUDE_LEN as u32);
    put_u64(&mut prelude, 24, PRELUDE_LEN as u64);
    put_u64(&mut prelude, 32, manifest.len() as u64);
    put_u64(&mut prelude, 40, manifest_padded_len);
    put_u64(&mut prelude, 48, extent_table_offset);
    put_u64(&mut prelude, 56, table.len() as u64);
    put_u64(&mut prelude, 64, blob_area_offset);
    put_u64(&mut prelude, 72, blob_area.len() as u64);
    put_u64(&mut prelude, 80, total_payload_bytes);
    put_u64(&mut prelude, 88, target_len);
    put_u64(&mut prelude, 96, target_len);
    put_u32(&mut prelude, 104, SECTOR_SIZE as u32);
    put_u32(&mut prelude, 108, REQUIRED_BLOCK_SIZE as u32);
    put_u32(&mut prelude, 112, EXTENT_ENTRY_SIZE as u32);
    put_u32(&mut prelude, 116, COMPRESSION_ZSTD);
    put_i32(&mut prelude, 120, ZSTD_LEVEL);
    put_u32(&mut prelude, 124, ZSTD_WINDOW_LOG);
    prelude[128..160].copy_from_slice(&manifest_sha256);
    prelude[160..192].copy_from_slice(&extent_table_sha256);
    prelude[192..224].copy_from_slice(&blob_area_sha256);
    prelude[224..256].copy_from_slice(semantic_required_sha256);
    put_u64(&mut prelude, 256, extent_count);
    put_u64(&mut prelude, 264, blob_count);
    put_u64(&mut prelude, 272, planned_extent_bytes);
    put_u64(&mut prelude, 280, planned_zero_bytes);
    put_u64(&mut prelude, 288, blob_area.len() as u64);
    put_u64(&mut prelude, 296, target_len - planned_extent_bytes);

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::image(format!("create payload parent: {e}")))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(out)
        .map_err(|e| YaoshiError::image(format!("create payload: {e}")))?;
    file.write_all(&prelude)
        .and_then(|_| file.write_all(&manifest))
        .map_err(|e| YaoshiError::image(format!("write payload header: {e}")))?;
    write_padding(&mut file, manifest_padded_len - manifest.len() as u64)?;
    file.write_all(&table)
        .and_then(|_| file.write_all(&blob_area))
        .map_err(|e| YaoshiError::image(format!("write payload body: {e}")))?;
    write_padding(&mut file, total_payload_bytes - unpadded_total)?;
    file.sync_all()
        .map_err(|e| YaoshiError::image(format!("fsync payload: {e}")))?;

    let info = validate_payload(out)?;
    if info.semantic_required_sha256 != *semantic_required_sha256 {
        return Err(YaoshiError::image(
            "payload semantic hash mismatch after write",
        ));
    }
    Ok(info)
}

struct EncodedPayloadExtent {
    planned: yaoshi_image::VirtualTargetExtent,
    uncompressed_sha256: [u8; 32],
    kind: u8,
    encoded: Vec<u8>,
}

fn encode_planned_extents(
    planned_extents: &[yaoshi_image::VirtualTargetExtent],
    meta: &PayloadBuildMeta,
    encoding_cache: Option<&PayloadEncodingCache>,
) -> YaoshiResult<Vec<EncodedPayloadExtent>> {
    if planned_extents.is_empty() {
        return Ok(Vec::new());
    }
    let worker_count = payload_worker_count(planned_extents.len());
    if worker_count <= 1 {
        return planned_extents
            .iter()
            .cloned()
            .map(|planned| encode_planned_extent(planned, meta, encoding_cache))
            .collect();
    }
    let chunk_size = planned_extents.len().div_ceil(worker_count);
    let meta = meta.clone();
    let cache = encoding_cache.cloned();
    let mut encoded = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for (chunk_index, chunk) in planned_extents.chunks(chunk_size).enumerate() {
            let meta = meta.clone();
            let cache = cache.clone();
            handles.push(scope.spawn(move || -> YaoshiResult<Vec<(usize, EncodedPayloadExtent)>> {
                chunk
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(local_index, planned)| {
                        let encoded = encode_planned_extent(planned, &meta, cache.as_ref())?;
                        Ok((chunk_index * chunk_size + local_index, encoded))
                    })
                    .collect()
            }));
        }
        let mut encoded = Vec::with_capacity(planned_extents.len());
        for handle in handles {
            let chunk = handle
                .join()
                .map_err(|_| YaoshiError::internal("payload extent worker panicked"))??;
            encoded.extend(chunk);
        }
        Ok::<_, YaoshiError>(encoded)
    })?;
    encoded.sort_by_key(|(index, _)| *index);
    Ok(encoded.into_iter().map(|(_, extent)| extent).collect())
}

fn payload_worker_count(extent_count: usize) -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(extent_count)
        .max(1)
}

fn encode_planned_extent(
    planned: yaoshi_image::VirtualTargetExtent,
    meta: &PayloadBuildMeta,
    encoding_cache: Option<&PayloadEncodingCache>,
) -> YaoshiResult<EncodedPayloadExtent> {
    let uncompressed_sha256 = sha256_array(&planned.bytes);
    let (kind, encoded) = encode_extent_cached(&planned, &uncompressed_sha256, meta, encoding_cache)?;
    Ok(EncodedPayloadExtent {
        planned,
        uncompressed_sha256,
        kind,
        encoded,
    })
}

fn encode_extent(bytes: &[u8]) -> YaoshiResult<(u8, Vec<u8>)> {
    if bytes.iter().all(|b| *b == 0) {
        return Ok((KIND_ZERO, Vec::new()));
    }
    let compressed = zstd::bulk::compress(bytes, ZSTD_LEVEL)
        .map_err(|e| YaoshiError::image(format!("compress payload extent: {e}")))?;
    if compressed.len() + 32 < bytes.len() {
        Ok((KIND_ZSTD, compressed))
    } else {
        Ok((KIND_RAW, bytes.to_vec()))
    }
}

fn encode_extent_cached(
    planned: &yaoshi_image::VirtualTargetExtent,
    uncompressed_sha256: &[u8; 32],
    meta: &PayloadBuildMeta,
    cache: Option<&PayloadEncodingCache>,
) -> YaoshiResult<(u8, Vec<u8>)> {
    if planned.kind == yaoshi_image::TargetWriteExtentKind::Zero {
        return Ok((KIND_ZERO, Vec::new()));
    }
    let Some(cache) = cache else {
        return encode_extent(&planned.bytes);
    };
    let Some(source) = extent_source(
        cache,
        meta,
        planned.logical_offset,
        planned.bytes.len() as u64,
    ) else {
        return encode_extent(&planned.bytes);
    };
    let key = extent_cache_key(
        source,
        planned.logical_offset,
        planned.bytes.len() as u64,
        uncompressed_sha256,
        planned.kind,
    )?;
    if let Some(hit) = read_extent_cache(cache, &key, &planned.bytes)? {
        return Ok(hit);
    }
    let encoded = encode_extent(&planned.bytes)?;
    if encoded.0 != KIND_ZERO {
        write_extent_cache(cache, &key, encoded.0, &encoded.1)?;
    }
    Ok(encoded)
}

fn extent_source(
    _cache: &PayloadEncodingCache,
    meta: &PayloadBuildMeta,
    offset: u64,
    len: u64,
) -> Option<&'static str> {
    let end = offset.checked_add(len)?;
    let esp_end = meta.esp_start.checked_add(meta.esp_size)?;
    if offset >= meta.esp_start && end <= esp_end {
        return Some("installed-esp-fat32");
    }
    let root_end = meta.root_start.checked_add(meta.root_size)?;
    if offset >= meta.root_start && end <= root_end {
        return Some("installed-root-ext4");
    }
    if ranges_overlap(offset, end, meta.esp_start, esp_end)
        || ranges_overlap(offset, end, meta.root_start, root_end)
    {
        return None;
    }
    Some("gpt")
}

fn ranges_overlap(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start < b_end && b_start < a_end
}

fn extent_cache_key(
    source: &str,
    logical_offset: u64,
    uncompressed_len: u64,
    uncompressed_sha256: &[u8; 32],
    kind: yaoshi_image::TargetWriteExtentKind,
) -> YaoshiResult<String> {
    let value = json!({
        "grammar": "yaoshi.payload-extent-encoding-cache.v1",
        "source": source,
        "logical_offset": logical_offset,
        "uncompressed_len": uncompressed_len,
        "uncompressed_sha256": hex(uncompressed_sha256),
        "extent_kind_before_encoding": kind.as_str(),
        "zstd_level": ZSTD_LEVEL,
        "zstd_window_log": ZSTD_WINDOW_LOG,
        "zstd_dictionary": "none",
        "zstd_long_distance_matching": false,
        "zstd_content_checksum": false,
        "raw_encoding_threshold_rule": "encoded_len_plus_32_lt_uncompressed_len",
    });
    Ok(hex(&sha256_array(&canonical_json(&value)?)))
}

fn read_extent_cache(
    cache: &PayloadEncodingCache,
    key: &str,
    uncompressed: &[u8],
) -> YaoshiResult<Option<(u8, Vec<u8>)>> {
    let path = cache.root.join(key);
    let Ok(bytes) = fs::read(&path) else {
        return Ok(None);
    };
    let Some(split) = bytes.windows(2).position(|window| window == b"\n\n") else {
        return Ok(None);
    };
    let header = match std::str::from_utf8(&bytes[..split]) {
        Ok(header) if header.is_ascii() => header,
        _ => return Ok(None),
    };
    let blob = bytes[split + 2..].to_vec();
    let mut kind = None;
    let mut encoded_sha256 = None;
    let mut encoded_len = None;
    let mut lines = header.lines();
    if lines.next() != Some("yaoshi-payload-extent-cache-v1") {
        return Ok(None);
    }
    for line in lines {
        if let Some(value) = line.strip_prefix("kind=") {
            kind = match value {
                "raw" => Some(KIND_RAW),
                "zstd" => Some(KIND_ZSTD),
                _ => return Ok(None),
            };
        } else if let Some(value) = line.strip_prefix("encoded-sha256=") {
            if value.len() != 64
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Ok(None);
            }
            encoded_sha256 = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("encoded-len=") {
            encoded_len = value.parse::<usize>().ok();
        } else {
            return Ok(None);
        }
    }
    let Some(kind) = kind else {
        return Ok(None);
    };
    if encoded_len != Some(blob.len()) {
        return Ok(None);
    }
    if encoded_sha256.as_deref() != Some(&hex(&sha256_array(&blob))) {
        return Ok(None);
    }
    match kind {
        KIND_RAW if blob == uncompressed => Ok(Some((kind, blob))),
        KIND_ZSTD if blob.len() + 32 < uncompressed.len() => {
            let decoded = zstd::bulk::decompress(&blob, uncompressed.len())
                .map_err(|e| YaoshiError::image(format!("decode cached payload extent: {e}")))?;
            if decoded == uncompressed {
                Ok(Some((kind, blob)))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

fn write_extent_cache(
    cache: &PayloadEncodingCache,
    key: &str,
    kind: u8,
    encoded: &[u8],
) -> YaoshiResult<()> {
    fs::create_dir_all(&cache.root)
        .map_err(|e| YaoshiError::image(format!("create payload extent cache: {e}")))?;
    let path = cache.root.join(key);
    let tmp = cache.root.join(format!(".{key}.tmp"));
    let kind_text = match kind {
        KIND_RAW => "raw",
        KIND_ZSTD => "zstd",
        _ => return Ok(()),
    };
    let mut bytes = format!(
        "yaoshi-payload-extent-cache-v1\nkind={kind_text}\nencoded-sha256={}\nencoded-len={}\n\n",
        hex(&sha256_array(encoded)),
        encoded.len()
    )
    .into_bytes();
    bytes.extend_from_slice(encoded);
    fs::write(&tmp, bytes)
        .map_err(|e| YaoshiError::image(format!("write payload extent cache temp: {e}")))?;
    File::open(&tmp)
        .and_then(|file| file.sync_all())
        .map_err(|e| YaoshiError::image(format!("fsync payload extent cache temp: {e}")))?;
    fs::rename(&tmp, &path)
        .map_err(|e| YaoshiError::image(format!("install payload extent cache entry: {e}")))?;
    Ok(())
}

struct ManifestParts<'a> {
    map: &'a PayloadPlan,
    meta: &'a PayloadBuildMeta,
    extent_count: u64,
    blob_count: u64,
    planned_extent_bytes: u64,
    planned_zero_bytes: u64,
    encoded_blob_bytes: u64,
    omitted_target_bytes: u64,
    semantic_hash: &'a [u8; 32],
    extent_hash: &'a [u8; 32],
    blob_hash: &'a [u8; 32],
}

fn manifest_json(parts: ManifestParts<'_>) -> Value {
    let ManifestParts {
        map,
        meta,
        extent_count,
        blob_count,
        planned_extent_bytes,
        planned_zero_bytes,
        encoded_blob_bytes,
        omitted_target_bytes,
        semantic_hash,
        extent_hash,
        blob_hash,
    } = parts;
    json!({
        "format": "yaoshi.payload.v1",
        "hashes": {
            "blob_area_sha256": hex(blob_hash),
            "extent_table_sha256": hex(extent_hash),
            "semantic_required_sha256": hex(semantic_hash)
        },
        "partitions": [
            {
                "byte_size": meta.esp_size,
                "filesystem": "FAT32",
                "label": INSTALLED_ESP_LABEL,
                "name": INSTALLED_ESP_NAME,
                "number": 1,
                "start_byte_offset": meta.esp_start
            },
            {
                "byte_size": meta.root_size,
                "filesystem": "ext4",
                "label": INSTALLED_ROOT_LABEL,
                "name": INSTALLED_ROOT_NAME,
                "number": 2,
                "start_byte_offset": meta.root_start
            }
        ],
        "payload": {
            "blob_count": blob_count,
            "encoded_blob_bytes": encoded_blob_bytes,
            "extent_count": extent_count,
            "omitted_target_bytes": omitted_target_bytes,
            "planned_extent_bytes": planned_extent_bytes,
            "planned_zero_bytes": planned_zero_bytes
        },
        "target": {
            "image_bytes": map.target_image_bytes,
            "logical_sector_size": SECTOR_SIZE,
            "minimum_bytes": map.target_image_bytes,
            "required_block_size": REQUIRED_BLOCK_SIZE
        },
        "version": VERSION,
        "zstd": {
            "content_checksum": false,
            "dictionary": "none",
            "level": ZSTD_LEVEL,
            "long_distance_matching": false,
            "window_log": ZSTD_WINDOW_LOG
        }
    })
}

fn canonical_json(value: &Value) -> YaoshiResult<Vec<u8>> {
    fn sort(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut sorted = serde_json::Map::new();
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort();
                for key in keys {
                    sorted.insert(key.clone(), sort(&map[key]));
                }
                Value::Object(sorted)
            }
            Value::Array(values) => Value::Array(values.iter().map(sort).collect()),
            _ => value.clone(),
        }
    }
    serde_json::to_vec(&sort(value))
        .map_err(|e| YaoshiError::internal(format!("serialize canonical payload manifest: {e}")))
}
