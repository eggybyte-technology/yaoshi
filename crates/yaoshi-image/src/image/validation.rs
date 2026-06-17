pub fn check_installed_system_image(image: &Path, layout: &InstalledGptLayout) -> YaoshiResult<()> {
    if !image.is_file() {
        return Err(YaoshiError::image(
            "installed-system.img is not a regular file",
        ));
    }
    let len = fs::metadata(image)
        .map_err(|e| YaoshiError::image(format!("stat installed-system.img: {e}")))?
        .len();
    if len != layout.image_bytes() {
        return Err(YaoshiError::image("installed-system.img size mismatch"));
    }
    check_installed_system_shape_at(image, 0, layout, true)
}

pub fn check_installer_image(
    candidate: &Path,
    installed_system: &Path,
    layout: &InstallerMbrLayout,
) -> YaoshiResult<()> {
    check_installer_image_inner(candidate, installed_system, layout, None)
}

pub fn check_installer_image_with_installed_layout(
    candidate: &Path,
    installed_system: &Path,
    layout: &InstallerMbrLayout,
    installed_layout: &InstalledGptLayout,
) -> YaoshiResult<()> {
    check_installer_image_inner(candidate, installed_system, layout, Some(installed_layout))
}

fn check_installer_image_inner(
    candidate: &Path,
    installed_system: &Path,
    layout: &InstallerMbrLayout,
    _installed_layout: Option<&InstalledGptLayout>,
) -> YaoshiResult<()> {
    if !candidate.is_file() {
        return Err(YaoshiError::image(
            "candidate installer image is not a regular file",
        ));
    }
    let len = fs::metadata(candidate)
        .map_err(|e| YaoshiError::image(format!("stat installer image: {e}")))?
        .len();
    if len != layout.image_bytes() {
        return Err(YaoshiError::image("installer image size mismatch"));
    }
    reject_iso9660_structures(candidate)?;
    let mbr = parse_installer_mbr(candidate, layout)?;
    let boot = layout.boot_partition();
    let payload = layout.payload_partition();
    let _ = mbr;
    if len > MAX_INSTALLER_IMAGE_BYTES {
        return Err(YaoshiError::image("installer image exceeds maximum size"));
    }
    compare_partition_bytes(
        candidate,
        payload.start_byte,
        payload.byte_size,
        installed_system,
    )?;
    verify_zero_range(
        candidate,
        layout.image_bytes() - TRAILING_GAP_BYTES,
        TRAILING_GAP_BYTES,
    )?;
    let boot_files = validate_fat32_partition_files(
        candidate,
        &boot,
        INSTALLER_BOOT_LABEL,
        &[
            EFI_BOOT_PATH,
            "loader/loader.conf",
            "loader/entries/yaoshi-installer.conf",
            INSTALLER_KERNEL_PATH,
            INSTALLER_KERNEL_RELEASE_PATH,
            INSTALLER_BASE_INITRAMFS_FAT_PATH,
            INSTALLER_APP_INITRAMFS_FAT_PATH,
        ],
    )?;
    let boot_efi = &boot_files[0];
    let loader_conf = &boot_files[1];
    let loader_entry = &boot_files[2];
    let installer_kernel = &boot_files[3];
    let installer_release = &boot_files[4];
    let installer_base_cpio = &boot_files[5];
    let installer_app_cpio = &boot_files[6];
    if !boot_efi.starts_with(b"MZ") {
        return Err(YaoshiError::image("installer BOOTX64.EFI is not PE/COFF"));
    }
    if loader_conf != b"default yaoshi-installer.conf\ntimeout 0\neditor no\n"
        || !loader_entry.starts_with(b"title Yaoshi Installer\n")
        || loader_entry.as_slice()
            != b"title Yaoshi Installer\nlinux /YAOSHI/BOOT/VMLINUZ\ninitrd /YAOSHI/INSTALLER-BASE.CPIO.ZST\ninitrd /YAOSHI/INSTALLER-APP.CPIO.ZST\noptions quiet loglevel=3 yaoshi.mode=installer psi=1 console=ttyS0,115200n8 consoleblank=0 rdinit=/init\n"
        || !installer_kernel.starts_with(b"MZ")
        || !installer_release.ends_with(b"\n")
    {
        return Err(YaoshiError::image("installer boot tree mismatch"));
    }
    if !installer_base_cpio.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
        || !installer_app_cpio.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
    {
        return Err(YaoshiError::image("installer initramfs layer is not zstd"));
    }
    let payload_magic = read_at(candidate, payload.start_byte, 16)?;
    if payload_magic.as_slice() != PAYLOAD_MAGIC {
        return Err(YaoshiError::image(
            "installer partition 2 is not YAOSHI_PAYLOAD_V1",
        ));
    }
    Ok(())
}

fn read_at(path: &Path, offset: u64, len: usize) -> YaoshiResult<Vec<u8>> {
    let mut file = File::open(path).map_err(|e| YaoshiError::image(format!("open image: {e}")))?;
    let mut bytes = vec![0u8; len];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.read_exact(&mut bytes))
        .map_err(|e| YaoshiError::image(format!("read image bytes: {e}")))?;
    Ok(bytes)
}

pub fn check_installed_system_partition(image: &Path, part: &PartitionLayout) -> YaoshiResult<()> {
    check_installed_system_region_shape(image, part.start_byte, part.byte_size)
}

pub fn check_installed_system_partition_strict(
    image: &Path,
    part: &PartitionLayout,
) -> YaoshiResult<()> {
    let installed_root_bytes = part
        .byte_size
        .checked_sub(LEADING_GAP_BYTES + INSTALLED_ESP_SIZE_BYTES + TRAILING_GAP_BYTES)
        .ok_or_else(|| YaoshiError::image("installed-system payload is too small"))?;
    let layout = InstalledGptLayout::fixed(installed_root_bytes);
    check_installed_system_shape_at(image, part.start_byte, &layout, true)
}

pub fn check_installed_system_payload_shape(
    image: &Path,
    installed_system_bytes: u64,
) -> YaoshiResult<()> {
    check_installed_system_region_shape(image, 0, installed_system_bytes)
}

fn check_installed_system_region_shape(
    image: &Path,
    image_base: u64,
    installed_system_bytes: u64,
) -> YaoshiResult<()> {
    let installed_root_bytes = installed_system_bytes
        .checked_sub(LEADING_GAP_BYTES + INSTALLED_ESP_SIZE_BYTES + TRAILING_GAP_BYTES)
        .ok_or_else(|| YaoshiError::image("installed-system payload is too small"))?;
    let layout = InstalledGptLayout {
        disk_guid: Uuid::nil(),
        esp_guid: Uuid::nil(),
        root_guid: Uuid::nil(),
        root_bytes: installed_root_bytes,
    };
    check_installed_system_shape_at(image, image_base, &layout, false)
}

fn check_installed_system_shape_at(
    image: &Path,
    image_base: u64,
    layout: &InstalledGptLayout,
    strict_identity: bool,
) -> YaoshiResult<()> {
    parse_image_region(image, image_base, layout.image_bytes())?;
    let disk = parse_gpt_region(image, image_base, layout.image_bytes(), true)?;
    if strict_identity && disk.disk_guid != layout.disk_guid {
        return Err(YaoshiError::image(
            "installed-system GPT disk GUID mismatch",
        ));
    }
    if disk.partitions.len() != 2 {
        return Err(YaoshiError::image(
            "installed-system payload GPT partition count mismatch",
        ));
    }
    let esp_layout = layout.esp_partition();
    let root_layout = layout.root_partition();
    let esp = disk
        .partitions
        .iter()
        .find(|p| p.number == 1)
        .ok_or_else(|| YaoshiError::image("installed-system payload missing ESP"))?;
    let root = disk
        .partitions
        .iter()
        .find(|p| p.number == 2)
        .ok_or_else(|| YaoshiError::image("installed-system payload missing root"))?;
    if esp.name != INSTALLED_ESP_NAME
        || esp.type_guid != Uuid::parse_str(ESP_TYPE_GUID).unwrap()
        || esp.start_lba != esp_layout.start_lba()
        || esp.end_lba + 1 != esp_layout.start_lba() + esp_layout.sector_count()
    {
        return Err(YaoshiError::image("installed-system payload ESP mismatch"));
    }
    if root.name != INSTALLED_ROOT_NAME
        || root.type_guid != Uuid::parse_str(X86_64_ROOT_TYPE_GUID).unwrap()
        || root.start_lba != root_layout.start_lba()
        || root.end_lba + 1 != root_layout.start_lba() + root_layout.sector_count()
    {
        return Err(YaoshiError::image("installed-system payload root mismatch"));
    }
    if strict_identity
        && (esp.unique_guid != layout.esp_guid || root.unique_guid != layout.root_guid)
    {
        return Err(YaoshiError::image("installed partition GUID mismatch"));
    }
    let esp_files = validate_fat32_partition_files_from(
        image,
        image_base,
        &esp_layout,
        INSTALLED_ESP_LABEL,
        &[
            EFI_BOOT_PATH,
            "loader/loader.conf",
            "loader/entries/yaoshi.conf",
            INSTALLED_KERNEL_PATH,
            INSTALLED_INITRD_PATH,
            INSTALLED_KERNEL_RELEASE_PATH,
            "YAOSHI/CONFIG/HOSTNAME",
            "YAOSHI/CONFIG/AUTHKEYS",
            "YAOSHI/RUNTIME/PREPARE",
            "YAOSHI/RUNTIME/FIRST-BOOT",
            "YAOSHI/DASHBOARD/YAOSHI-DASHBOARD",
        ],
    )?;
    let boot_efi = &esp_files[0];
    let loader_conf = &esp_files[1];
    let loader_entry = &esp_files[2];
    let vmlinuz = &esp_files[3];
    let initrd = &esp_files[4];
    let kernel_release = &esp_files[5];
    let first_boot = &esp_files[9];
    let dashboard = &esp_files[10];
    if !boot_efi.starts_with(b"MZ") {
        return Err(YaoshiError::image(
            "installed-system payload BOOTX64.EFI is not PE/COFF",
        ));
    }
    if loader_conf != b"default yaoshi.conf\ntimeout 0\neditor no\n" {
        return Err(YaoshiError::image("installed loader.conf mismatch"));
    }
    if !loader_entry.starts_with(b"title Yaoshi Debian trixie\n")
        || !loader_entry.ends_with(b"console=ttyS0,115200n8 console=tty1 consoleblank=0\n")
        || !vmlinuz.starts_with(b"MZ")
        || initrd.is_empty()
        || !kernel_release.ends_with(b"\n")
    {
        return Err(YaoshiError::image("installed Debian boot tree mismatch"));
    }
    if first_boot.contains(&0) {
        return Err(YaoshiError::image(
            "installed first-boot script contains NUL",
        ));
    }
    validate_embedded_runtime_elf(dashboard)?;
    if ext4_label_in_image_from(image, image_base, &root_layout)? != INSTALLED_ROOT_LABEL {
        return Err(YaoshiError::image(
            "installed-system payload root ext4 label mismatch",
        ));
    }
    if ext4_magic_in_image_from(image, image_base, &root_layout)? != 0xEF53 {
        return Err(YaoshiError::image(
            "installed-system payload root ext4 magic mismatch",
        ));
    }
    if strict_identity {
        let partuuid = layout
            .root_guid
            .hyphenated()
            .to_string()
            .to_ascii_lowercase();
        let needle = format!("{INSTALLED_CMDLINE_PREFIX}{partuuid}{INSTALLED_CMDLINE_SUFFIX}");
        if !loader_entry
            .windows(needle.len())
            .any(|w| w == needle.as_bytes())
        {
            return Err(YaoshiError::image("installed loader command line mismatch"));
        }
    }
    Ok(())
}

fn validate_embedded_runtime_elf(bytes: &[u8]) -> YaoshiResult<()> {
    let file = object::File::parse(bytes)
        .map_err(|e| YaoshiError::image(format!("parse dashboard ELF: {e}")))?;
    if file.architecture() != object::Architecture::X86_64 || !file.is_64() {
        return Err(YaoshiError::image(
            "installed-system dashboard binary is not ELF64 x86-64",
        ));
    }
    if elf64_has_pt_interp(bytes)? {
        return Err(YaoshiError::image(
            "installed-system dashboard binary contains PT_INTERP",
        ));
    }
    Ok(())
}

fn elf64_has_pt_interp(bytes: &[u8]) -> YaoshiResult<bool> {
    const EI_CLASS: usize = 4;
    const EI_DATA: usize = 5;
    const ELFCLASS64: u8 = 2;
    const ELFDATA2LSB: u8 = 1;
    const PT_INTERP: u32 = 3;

    if bytes.len() < 64 || &bytes[0..4] != b"\x7fELF" {
        return Err(YaoshiError::image(
            "installed-system dashboard binary is not an ELF file",
        ));
    }
    if bytes[EI_CLASS] != ELFCLASS64 || bytes[EI_DATA] != ELFDATA2LSB {
        return Err(YaoshiError::image(
            "installed-system dashboard binary is not ELF64 little-endian",
        ));
    }

    let phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap()) as usize;
    let phentsize = u16::from_le_bytes(bytes[54..56].try_into().unwrap()) as usize;
    let phnum = u16::from_le_bytes(bytes[56..58].try_into().unwrap()) as usize;
    if phentsize < 56 {
        return Err(YaoshiError::image(
            "installed-system dashboard ELF program header size is invalid",
        ));
    }

    for idx in 0..phnum {
        let offset = phoff
            .checked_add(idx.checked_mul(phentsize).ok_or_else(|| {
                YaoshiError::image("installed-system dashboard ELF header offset overflows")
            })?)
            .ok_or_else(|| {
                YaoshiError::image("installed-system dashboard ELF header offset overflows")
            })?;
        let end = offset.checked_add(4).ok_or_else(|| {
            YaoshiError::image("installed-system dashboard ELF header offset overflows")
        })?;
        if end > bytes.len() {
            return Err(YaoshiError::image(
                "installed-system dashboard ELF program header exceeds file size",
            ));
        }
        if u32::from_le_bytes(bytes[offset..end].try_into().unwrap()) == PT_INTERP {
            return Ok(true);
        }
    }
    Ok(false)
}

fn parse_image_region(image: &Path, image_base: u64, image_bytes: u64) -> YaoshiResult<()> {
    let metadata = fs::metadata(image)
        .map_err(|e| YaoshiError::image(format!("stat installed-system payload: {e}")))?;
    if !metadata.is_file() && !metadata.file_type().is_block_device() {
        return Err(YaoshiError::image(
            "installed-system payload is not a regular file or block device",
        ));
    }
    let end = image_base
        .checked_add(image_bytes)
        .ok_or_else(|| YaoshiError::image("installed-system payload span overflows"))?;
    if metadata.is_file() && end > metadata.len() {
        return Err(YaoshiError::image("installed-system payload size mismatch"));
    }
    Ok(())
}

fn reject_iso9660_structures(image: &Path) -> YaoshiResult<()> {
    let mut file = File::open(image)
        .map_err(|e| YaoshiError::image(format!("open installer image for ISO9660 check: {e}")))?;
    for sector in 16u64..=18 {
        let offset = sector * 2048;
        file.seek(SeekFrom::Start(offset + 1))
            .map_err(|e| YaoshiError::image(format!("seek ISO9660 signature: {e}")))?;
        let mut signature = [0u8; 5];
        file.read_exact(&mut signature)
            .map_err(|e| YaoshiError::image(format!("read ISO9660 signature: {e}")))?;
        if &signature == b"CD001" {
            return Err(YaoshiError::image(
                "installer image contains ISO9660 structures",
            ));
        }
    }
    Ok(())
}

fn verify_zero_range(image: &Path, offset: u64, size: u64) -> YaoshiResult<()> {
    let mut file = File::open(image).map_err(|e| YaoshiError::image(format!("open image: {e}")))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| YaoshiError::image(format!("seek zero range: {e}")))?;
    let mut remaining = size;
    let mut buf = vec![0u8; 1024 * 1024];
    while remaining > 0 {
        let n = remaining.min(buf.len() as u64) as usize;
        file.read_exact(&mut buf[..n])
            .map_err(|e| YaoshiError::image(format!("read zero range: {e}")))?;
        if buf[..n].iter().any(|b| *b != 0) {
            return Err(YaoshiError::image("installer trailing gap is not zero"));
        }
        remaining -= n as u64;
    }
    Ok(())
}

fn compare_partition_bytes(
    image: &Path,
    offset: u64,
    size: u64,
    expected: &Path,
) -> YaoshiResult<()> {
    let expected_len = fs::metadata(expected)
        .map_err(|e| YaoshiError::image(format!("stat expected partition bytes: {e}")))?
        .len();
    if expected_len != size {
        return Err(YaoshiError::image("expected partition byte size mismatch"));
    }
    let mut a = File::open(image).map_err(|e| YaoshiError::image(format!("open image: {e}")))?;
    let mut b =
        File::open(expected).map_err(|e| YaoshiError::image(format!("open expected: {e}")))?;
    a.seek(SeekFrom::Start(offset))
        .map_err(|e| YaoshiError::image(format!("seek image partition: {e}")))?;
    let mut remaining = size;
    let mut buf_a = vec![0u8; 1024 * 1024];
    let mut buf_b = vec![0u8; 1024 * 1024];
    while remaining > 0 {
        let n = remaining.min(buf_a.len() as u64) as usize;
        a.read_exact(&mut buf_a[..n])
            .map_err(|e| YaoshiError::image(format!("read image partition: {e}")))?;
        b.read_exact(&mut buf_b[..n])
            .map_err(|e| YaoshiError::image(format!("read expected bytes: {e}")))?;
        if buf_a[..n] != buf_b[..n] {
            return Err(YaoshiError::image("partition bytes differ"));
        }
        remaining -= n as u64;
    }
    Ok(())
}
