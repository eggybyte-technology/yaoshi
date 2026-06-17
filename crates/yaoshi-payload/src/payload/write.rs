pub fn write_payload_to_target(
    payload_path: &Path,
    target: &mut File,
    progress: &mut dyn FnMut(PayloadWriteProgress),
) -> YaoshiResult<()> {
    let mut payload = File::open(payload_path)
        .map_err(|e| YaoshiError::image(format!("open payload for install: {e}")))?;
    write_payload_reader_to_target(&mut payload, target, progress)
}

pub fn write_payload_reader_to_target(
    payload: &mut File,
    target: &mut File,
    progress: &mut dyn FnMut(PayloadWriteProgress),
) -> YaoshiResult<()> {
    let len = payload
        .metadata()
        .map_err(|e| YaoshiError::image(format!("stat payload reader: {e}")))?
        .len();
    write_payload_region_reader_to_target(payload, 0, len, target, progress)
}

pub fn write_payload_region_to_target(
    payload_path: &Path,
    payload_offset: u64,
    payload_len: u64,
    target: &mut File,
    progress: &mut dyn FnMut(PayloadWriteProgress),
) -> YaoshiResult<()> {
    let mut payload = File::open(payload_path)
        .map_err(|e| YaoshiError::image(format!("open payload media for install: {e}")))?;
    write_payload_region_reader_to_target(
        &mut payload,
        payload_offset,
        payload_len,
        target,
        progress,
    )
}

fn write_payload_region_reader_to_target(
    payload: &mut File,
    base_offset: u64,
    len: u64,
    target: &mut File,
    progress: &mut dyn FnMut(PayloadWriteProgress),
) -> YaoshiResult<()> {
    let mut prelude_raw = [0u8; PRELUDE_LEN];
    payload
        .read_exact_at(&mut prelude_raw, base_offset)
        .map_err(|e| YaoshiError::image(format!("read payload prelude: {e}")))?;
    let prelude = parse_prelude(&prelude_raw, len)?;
    let manifest = read_region_at(
        payload,
        base_offset + prelude.manifest_offset,
        prelude.manifest_len,
    )?;
    if sha256_array(&manifest) != prelude.manifest_sha256 {
        return Err(YaoshiError::image("payload manifest sha256 mismatch"));
    }
    validate_manifest(&manifest, &prelude)?;
    let table = read_region_at(
        payload,
        base_offset + prelude.extent_table_offset,
        prelude.extent_table_len,
    )?;
    if sha256_array(&table) != prelude.extent_table_sha256 {
        return Err(YaoshiError::image("payload extent table sha256 mismatch"));
    }
    let entries = parse_extent_table(&table, &prelude)?;
    validate_entries_metadata(&entries, &prelude)?;
    let mut written = 0u64;
    let mut zero_written = 0u64;
    let mut last_progress_written = 0u64;
    for entry in entries {
        match entry.kind {
            KIND_ZERO => {
                zero_written += u64::from(entry.uncompressed_len);
                zero_target_range(
                    target,
                    entry.logical_offset,
                    u64::from(entry.uncompressed_len),
                )?;
            }
            KIND_RAW | KIND_ZSTD => {
                let encoded_offset = prelude
                    .blob_area_offset
                    .checked_add(entry.blob_offset)
                    .ok_or_else(|| YaoshiError::image("payload encoded offset overflows"))?;
                let encoded = read_region_at(
                    payload,
                    base_offset + encoded_offset,
                    u64::from(entry.encoded_len),
                )?;
                let bytes = if entry.kind == KIND_RAW {
                    encoded
                } else {
                    zstd::bulk::decompress(&encoded, entry.uncompressed_len as usize).map_err(
                        |e| YaoshiError::image(format!("decompress payload extent: {e}")),
                    )?
                };
                if bytes.len() != entry.uncompressed_len as usize {
                    return Err(YaoshiError::image("payload decoded length mismatch"));
                }
                target
                    .write_all_at(&bytes, entry.logical_offset)
                    .map_err(|e| YaoshiError::image(format!("write payload extent to target: {e}")))?;
            }
            _ => return Err(YaoshiError::image("payload extent kind unsupported")),
        }
        written += u64::from(entry.uncompressed_len);
        if written == prelude.planned_extent_bytes
            || written.saturating_sub(last_progress_written)
                >= PAYLOAD_EXTENT_MAX_UNCOMPRESSED_BYTES
        {
            last_progress_written = written;
            progress(PayloadWriteProgress {
                planned_written_bytes: written,
                planned_total_bytes: prelude.planned_extent_bytes,
                zero_written_bytes: zero_written,
                source_read_bytes: prelude.blob_area_offset + prelude.blob_area_len,
                payload_source_bytes: prelude.total_payload_bytes,
            });
        }
    }
    if written != prelude.planned_extent_bytes {
        return Err(YaoshiError::image("payload written byte count mismatch"));
    }
    if last_progress_written != written {
        progress(PayloadWriteProgress {
            planned_written_bytes: written,
            planned_total_bytes: prelude.planned_extent_bytes,
            zero_written_bytes: zero_written,
            source_read_bytes: prelude.blob_area_offset + prelude.blob_area_len,
            payload_source_bytes: prelude.total_payload_bytes,
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct PayloadWriteProgress {
    pub planned_written_bytes: u64,
    pub planned_total_bytes: u64,
    pub zero_written_bytes: u64,
    pub source_read_bytes: u64,
    pub payload_source_bytes: u64,
}

fn write_extent_entry(out: &mut Vec<u8>, entry: &PayloadExtent) {
    out.extend_from_slice(&entry.logical_offset.to_le_bytes());
    out.extend_from_slice(&entry.uncompressed_len.to_le_bytes());
    out.extend_from_slice(&entry.encoded_len.to_le_bytes());
    out.extend_from_slice(&entry.blob_offset.to_le_bytes());
    out.extend_from_slice(&entry.uncompressed_sha256);
    out.extend_from_slice(&entry.encoded_sha256);
    out.push(entry.kind);
    out.push(0);
    out.extend_from_slice(&[0u8; 38]);
}

fn read_region_at(file: &File, offset: u64, len: u64) -> YaoshiResult<Vec<u8>> {
    let usize_len =
        usize::try_from(len).map_err(|_| YaoshiError::image("payload region too large"))?;
    let mut bytes = vec![0u8; usize_len];
    file.read_exact_at(&mut bytes, offset)
        .map_err(|e| YaoshiError::image(format!("read payload region: {e}")))?;
    Ok(bytes)
}

fn read_region(file: &mut File, offset: u64, len: u64) -> YaoshiResult<Vec<u8>> {
    read_region_at(file, offset, len)
}

fn zero_target_range(file: &File, offset: u64, len: u64) -> YaoshiResult<()> {
    if len == 0 {
        return Ok(());
    }
    if !offset.is_multiple_of(SECTOR_SIZE) || !len.is_multiple_of(SECTOR_SIZE) {
        return Err(YaoshiError::image("payload zero range is not sector aligned"));
    }
    const BLKZEROOUT: libc::Ioctl = 0x127f;
    let mut range = [offset, len];
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), BLKZEROOUT, range.as_mut_ptr()) };
    if rc == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(code) if code == libc::ENOTTY || code == libc::EOPNOTSUPP || code == libc::EINVAL => {}
        _ => return Err(YaoshiError::image(format!("zero payload range: {err}"))),
    }
    let zeros = vec![0u8; 1024 * 1024];
    let mut remaining = len;
    let mut cursor = offset;
    while remaining > 0 {
        let n = remaining.min(zeros.len() as u64) as usize;
        file.write_all_at(&zeros[..n], cursor)
            .map_err(|e| YaoshiError::image(format!("write payload zero fallback: {e}")))?;
        cursor += n as u64;
        remaining -= n as u64;
    }
    Ok(())
}

fn write_padding(file: &mut File, len: u64) -> YaoshiResult<()> {
    if len == 0 {
        return Ok(());
    }
    let zeros = vec![0u8; len as usize];
    file.write_all(&zeros)
        .map_err(|e| YaoshiError::image(format!("write payload padding: {e}")))
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_i32(bytes: &mut [u8], offset: usize, value: i32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn get_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

pub fn sha256_file_hex(path: &Path) -> YaoshiResult<String> {
    let mut file =
        File::open(path).map_err(|e| YaoshiError::image(format!("open file for sha256: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| YaoshiError::image(format!("read file for sha256: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&sha256_finalize_array(hasher)))
}

fn sha256_array(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    sha256_finalize_array(hasher)
}

fn sha256_finalize_array(hasher: Sha256) -> [u8; 32] {
    arr32(hasher.finalize().as_slice())
}

fn arr32(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(bytes);
    out
}

fn hex(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
