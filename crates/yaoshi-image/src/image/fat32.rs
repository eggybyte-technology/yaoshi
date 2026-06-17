pub fn create_fat32_image(path: &Path, label: &str, files: &[TreeFile]) -> YaoshiResult<()> {
    let image_bytes = match label {
        INSTALLED_ESP_LABEL => INSTALLED_ESP_SIZE_BYTES,
        INSTALLER_BOOT_LABEL => INSTALLER_BOOT_SIZE_BYTES,
        _ => return Err(YaoshiError::image("unknown FAT32 image label")),
    };
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| YaoshiError::image(format!("create FAT32 image: {e}")))?;
    file.set_len(image_bytes)
        .map_err(|e| YaoshiError::image(format!("size FAT32 image: {e}")))?;
    let options = FormatVolumeOptions::new()
        .fat_type(fatfs::FatType::Fat32)
        .bytes_per_sector(512)
        .volume_label(fat_label(label)?);
    fatfs::format_volume(BufStream::new(file), options)
        .map_err(|e| YaoshiError::image(format!("format FAT32 image: {e}")))?;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| YaoshiError::image(format!("reopen FAT32 image: {e}")))?;
    let fs = FileSystem::new(BufStream::new(file), FsOptions::new())
        .map_err(|e| YaoshiError::image(format!("open FAT32 image: {e}")))?;
    let root = fs.root_dir();
    for tree_file in files {
        let mut dir = root.clone();
        let components: Vec<String> = tree_file
            .path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        if components.is_empty() {
            return Err(YaoshiError::image("empty FAT32 tree path"));
        }
        for component in &components[..components.len() - 1] {
            dir = match dir.open_dir(component) {
                Ok(existing) => existing,
                Err(_) => dir
                    .create_dir(component)
                    .map_err(|e| YaoshiError::image(format!("create FAT32 dir: {e}")))?,
            };
        }
        let mut f = dir
            .create_file(components.last().unwrap())
            .map_err(|e| YaoshiError::image(format!("create FAT32 file: {e}")))?;
        f.write_all(&tree_file.bytes)
            .map_err(|e| YaoshiError::image(format!("write FAT32 file: {e}")))?;
    }
    drop(root);
    drop(fs);
    validate_fat32_tree(path, label, files)
}

fn fat_label(label: &str) -> YaoshiResult<[u8; 11]> {
    let bytes = label.as_bytes();
    if bytes.len() > 11 || !bytes.iter().all(|b| b.is_ascii()) {
        return Err(YaoshiError::image("invalid FAT32 label"));
    }
    let mut out = [b' '; 11];
    out[..bytes.len()].copy_from_slice(bytes);
    Ok(out)
}

pub fn validate_fat32_tree(path: &Path, label: &str, files: &[TreeFile]) -> YaoshiResult<()> {
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|e| YaoshiError::image(format!("open FAT32 image: {e}")))?;
    let fs = FileSystem::new(BufStream::new(file), FsOptions::new())
        .map_err(|e| YaoshiError::image(format!("parse FAT32 image: {e}")))?;
    let volume_label = fs.volume_label().trim().to_string();
    if volume_label != label {
        return Err(YaoshiError::image(format!(
            "FAT32 label mismatch: expected {label}, got {volume_label}"
        )));
    }
    let root = fs.root_dir();
    for expected in files {
        let mut dir = root.clone();
        let components: Vec<String> = expected
            .path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        for component in &components[..components.len() - 1] {
            dir = dir
                .open_dir(component)
                .map_err(|e| YaoshiError::image(format!("open FAT32 dir {component}: {e}")))?;
        }
        let mut f = dir
            .open_file(components.last().unwrap())
            .map_err(|e| YaoshiError::image(format!("open FAT32 file: {e}")))?;
        let mut actual = Vec::new();
        f.read_to_end(&mut actual)
            .map_err(|e| YaoshiError::image(format!("read FAT32 file: {e}")))?;
        if actual != expected.bytes {
            return Err(YaoshiError::image(format!(
                "FAT32 file bytes mismatch: {}",
                expected.path.display()
            )));
        }
    }
    let actual_tree = collect_fat32_tree(&root)?;
    let expected_paths = files
        .iter()
        .map(|file| normalized_fat_path(&file.path))
        .collect::<BTreeSet<_>>();
    let expected_dirs = expected_fat_dirs_from_files(&expected_paths);
    if actual_tree.files != expected_paths || actual_tree.dirs != expected_dirs {
        return Err(YaoshiError::image("FAT32 tree contains unexpected files"));
    }
    Ok(())
}

pub fn extract_fat_file_from_image(
    image: &Path,
    part: &PartitionLayout,
    tree_path: &str,
) -> YaoshiResult<Vec<u8>> {
    let file = File::open(image).map_err(|e| YaoshiError::image(format!("open image: {e}")))?;
    let region = BoundedRegion::new(file, part.start_byte, part.byte_size);
    let fs = FileSystem::new(BufStream::new(region), FsOptions::new())
        .map_err(|e| YaoshiError::image(format!("parse FAT slice: {e}")))?;
    let mut dir = fs.root_dir();
    let components: Vec<&str> = tree_path.split('/').filter(|s| !s.is_empty()).collect();
    for component in &components[..components.len() - 1] {
        dir = dir
            .open_dir(component)
            .map_err(|e| YaoshiError::image(format!("open FAT dir: {e}")))?;
    }
    let mut file = dir
        .open_file(components.last().unwrap())
        .map_err(|e| YaoshiError::image(format!("open FAT file: {e}")))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| YaoshiError::image(format!("read FAT file: {e}")))?;
    Ok(bytes)
}

pub fn validate_fat32_partition(
    image: &Path,
    part: &PartitionLayout,
    label: &str,
    expected_file: &str,
) -> YaoshiResult<Vec<u8>> {
    validate_fat32_partition_from(image, 0, part, label, expected_file)
}

pub fn validate_fat32_partition_files(
    image: &Path,
    part: &PartitionLayout,
    label: &str,
    expected_files: &[&str],
) -> YaoshiResult<Vec<Vec<u8>>> {
    validate_fat32_partition_files_from(image, 0, part, label, expected_files)
}

fn validate_fat32_partition_from(
    image: &Path,
    image_base: u64,
    part: &PartitionLayout,
    label: &str,
    expected_file: &str,
) -> YaoshiResult<Vec<u8>> {
    let file = File::open(image).map_err(|e| YaoshiError::image(format!("open image: {e}")))?;
    let start = region_offset(image_base, part.start_byte, "FAT32 partition")?;
    let region = BoundedRegion::new(file, start, part.byte_size);
    validate_fat32_reader(region, label, expected_file)
}

fn validate_fat32_partition_files_from(
    image: &Path,
    image_base: u64,
    part: &PartitionLayout,
    label: &str,
    expected_files: &[&str],
) -> YaoshiResult<Vec<Vec<u8>>> {
    let file = File::open(image).map_err(|e| YaoshiError::image(format!("open image: {e}")))?;
    let start = region_offset(image_base, part.start_byte, "FAT32 partition")?;
    let region = BoundedRegion::new(file, start, part.byte_size);
    validate_fat32_reader_files(region, label, expected_files)
}

fn validate_fat32_reader<T: fatfs::ReadWriteSeek>(
    reader: T,
    label: &str,
    expected_file: &str,
) -> YaoshiResult<Vec<u8>> {
    let files = validate_fat32_reader_files(reader, label, &[expected_file])?;
    Ok(files.into_iter().next().unwrap_or_default())
}

fn validate_fat32_reader_files<T: fatfs::ReadWriteSeek>(
    reader: T,
    label: &str,
    expected_files: &[&str],
) -> YaoshiResult<Vec<Vec<u8>>> {
    let fs = FileSystem::new(BufStream::new(reader), FsOptions::new())
        .map_err(|e| YaoshiError::image(format!("parse FAT32 partition: {e}")))?;
    let volume_label = fs.volume_label().trim().to_string();
    if volume_label != label {
        return Err(YaoshiError::image(format!(
            "FAT32 label mismatch: expected {label}, got {volume_label}"
        )));
    }
    let mut out = Vec::new();
    for expected_file in expected_files {
        let mut dir = fs.root_dir();
        let components: Vec<&str> = expected_file.split('/').filter(|s| !s.is_empty()).collect();
        if components.is_empty() {
            return Err(YaoshiError::image("empty FAT32 expected path"));
        }
        for component in &components[..components.len() - 1] {
            dir = dir
                .open_dir(component)
                .map_err(|e| YaoshiError::image(format!("open FAT32 dir {component}: {e}")))?;
        }
        let mut file = dir
            .open_file(components.last().unwrap())
            .map_err(|e| YaoshiError::image(format!("open FAT32 file: {e}")))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| YaoshiError::image(format!("read FAT32 file: {e}")))?;
        out.push(bytes);
    }
    let actual_tree = collect_fat32_tree(&fs.root_dir())?;
    let expected_paths = expected_files
        .iter()
        .map(|path| normalized_fat_str_path(path))
        .collect::<BTreeSet<_>>();
    let expected_dirs = expected_fat_dirs_from_files(&expected_paths);
    if actual_tree.files != expected_paths || actual_tree.dirs != expected_dirs {
        return Err(YaoshiError::image(
            "FAT32 partition contains unexpected files",
        ));
    }
    Ok(out)
}

#[derive(Debug, Default)]
struct Fat32Tree {
    dirs: BTreeSet<String>,
    files: BTreeSet<String>,
}

fn collect_fat32_tree<T: fatfs::ReadWriteSeek>(dir: &fatfs::Dir<'_, T>) -> YaoshiResult<Fat32Tree> {
    let mut out = Fat32Tree::default();
    out.dirs.insert("/".to_string());
    collect_fat32_tree_from(dir, "", &mut out)?;
    Ok(out)
}

fn collect_fat32_tree_from<T: fatfs::ReadWriteSeek>(
    dir: &fatfs::Dir<'_, T>,
    prefix: &str,
    out: &mut Fat32Tree,
) -> YaoshiResult<()> {
    for entry in dir.iter() {
        let entry = entry.map_err(|e| YaoshiError::image(format!("read FAT32 dir entry: {e}")))?;
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let path = if prefix.is_empty() {
            format!("/{name}")
        } else {
            format!("{prefix}/{name}")
        };
        if entry.is_dir() {
            out.dirs.insert(path.clone());
            let child = entry.to_dir();
            collect_fat32_tree_from(&child, &path, out)?;
        } else if entry.is_file() {
            out.files.insert(path);
        }
    }
    Ok(())
}

fn expected_fat_dirs_from_files(files: &BTreeSet<String>) -> BTreeSet<String> {
    let mut dirs = BTreeSet::from(["/".to_string()]);
    for file in files {
        let mut path = file.trim_matches('/').split('/').collect::<Vec<_>>();
        path.pop();
        let mut current = String::new();
        for component in path {
            current.push('/');
            current.push_str(component);
            dirs.insert(current.clone());
        }
    }
    dirs
}

fn normalized_fat_path(path: &Path) -> String {
    let joined = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    normalized_fat_str_path(&joined)
}

fn normalized_fat_str_path(path: &str) -> String {
    format!("/{}", path.trim_matches('/'))
}

fn ext4_label_in_image_from(
    image: &Path,
    image_base: u64,
    part: &PartitionLayout,
) -> YaoshiResult<String> {
    let mut file = File::open(image).map_err(|e| YaoshiError::image(format!("open ext4: {e}")))?;
    let label_offset = region_offset(image_base, part.start_byte + 1024 + 120, "ext4 label")?;
    file.seek(SeekFrom::Start(label_offset))
        .map_err(|e| YaoshiError::image(format!("seek ext4 label: {e}")))?;
    let mut raw = [0u8; 16];
    file.read_exact(&mut raw)
        .map_err(|e| YaoshiError::image(format!("read ext4 label: {e}")))?;
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    Ok(String::from_utf8_lossy(&raw[..end]).to_string())
}

fn ext4_magic_in_image_from(
    image: &Path,
    image_base: u64,
    part: &PartitionLayout,
) -> YaoshiResult<u16> {
    let mut file = File::open(image).map_err(|e| YaoshiError::image(format!("open ext4: {e}")))?;
    let magic_offset = region_offset(image_base, part.start_byte + 1024 + 0x38, "ext4 magic")?;
    file.seek(SeekFrom::Start(magic_offset))
        .map_err(|e| YaoshiError::image(format!("seek ext4 magic: {e}")))?;
    let mut raw = [0u8; 2];
    file.read_exact(&mut raw)
        .map_err(|e| YaoshiError::image(format!("read ext4 magic: {e}")))?;
    Ok(u16::from_le_bytes(raw))
}
