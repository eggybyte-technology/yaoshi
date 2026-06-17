pub fn starts_with_mz(path: &Path) -> YaoshiResult<bool> {
    let mut file = File::open(path).map_err(|e| YaoshiError::image(format!("open PE: {e}")))?;
    let mut magic = [0u8; 2];
    file.read_exact(&mut magic)
        .map_err(|e| YaoshiError::image(format!("read PE magic: {e}")))?;
    Ok(&magic == b"MZ")
}

pub fn ext4_label(path: &Path) -> YaoshiResult<String> {
    let mut file = File::open(path).map_err(|e| YaoshiError::image(format!("open ext4: {e}")))?;
    file.seek(SeekFrom::Start(1024 + 120))
        .map_err(|e| YaoshiError::image(format!("seek ext4 label: {e}")))?;
    let mut raw = [0u8; 16];
    file.read_exact(&mut raw)
        .map_err(|e| YaoshiError::image(format!("read ext4 label: {e}")))?;
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    Ok(String::from_utf8_lossy(&raw[..end]).to_string())
}

pub fn copy_file_exact_at(
    src: &Path,
    dst: &mut File,
    dst_offset: u64,
    expected_len: u64,
) -> YaoshiResult<()> {
    let src_len = fs::metadata(src)
        .map_err(|e| YaoshiError::image(format!("stat source image: {e}")))?
        .len();
    if src_len != expected_len {
        return Err(YaoshiError::image("source image byte length mismatch"));
    }
    let mut src_file =
        File::open(src).map_err(|e| YaoshiError::image(format!("open source image: {e}")))?;
    dst.seek(SeekFrom::Start(dst_offset))
        .map_err(|e| YaoshiError::image(format!("seek destination image: {e}")))?;
    let copied = std::io::copy(&mut Read::by_ref(&mut src_file).take(expected_len), dst)
        .map_err(|e| YaoshiError::image(format!("copy image bytes: {e}")))?;
    if copied != expected_len {
        return Err(YaoshiError::image("copied image byte count mismatch"));
    }
    let mut trailing = [0u8; 1];
    if src_file
        .read(&mut trailing)
        .map_err(|e| YaoshiError::image(format!("read source trailing byte: {e}")))?
        != 0
    {
        return Err(YaoshiError::image("source image has trailing bytes"));
    }
    Ok(())
}

pub fn write_mbr_installer_image(
    candidate: &Path,
    boot_fat32: &Path,
    installed_system: &Path,
    layout: &InstallerMbrLayout,
) -> YaoshiResult<()> {
    let sector = installer_mbr_sector(layout)?;
    let image_bytes = layout.image_bytes();
    if image_bytes > MAX_INSTALLER_IMAGE_BYTES {
        return Err(YaoshiError::image("installer image exceeds maximum size"));
    }
    let payload = layout.payload_partition();
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(candidate)
        .map_err(|e| YaoshiError::image(format!("create installer image: {e}")))?;
    file.set_len(image_bytes)
        .map_err(|e| YaoshiError::image(format!("size installer image: {e}")))?;
    file.write_all(&sector)
        .map_err(|e| YaoshiError::image(format!("write MBR: {e}")))?;
    copy_file_exact_at(
        boot_fat32,
        &mut file,
        layout.boot_partition().start_byte,
        layout.boot_partition().byte_size,
    )?;
    copy_file_exact_at(
        installed_system,
        &mut file,
        payload.start_byte,
        payload.byte_size,
    )?;
    Ok(())
}

pub fn installer_mbr_sector(layout: &InstallerMbrLayout) -> YaoshiResult<[u8; 512]> {
    let payload = layout.payload_partition();
    if payload.sector_count() > u32::MAX as u64 {
        return Err(YaoshiError::image(
            "installer payload partition exceeds MBR sector count field",
        ));
    }
    let mut sector = [0u8; 512];
    write_mbr_entry(&mut sector[446..462], 0xEF, &layout.boot_partition())?;
    write_mbr_entry(&mut sector[462..478], 0x83, &payload)?;
    sector[510] = 0x55;
    sector[511] = 0xaa;
    Ok(sector)
}

fn write_mbr_entry(entry: &mut [u8], mbr_type: u8, part: &PartitionLayout) -> YaoshiResult<()> {
    let start = u32::try_from(part.start_lba())
        .map_err(|_| YaoshiError::image("partition start exceeds MBR field"))?;
    let count = u32::try_from(part.sector_count())
        .map_err(|_| YaoshiError::image("partition size exceeds MBR field"))?;
    entry[0] = 0;
    entry[1..4].fill(0);
    entry[4] = mbr_type;
    entry[5..8].fill(0);
    entry[8..12].copy_from_slice(&start.to_le_bytes());
    entry[12..16].copy_from_slice(&count.to_le_bytes());
    Ok(())
}

pub fn parse_mbr(path: &Path) -> YaoshiResult<Vec<MbrPartition>> {
    let file_len = fs::metadata(path)
        .map_err(|e| YaoshiError::image(format!("stat MBR image: {e}")))?
        .len();
    parse_mbr_with_len(path, file_len)
}

pub fn parse_mbr_with_len(path: &Path, file_len: u64) -> YaoshiResult<Vec<MbrPartition>> {
    let entries = read_mbr_entries_with_len(path, file_len)?;
    Ok(entries[..2].to_vec())
}

pub fn read_mbr_entries(path: &Path) -> YaoshiResult<[MbrEntry; 4]> {
    let file_len = fs::metadata(path)
        .map_err(|e| YaoshiError::image(format!("stat MBR image: {e}")))?
        .len();
    read_mbr_entries_with_len(path, file_len)
}

fn read_mbr_entries_with_len(path: &Path, file_len: u64) -> YaoshiResult<[MbrEntry; 4]> {
    let mut file = File::open(path).map_err(|e| YaoshiError::image(format!("open MBR: {e}")))?;
    let mut sector = [0u8; 512];
    file.read_exact(&mut sector)
        .map_err(|e| YaoshiError::image(format!("read MBR: {e}")))?;
    if sector[510] != 0x55 || sector[511] != 0xaa {
        return Err(YaoshiError::image("invalid MBR signature"));
    }
    let mut entries = [
        MbrEntry {
            number: 1,
            mbr_type: 0,
            start_lba: 0,
            sector_count: 0,
        },
        MbrEntry {
            number: 2,
            mbr_type: 0,
            start_lba: 0,
            sector_count: 0,
        },
        MbrEntry {
            number: 3,
            mbr_type: 0,
            start_lba: 0,
            sector_count: 0,
        },
        MbrEntry {
            number: 4,
            mbr_type: 0,
            start_lba: 0,
            sector_count: 0,
        },
    ];
    for (idx, out) in entries.iter_mut().enumerate() {
        let start = 446 + idx * 16;
        let entry = &sector[start..start + 16];
        let all_zero = entry.iter().all(|b| *b == 0);
        if idx >= 2 {
            if !all_zero {
                return Err(YaoshiError::image("nonzero unused MBR partition entry"));
            }
            continue;
        }
        if all_zero {
            return Err(YaoshiError::image("missing required MBR partition entry"));
        }
        if entry[0] != 0x00 {
            return Err(YaoshiError::image("MBR partition boot indicator mismatch"));
        }
        let start_lba = u32::from_le_bytes(entry[8..12].try_into().unwrap());
        let sector_count = u32::from_le_bytes(entry[12..16].try_into().unwrap());
        let end = (start_lba as u64 + sector_count as u64) * SECTOR_SIZE;
        if end > file_len {
            return Err(YaoshiError::image("MBR partition exceeds image size"));
        }
        *out = MbrEntry {
            number: idx as u8 + 1,
            mbr_type: entry[4],
            start_lba,
            sector_count,
        };
    }
    Ok(entries)
}

pub fn parse_installer_mbr(
    path: &Path,
    expected_layout: &InstallerMbrLayout,
) -> YaoshiResult<InstallerMbr> {
    let file_len = fs::metadata(path)
        .map_err(|e| YaoshiError::image(format!("stat MBR image: {e}")))?
        .len();
    let entries = read_mbr_entries_with_len(path, file_len)?;
    let boot = expected_layout.boot_partition();
    let payload = expected_layout.payload_partition();
    let entry1 = &entries[0];
    let entry2 = &entries[1];
    if entry1.mbr_type != 0xEF
        || entry1.start_lba as u64 != boot.start_lba()
        || entry1.sector_count as u64 != boot.sector_count()
    {
        return Err(YaoshiError::image("installer boot partition mismatch"));
    }
    if entry2.mbr_type != 0x83
        || entry2.start_lba as u64 != payload.start_lba()
        || entry2.sector_count as u64 != payload.sector_count()
    {
        return Err(YaoshiError::image("installer payload partition mismatch"));
    }
    if payload.sector_count() > u32::MAX as u64 {
        return Err(YaoshiError::image(
            "installer partition 2 sector count overflow",
        ));
    }
    Ok(InstallerMbr {
        boot_partition: entry1.clone(),
        payload_partition: entry2.clone(),
    })
}

