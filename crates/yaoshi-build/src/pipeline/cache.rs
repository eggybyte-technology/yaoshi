fn cache_file_phase(
    context: &BuildContext,
    phase: &str,
    input: serde_json::Value,
    work_output: &Path,
    build: impl FnOnce(&Path) -> YaoshiResult<()>,
) -> YaoshiResult<PathBuf> {
    let key = cache_input_key(input)?;
    if let Some(object) = restore_file_cache(context, phase, &key)? {
        return Ok(object);
    }
    prepare_work_root_on_first_producer_miss(context)?;
    if let Some(parent) = work_output.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::build(format!("create phase output parent: {e}")))?;
    }
    let _ = fs::remove_file(work_output);
    build(work_output)?;
    store_file_cache(context, phase, &key, work_output)
}

fn cache_tree_phase(
    context: &BuildContext,
    phase: &str,
    input: serde_json::Value,
    work_output: &Path,
    build: impl FnOnce(&Path) -> YaoshiResult<()>,
) -> YaoshiResult<PathBuf> {
    let key = cache_input_key(input)?;
    if let Some(object) = restore_tree_cache(context, phase, &key)? {
        return Ok(object);
    }
    prepare_work_root_on_first_producer_miss(context)?;
    if work_output.exists() {
        fs::remove_dir_all(work_output)
            .map_err(|e| YaoshiError::build(format!("reset phase tree output: {e}")))?;
    }
    fs::create_dir_all(work_output)
        .map_err(|e| YaoshiError::build(format!("create phase tree output: {e}")))?;
    build(work_output)?;
    store_tree_cache(context, phase, &key, work_output)
}

fn cache_composite_phase(
    context: &BuildContext,
    phase: &str,
    input: serde_json::Value,
    build: impl FnOnce() -> YaoshiResult<(String, Vec<u8>)>,
) -> YaoshiResult<PathBuf> {
    let key = cache_input_key(input)?;
    if let Some(object) = restore_composite_cache(context, phase, &key)? {
        return Ok(object);
    }
    prepare_work_root_on_first_producer_miss(context)?;
    let (logical_digest, manifest) = build()?;
    store_composite_cache(context, phase, &key, &logical_digest, &manifest)
}

fn cache_input_key(input: serde_json::Value) -> YaoshiResult<String> {
    Ok(sha256_bytes(&canonical_json_bytes(&input)?))
}

fn restore_file_cache(
    context: &BuildContext,
    phase: &str,
    key: &str,
) -> YaoshiResult<Option<PathBuf>> {
    let paths = Paths::new(context);
    let entry = cache_index_path(&paths.cache, phase, key);
    let Ok(output_ref) = read_cache_entry(&entry) else {
        return Ok(None);
    };
    if output_ref.kind != OutputKind::File {
        return Ok(None);
    }
    let object = output_ref.object_path(&paths.cache);
    if !is_named_cache_object(&object, output_ref.digest.as_str()) || !object.is_file() {
        return Ok(None);
    }
    Ok(Some(object))
}

fn restore_tree_cache(
    context: &BuildContext,
    phase: &str,
    key: &str,
) -> YaoshiResult<Option<PathBuf>> {
    let paths = Paths::new(context);
    let entry = cache_index_path(&paths.cache, phase, key);
    let Ok(output_ref) = read_cache_entry(&entry) else {
        return Ok(None);
    };
    if output_ref.kind != OutputKind::Tree {
        return Ok(None);
    }
    let object = output_ref.object_path(&paths.cache);
    if !is_named_cache_object(&object, output_ref.digest.as_str()) || !object.is_dir() {
        return Ok(None);
    }
    Ok(Some(object))
}

fn restore_composite_cache(
    context: &BuildContext,
    phase: &str,
    key: &str,
) -> YaoshiResult<Option<PathBuf>> {
    let paths = Paths::new(context);
    let entry = cache_index_path(&paths.cache, phase, key);
    let Ok(output_ref) = read_cache_entry(&entry) else {
        return Ok(None);
    };
    if output_ref.kind != OutputKind::CompositeFile {
        return Ok(None);
    }
    let object = output_ref.object_path(&paths.cache);
    if !is_named_cache_object(&object, output_ref.digest.as_str()) || !object.is_file() {
        return Ok(None);
    }
    Ok(Some(object))
}

fn store_file_cache(
    context: &BuildContext,
    phase: &str,
    key: &str,
    output: &Path,
) -> YaoshiResult<PathBuf> {
    if !output.is_file() {
        return Err(YaoshiError::build(format!(
            "phase {phase} did not produce a regular file"
        )));
    }
    let paths = Paths::new(context);
    let digest = sha256_file_hex(output)?;
    let object_dir = paths.cache.join("object/file");
    fs::create_dir_all(&object_dir)
        .map_err(|e| YaoshiError::build(format!("create file object dir: {e}")))?;
    let object = object_dir.join(&digest);
    if !object.exists() {
        let tmp = object_dir.join(format!(".{digest}.tmp"));
        fs::copy(output, &tmp)
            .map_err(|e| YaoshiError::build(format!("write file object for {phase}: {e}")))?;
        File::open(&tmp)
            .and_then(|file| file.sync_all())
            .map_err(|e| YaoshiError::build(format!("fsync file object for {phase}: {e}")))?;
        fs::rename(&tmp, &object)
            .map_err(|e| YaoshiError::build(format!("install file object for {phase}: {e}")))?;
        fsync_dir(&object_dir)?;
    }
    write_cache_entry(&paths.cache, phase, key, OutputKind::File, &digest)?;
    Ok(object)
}

fn store_tree_cache(
    context: &BuildContext,
    phase: &str,
    key: &str,
    output: &Path,
) -> YaoshiResult<PathBuf> {
    if !output.is_dir() {
        return Err(YaoshiError::build(format!(
            "phase {phase} did not produce a tree"
        )));
    }
    let paths = Paths::new(context);
    let digest = tree_sha256_hex(output)?;
    let object_dir = paths.cache.join("object/tree");
    fs::create_dir_all(&object_dir)
        .map_err(|e| YaoshiError::build(format!("create tree object dir: {e}")))?;
    let object = object_dir.join(&digest);
    if !object.exists() {
        let tmp = object_dir.join(format!(".{digest}.tmp"));
        copy_dir_recursive(output, &tmp)?;
        fsync_tree(&tmp)?;
        fs::rename(&tmp, &object)
            .map_err(|e| YaoshiError::build(format!("install tree object for {phase}: {e}")))?;
        fsync_dir(&object_dir)?;
    }
    write_cache_entry(&paths.cache, phase, key, OutputKind::Tree, &digest)?;
    Ok(object)
}

fn store_composite_cache(
    context: &BuildContext,
    phase: &str,
    key: &str,
    logical_digest: &str,
    manifest: &[u8],
) -> YaoshiResult<PathBuf> {
    if !is_lower_sha256_hex(logical_digest) {
        return Err(YaoshiError::image("composite logical digest is invalid"));
    }
    validate_composite_manifest_bytes(manifest, Some(&context.cache_root))?;
    let value: Value = serde_json::from_slice(manifest)
        .map_err(|e| YaoshiError::image(format!("parse composite manifest: {e}")))?;
    if value
        .get("logical_sha256")
        .and_then(Value::as_str)
        .is_none_or(|actual| actual != logical_digest)
    {
        return Err(YaoshiError::image(
            "composite manifest logical digest mismatch",
        ));
    }
    let object_dir = context.cache_root.join("object/composite-file");
    fs::create_dir_all(&object_dir)
        .map_err(|e| YaoshiError::image(format!("create composite object dir: {e}")))?;
    let object = object_dir.join(logical_digest);
    if !object.exists() {
        let tmp = object_dir.join(format!(".{logical_digest}.tmp"));
        fs::write(&tmp, manifest)
            .map_err(|e| YaoshiError::image(format!("write composite object for {phase}: {e}")))?;
        File::open(&tmp)
            .and_then(|file| file.sync_all())
            .map_err(|e| YaoshiError::image(format!("fsync composite object for {phase}: {e}")))?;
        fs::rename(&tmp, &object).map_err(|e| {
            YaoshiError::image(format!("install composite object for {phase}: {e}"))
        })?;
        fsync_dir(&object_dir)?;
    }
    write_cache_entry(
        &context.cache_root,
        phase,
        key,
        OutputKind::CompositeFile,
        logical_digest,
    )?;
    Ok(object)
}

fn validate_composite_manifest_bytes(
    manifest: &[u8],
    cache_root: Option<&Path>,
) -> YaoshiResult<()> {
    let value: Value = serde_json::from_slice(manifest)
        .map_err(|e| YaoshiError::image(format!("parse composite manifest: {e}")))?;
    if canonical_json_bytes(&value)? != manifest {
        return Err(YaoshiError::image(
            "composite manifest is not canonical JSON",
        ));
    }
    let object = value
        .as_object()
        .ok_or_else(|| YaoshiError::image("composite manifest root is not an object"))?;
    let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = [
        "grammar",
        "kind",
        "logical_sha256",
        "logical_size",
        "segments",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    if keys != expected {
        return Err(YaoshiError::image("composite manifest root keys mismatch"));
    }
    if object.get("grammar").and_then(Value::as_str) != Some(COMPOSITE_FILE_GRAMMAR)
        || object.get("kind").and_then(Value::as_str) != Some("composite-file")
    {
        return Err(YaoshiError::image(
            "composite manifest fixed field mismatch",
        ));
    }
    let logical_sha256 = object
        .get("logical_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| YaoshiError::image("composite logical_sha256 is invalid"))?;
    if !is_lower_sha256_hex(logical_sha256) {
        return Err(YaoshiError::image("composite logical_sha256 is invalid"));
    }
    let logical_size = object
        .get("logical_size")
        .and_then(Value::as_u64)
        .ok_or_else(|| YaoshiError::image("composite logical_size is invalid"))?;
    let segments = object
        .get("segments")
        .and_then(Value::as_array)
        .ok_or_else(|| YaoshiError::image("composite segments are invalid"))?;
    let mut cursor = 0u64;
    for segment in segments {
        let segment = segment
            .as_object()
            .ok_or_else(|| YaoshiError::image("composite segment is not an object"))?;
        let kind = segment
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| YaoshiError::image("composite segment kind is invalid"))?;
        let offset = segment
            .get("offset")
            .and_then(Value::as_u64)
            .ok_or_else(|| YaoshiError::image("composite segment offset is invalid"))?;
        let len = segment
            .get("len")
            .and_then(Value::as_u64)
            .ok_or_else(|| YaoshiError::image("composite segment length is invalid"))?;
        if offset != cursor || len == 0 {
            return Err(YaoshiError::image(
                "composite segment map is not complete and non-overlapping",
            ));
        }
        cursor = cursor
            .checked_add(len)
            .ok_or_else(|| YaoshiError::image("composite segment range overflows"))?;
        match kind {
            "inline-bytes" => {
                let keys = segment.keys().map(String::as_str).collect::<BTreeSet<_>>();
                let expected = ["bytes_hex", "kind", "len", "offset", "sha256"]
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                if keys != expected {
                    return Err(YaoshiError::image("composite inline segment keys mismatch"));
                }
                let bytes = hex_bytes(
                    segment
                        .get("bytes_hex")
                        .and_then(Value::as_str)
                        .ok_or_else(|| YaoshiError::image("composite inline bytes invalid"))?,
                )?;
                if bytes.len() as u64 != len {
                    return Err(YaoshiError::image("composite inline length mismatch"));
                }
                let sha256 = segment
                    .get("sha256")
                    .and_then(Value::as_str)
                    .ok_or_else(|| YaoshiError::image("composite inline sha256 invalid"))?;
                if sha256_bytes(&bytes) != sha256 {
                    return Err(YaoshiError::image("composite inline sha256 mismatch"));
                }
            }
            "zero" => {
                let keys = segment.keys().map(String::as_str).collect::<BTreeSet<_>>();
                let expected = ["kind", "len", "offset"]
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                if keys != expected {
                    return Err(YaoshiError::image("composite zero segment keys mismatch"));
                }
            }
            "file-ref" => {
                let keys = segment.keys().map(String::as_str).collect::<BTreeSet<_>>();
                let expected = ["digest", "kind", "len", "offset", "source_offset"]
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                if keys != expected {
                    return Err(YaoshiError::image(
                        "composite file-ref segment keys mismatch",
                    ));
                }
                let digest = segment
                    .get("digest")
                    .and_then(Value::as_str)
                    .ok_or_else(|| YaoshiError::image("composite file-ref digest invalid"))?;
                if !is_lower_sha256_hex(digest) {
                    return Err(YaoshiError::image("composite file-ref digest invalid"));
                }
                let source_offset = segment
                    .get("source_offset")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        YaoshiError::image("composite file-ref source offset invalid")
                    })?;
                if let Some(cache_root) = cache_root {
                    let object = cache_root.join("object/file").join(digest);
                    let meta = fs::metadata(&object).map_err(|e| {
                        YaoshiError::image(format!("stat composite file-ref object: {e}"))
                    })?;
                    if !is_named_cache_object(&object, digest)
                        || !meta.is_file()
                        || source_offset
                            .checked_add(len)
                            .is_none_or(|end| end > meta.len())
                    {
                        return Err(YaoshiError::image(
                            "composite file-ref object validation failed",
                        ));
                    }
                }
            }
            _ => return Err(YaoshiError::image("unsupported composite segment kind")),
        }
    }
    if cursor != logical_size {
        return Err(YaoshiError::image(
            "composite segments do not cover logical size",
        ));
    }
    Ok(())
}

fn is_named_cache_object(path: &Path, digest: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == digest && is_lower_sha256_hex(name))
}

fn write_cache_entry(
    cache: &Path,
    phase: &str,
    key: &str,
    kind: OutputKind,
    digest: &str,
) -> YaoshiResult<()> {
    let entry_dir = cache.join("index").join(cache_index_branch(phase));
    fs::create_dir_all(&entry_dir)
        .map_err(|e| YaoshiError::build(format!("create cache entry dir: {e}")))?;
    let tmp = entry_dir.join(format!(".{key}.tmp"));
    fs::write(
        &tmp,
        format!("kind={}\ndigest={digest}\n", kind.cache_dir_name()),
    )
        .map_err(|e| YaoshiError::build(format!("write cache entry for {phase}: {e}")))?;
    File::open(&tmp)
        .and_then(|file| file.sync_all())
        .map_err(|e| YaoshiError::build(format!("fsync cache entry for {phase}: {e}")))?;
    fs::rename(&tmp, entry_dir.join(key))
        .map_err(|e| YaoshiError::build(format!("install cache entry for {phase}: {e}")))?;
    fsync_dir(&entry_dir)
}

fn read_cache_entry(path: &Path) -> YaoshiResult<OutputRef> {
    let text = fs::read_to_string(path)
        .map_err(|e| YaoshiError::build(format!("read cache index {}: {e}", path.display())))?;
    let mut lines = text.lines();
    let kind = match lines.next().and_then(|line| line.strip_prefix("kind=")) {
        Some("file") => OutputKind::File,
        Some("tree") => OutputKind::Tree,
        Some("composite-file") => OutputKind::CompositeFile,
        _ => return Err(YaoshiError::build("cache index kind is invalid")),
    };
    let digest = lines
        .next()
        .and_then(|line| line.strip_prefix("digest="))
        .ok_or_else(|| YaoshiError::build("cache index digest is missing"))?;
    if lines.next().is_some() || !text.ends_with('\n') {
        return Err(YaoshiError::build("cache index has extra fields"));
    }
    Ok(OutputRef::new(kind, Sha256Hex::parse(digest.to_string())?))
}

fn cache_index_path(cache: &Path, phase: &str, key: &str) -> PathBuf {
    cache.join("index").join(cache_index_branch(phase)).join(key)
}

fn cache_index_branch(phase: &str) -> &'static str {
    if phase.starts_with("resolve-debian-package-root")
        || phase.starts_with("compute-installer-module-closure")
        || phase.starts_with("pack-installer-base-initramfs")
    {
        "foundation"
    } else if phase.starts_with("resolve-customized-root")
        || phase.starts_with("resolve-root-bridge-overlay")
        || phase.starts_with("resolve-installed-root-source")
        || phase.starts_with("resolve-installed-root-ext4")
    {
        "root"
    } else if phase.starts_with("build-yaoshi-")
        || phase.starts_with("pack-installed-esp")
        || phase.starts_with("pack-installed-system-payload")
        || phase.starts_with("pack-installer-app-initramfs")
    {
        "runtime"
    } else {
        "image"
    }
}

fn prepare_work_root_on_first_producer_miss(context: &BuildContext) -> YaoshiResult<()> {
    if context.work_prepared.replace(true) {
        return Ok(());
    }
    check_environment(context)?;
    prepare_work_root(context)
}

fn tree_sha256_hex(root: &Path) -> YaoshiResult<String> {
    let mut collected = Vec::new();
    collect_tree_digest_inputs(root, root, &mut collected)?;
    let mut entries = resolve_tree_digest_entries(collected)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (rel, entry) in entries {
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        match entry {
            TreeDigestEntry::Directory { mode } => {
                hasher.update(b"dir");
                hasher.update(mode.to_le_bytes());
            }
            TreeDigestEntry::File { mode, sha256 } => {
                hasher.update(b"file");
                hasher.update(mode.to_le_bytes());
                hasher.update(sha256.as_bytes());
            }
            TreeDigestEntry::Symlink { target } => {
                hasher.update(b"symlink");
                hasher.update(target.as_bytes());
            }
        }
        hasher.update([0xff]);
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

enum TreeDigestInput {
    Directory { mode: u32 },
    File { mode: u32, path: PathBuf },
    Symlink { target: String },
}

enum TreeDigestEntry {
    Directory { mode: u32 },
    File { mode: u32, sha256: String },
    Symlink { target: String },
}

fn collect_tree_digest_inputs(
    root: &Path,
    current: &Path,
    out: &mut Vec<(String, TreeDigestInput)>,
) -> YaoshiResult<()> {
    for entry in fs::read_dir(current)
        .map_err(|e| YaoshiError::build(format!("read tree {}: {e}", current.display())))?
    {
        let entry = entry.map_err(|e| YaoshiError::build(format!("read tree entry: {e}")))?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map_err(|e| YaoshiError::internal(format!("strip tree path: {e}")))?
            .to_string_lossy()
            .replace('\\', "/");
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| YaoshiError::build(format!("stat tree entry {}: {e}", path.display())))?;
        let mode = meta.mode() & 0o7777;
        if meta.is_dir() {
            out.push((rel, TreeDigestInput::Directory { mode }));
            collect_tree_digest_inputs(root, &path, out)?;
        } else if meta.is_file() {
            out.push((
                rel,
                TreeDigestInput::File {
                    mode,
                    path,
                },
            ));
        } else if meta.file_type().is_symlink() {
            let target = fs::read_link(&path)
                .map_err(|e| YaoshiError::build(format!("read symlink {}: {e}", path.display())))?;
            out.push((
                rel,
                TreeDigestInput::Symlink {
                    target: target.to_string_lossy().into_owned(),
                },
            ));
        } else {
            return Err(YaoshiError::build(format!(
                "unsupported tree object entry type: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn resolve_tree_digest_entries(
    inputs: Vec<(String, TreeDigestInput)>,
) -> YaoshiResult<Vec<(String, TreeDigestEntry)>> {
    if inputs.len() <= 1 {
        return inputs
            .into_iter()
            .map(|(rel, input)| resolve_tree_digest_entry(rel, input))
            .collect();
    }
    let worker_count = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(inputs.len())
        .max(1);
    if worker_count <= 1 {
        return inputs
            .into_iter()
            .map(|(rel, input)| resolve_tree_digest_entry(rel, input))
            .collect();
    }
    let chunk_size = inputs.len().div_ceil(worker_count);
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in inputs.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                chunk
                    .iter()
                    .map(|(rel, input)| resolve_tree_digest_entry_ref(rel, input))
                    .collect::<YaoshiResult<Vec<_>>>()
            }));
        }
        let mut entries = Vec::new();
        for handle in handles {
            entries.extend(
                handle
                    .join()
                    .map_err(|_| YaoshiError::internal("tree digest worker panicked"))??,
            );
        }
        Ok(entries)
    })
}

fn resolve_tree_digest_entry(
    rel: String,
    input: TreeDigestInput,
) -> YaoshiResult<(String, TreeDigestEntry)> {
    match input {
        TreeDigestInput::Directory { mode } => Ok((rel, TreeDigestEntry::Directory { mode })),
        TreeDigestInput::File { mode, path } => Ok((
            rel,
            TreeDigestEntry::File {
                mode,
                sha256: sha256_file_hex(&path)?,
            },
        )),
        TreeDigestInput::Symlink { target } => Ok((rel, TreeDigestEntry::Symlink { target })),
    }
}

fn resolve_tree_digest_entry_ref(
    rel: &str,
    input: &TreeDigestInput,
) -> YaoshiResult<(String, TreeDigestEntry)> {
    match input {
        TreeDigestInput::Directory { mode } => {
            Ok((rel.to_string(), TreeDigestEntry::Directory { mode: *mode }))
        }
        TreeDigestInput::File { mode, path } => Ok((
            rel.to_string(),
            TreeDigestEntry::File {
                mode: *mode,
                sha256: sha256_file_hex(path)?,
            },
        )),
        TreeDigestInput::Symlink { target } => Ok((
            rel.to_string(),
            TreeDigestEntry::Symlink {
                target: target.clone(),
            },
        )),
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> YaoshiResult<()> {
    if dst.exists() {
        fs::remove_dir_all(dst)
            .map_err(|e| YaoshiError::build(format!("remove directory {}: {e}", dst.display())))?;
    }
    fs::create_dir_all(dst)
        .map_err(|e| YaoshiError::build(format!("create directory {}: {e}", dst.display())))?;
    for entry in fs::read_dir(src)
        .map_err(|e| YaoshiError::build(format!("read directory {}: {e}", src.display())))?
    {
        let entry = entry.map_err(|e| YaoshiError::build(format!("read directory entry: {e}")))?;
        let ty = entry
            .file_type()
            .map_err(|e| YaoshiError::build(format!("read file type: {e}")))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_file() {
            fs::copy(&from, &to)
                .map_err(|e| YaoshiError::build(format!("copy file {}: {e}", from.display())))?;
            let mode = fs::metadata(&from)
                .map_err(|e| YaoshiError::build(format!("stat copied file: {e}")))?
                .permissions()
                .mode();
            fs::set_permissions(&to, fs::Permissions::from_mode(mode))
                .map_err(|e| YaoshiError::build(format!("chmod copied file: {e}")))?;
        } else if ty.is_symlink() {
            let target = fs::read_link(&from)
                .map_err(|e| YaoshiError::build(format!("read symlink {}: {e}", from.display())))?;
            symlink_force(&target.to_string_lossy(), &to)?;
        }
    }
    Ok(())
}

fn fsync_tree(root: &Path) -> YaoshiResult<()> {
    for entry in fs::read_dir(root)
        .map_err(|e| YaoshiError::build(format!("read tree for fsync {}: {e}", root.display())))?
    {
        let entry = entry.map_err(|e| YaoshiError::build(format!("read tree fsync entry: {e}")))?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| YaoshiError::build(format!("stat tree fsync entry: {e}")))?;
        if meta.is_dir() {
            fsync_tree(&path)?;
        } else if meta.is_file() {
            File::open(&path)
                .and_then(|file| file.sync_all())
                .map_err(|e| {
                    YaoshiError::build(format!("fsync tree file {}: {e}", path.display()))
                })?;
        }
    }
    fsync_dir(root)
}
