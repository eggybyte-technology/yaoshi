use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use yaoshi_common::{YaoshiError, YaoshiResult};

#[derive(Debug, Clone)]
pub enum EntryKind {
    Directory,
    Regular { source: PathBuf },
    CharacterDevice { major: u32, minor: u32 },
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    pub mode: u32,
    pub kind: EntryKind,
}

pub fn render_installer_base_tree(tree: &Path, module_closure: &Path) -> YaoshiResult<Vec<Entry>> {
    if tree.exists() {
        fs::remove_dir_all(tree)
            .map_err(|e| YaoshiError::build(format!("remove initramfs tree: {e}")))?;
    }
    for dir in ["dev", "proc", "run", "sys", "lib", "lib/modules"] {
        fs::create_dir_all(tree.join(dir))
            .map_err(|e| YaoshiError::build(format!("create initramfs dir {dir}: {e}")))?;
        fs::set_permissions(tree.join(dir), fs::Permissions::from_mode(0o755))
            .map_err(|e| YaoshiError::build(format!("chmod initramfs dir {dir}: {e}")))?;
    }
    let mut entries = vec![
        Entry {
            path: "dev/".to_string(),
            mode: 0o040755,
            kind: EntryKind::Directory,
        },
        Entry {
            path: "dev/console".to_string(),
            mode: 0o020600,
            kind: EntryKind::CharacterDevice { major: 5, minor: 1 },
        },
        Entry {
            path: "lib/".to_string(),
            mode: 0o040755,
            kind: EntryKind::Directory,
        },
        Entry {
            path: "lib/modules/".to_string(),
            mode: 0o040755,
            kind: EntryKind::Directory,
        },
        Entry {
            path: "proc/".to_string(),
            mode: 0o040755,
            kind: EntryKind::Directory,
        },
        Entry {
            path: "run/".to_string(),
            mode: 0o040755,
            kind: EntryKind::Directory,
        },
        Entry {
            path: "sys/".to_string(),
            mode: 0o040755,
            kind: EntryKind::Directory,
        },
    ];
    copy_module_closure(module_closure, tree, &mut entries)?;
    entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    Ok(entries)
}

pub fn render_installer_app_tree(tree: &Path, installer_binary: &Path) -> YaoshiResult<Vec<Entry>> {
    if tree.exists() {
        fs::remove_dir_all(tree)
            .map_err(|e| YaoshiError::build(format!("remove initramfs app tree: {e}")))?;
    }
    fs::create_dir_all(tree)
        .map_err(|e| YaoshiError::build(format!("create initramfs app tree: {e}")))?;
    fs::copy(installer_binary, tree.join("init"))
        .map_err(|e| YaoshiError::build(format!("copy installer /init: {e}")))?;
    fs::set_permissions(tree.join("init"), fs::Permissions::from_mode(0o755))
        .map_err(|e| YaoshiError::build(format!("chmod installer /init: {e}")))?;
    Ok(vec![Entry {
        path: "init".to_string(),
        mode: 0o100755,
        kind: EntryKind::Regular {
            source: tree.join("init"),
        },
    }])
}

fn copy_module_closure(
    module_closure: &Path,
    tree: &Path,
    entries: &mut Vec<Entry>,
) -> YaoshiResult<()> {
    let modules_list = module_closure.join("YAOSHI-MODULES");
    if !modules_list.is_file() {
        return Err(YaoshiError::build(
            "installer module closure missing YAOSHI-MODULES",
        ));
    }
    fs::copy(&modules_list, tree.join("YAOSHI-MODULES"))
        .map_err(|e| YaoshiError::build(format!("copy YAOSHI-MODULES: {e}")))?;
    entries.push(Entry {
        path: "YAOSHI-MODULES".to_string(),
        mode: 0o100644,
        kind: EntryKind::Regular {
            source: tree.join("YAOSHI-MODULES"),
        },
    });
    let lib_modules = module_closure.join("lib/modules");
    if lib_modules.exists() {
        copy_module_tree(module_closure, &lib_modules, tree, entries)?;
    }
    Ok(())
}

fn copy_module_tree(
    module_closure: &Path,
    current: &Path,
    tree: &Path,
    entries: &mut Vec<Entry>,
) -> YaoshiResult<()> {
    for entry in fs::read_dir(current)
        .map_err(|e| YaoshiError::build(format!("read module closure directory: {e}")))?
    {
        let entry = entry.map_err(|e| YaoshiError::build(format!("read module entry: {e}")))?;
        let src = entry.path();
        let rel = src
            .strip_prefix(module_closure)
            .map_err(|e| YaoshiError::internal(format!("strip module closure prefix: {e}")))?;
        let dst = tree.join(rel);
        let file_type = entry
            .file_type()
            .map_err(|e| YaoshiError::build(format!("read module file type: {e}")))?;
        if file_type.is_dir() {
            fs::create_dir_all(&dst)
                .map_err(|e| YaoshiError::build(format!("create initramfs module dir: {e}")))?;
            entries.push(Entry {
                path: format!("{}/", rel.to_string_lossy().replace('\\', "/")),
                mode: 0o040755,
                kind: EntryKind::Directory,
            });
            copy_module_tree(module_closure, &src, tree, entries)?;
        } else if file_type.is_file() {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    YaoshiError::build(format!("create initramfs module parent: {e}"))
                })?;
            }
            fs::copy(&src, &dst)
                .map_err(|e| YaoshiError::build(format!("copy initramfs module: {e}")))?;
            entries.push(Entry {
                path: rel.to_string_lossy().replace('\\', "/"),
                mode: 0o100644,
                kind: EntryKind::Regular { source: dst },
            });
        }
    }
    Ok(())
}

pub fn newc_archive_bytes(entries: &[Entry]) -> YaoshiResult<Vec<u8>> {
    let mut out = Vec::new();
    let mut ino = 1u32;
    for entry in entries {
        let data = match &entry.kind {
            EntryKind::Regular { source } => {
                let mut bytes = Vec::new();
                File::open(source)
                    .and_then(|mut f| f.read_to_end(&mut bytes))
                    .map_err(|e| YaoshiError::build(format!("read initramfs source: {e}")))?;
                bytes
            }
            EntryKind::Directory | EntryKind::CharacterDevice { .. } => Vec::new(),
        };
        let (major, minor) = match entry.kind {
            EntryKind::CharacterDevice { major, minor } => (major, minor),
            _ => (0, 0),
        };
        write_entry(&mut out, entry, ino, major, minor, &data)?;
        ino += 1;
    }
    write_trailer(&mut out, ino)?;
    Ok(out)
}

pub fn write_newc(entries: &[Entry], out: &Path) -> YaoshiResult<()> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::build(format!("create initramfs out dir: {e}")))?;
    }
    let mut file =
        File::create(out).map_err(|e| YaoshiError::build(format!("create initramfs: {e}")))?;
    file.write_all(&newc_archive_bytes(entries)?)
        .map_err(|e| YaoshiError::build(format!("write initramfs: {e}")))?;
    Ok(())
}

pub fn compress_zstd_bytes(input: &[u8]) -> YaoshiResult<Vec<u8>> {
    zstd::bulk::compress(input, yaoshi_common::ZSTD_COMPRESSION_LEVEL)
        .map_err(|e| YaoshiError::build(format!("compress installer initramfs zstd: {e}")))
}

pub fn compress_zstd_file(input: &Path, out: &Path) -> YaoshiResult<()> {
    let bytes = fs::read(input)
        .map_err(|e| YaoshiError::build(format!("read installer initramfs newc: {e}")))?;
    let compressed = compress_zstd_bytes(&bytes)?;
    if !compressed.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        return Err(YaoshiError::build(
            "compressed initramfs zstd magic mismatch",
        ));
    }
    let decoded = zstd::bulk::decompress(&compressed, bytes.len())
        .map_err(|e| YaoshiError::build(format!("verify installer initramfs zstd: {e}")))?;
    if decoded != bytes {
        return Err(YaoshiError::build(
            "compressed initramfs does not decode to newc bytes",
        ));
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| YaoshiError::build(format!("create zstd initramfs parent: {e}")))?;
    }
    fs::write(out, compressed).map_err(|e| YaoshiError::build(format!("write zstd initramfs: {e}")))
}

fn write_entry<W: Write>(
    out: &mut W,
    entry: &Entry,
    ino: u32,
    rdevmajor: u32,
    rdevminor: u32,
    data: &[u8],
) -> YaoshiResult<()> {
    let namesize = entry.path.len() as u32 + 1;
    write!(
        out,
        "070701{ino:08x}{mode:08x}{uid:08x}{gid:08x}{nlink:08x}{mtime:08x}{filesize:08x}{devmajor:08x}{devminor:08x}{rdevmajor:08x}{rdevminor:08x}{namesize:08x}{check:08x}",
        mode = entry.mode,
        uid = 0,
        gid = 0,
        nlink = match entry.kind {
            EntryKind::Directory => 2,
            EntryKind::Regular { .. } | EntryKind::CharacterDevice { .. } => 1,
        },
        mtime = 0,
        filesize = data.len(),
        devmajor = 0,
        devminor = 0,
        check = 0,
    )
    .map_err(|e| YaoshiError::build(format!("write newc header: {e}")))?;
    out.write_all(entry.path.as_bytes())
        .and_then(|_| out.write_all(&[0]))
        .map_err(|e| YaoshiError::build(format!("write newc name: {e}")))?;
    pad4(out, 110 + namesize as usize)?;
    out.write_all(data)
        .map_err(|e| YaoshiError::build(format!("write newc data: {e}")))?;
    pad4(out, data.len())?;
    Ok(())
}

fn write_trailer<W: Write>(out: &mut W, ino: u32) -> YaoshiResult<()> {
    let entry = Entry {
        path: "TRAILER!!!".to_string(),
        mode: 0,
        kind: EntryKind::Regular {
            source: PathBuf::new(),
        },
    };
    write!(
        out,
        "070701{ino:08x}{mode:08x}{uid:08x}{gid:08x}{nlink:08x}{mtime:08x}{filesize:08x}{devmajor:08x}{devminor:08x}{rdevmajor:08x}{rdevminor:08x}{namesize:08x}{check:08x}",
        mode = entry.mode,
        uid = 0,
        gid = 0,
        nlink = 1,
        mtime = 0,
        filesize = 0,
        devmajor = 0,
        devminor = 0,
        rdevmajor = 0,
        rdevminor = 0,
        namesize = entry.path.len() + 1,
        check = 0,
    )
    .map_err(|e| YaoshiError::build(format!("write newc trailer: {e}")))?;
    out.write_all(entry.path.as_bytes())
        .and_then(|_| out.write_all(&[0]))
        .map_err(|e| YaoshiError::build(format!("write newc trailer name: {e}")))?;
    pad4(out, 110 + entry.path.len() + 1)?;
    Ok(())
}

fn pad4<W: Write>(out: &mut W, len: usize) -> YaoshiResult<()> {
    let pad = (4 - (len % 4)) % 4;
    if pad > 0 {
        out.write_all(&vec![0u8; pad])
            .map_err(|e| YaoshiError::build(format!("write newc padding: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct ParsedEntry {
        name: String,
        mode: u32,
        uid: u32,
        gid: u32,
        nlink: u32,
        mtime: u32,
        rdevmajor: u32,
        rdevminor: u32,
        data: Vec<u8>,
    }

    fn parse_hex_u32(bytes: &[u8]) -> u32 {
        u32::from_str_radix(std::str::from_utf8(bytes).unwrap(), 16).unwrap()
    }

    fn align4(offset: usize) -> usize {
        offset + ((4 - (offset % 4)) % 4)
    }

    fn parse_newc(bytes: &[u8]) -> Vec<ParsedEntry> {
        let mut entries = Vec::new();
        let mut offset = 0usize;
        loop {
            assert_eq!(&bytes[offset..offset + 6], b"070701");
            let header = &bytes[offset + 6..offset + 110];
            let mode = parse_hex_u32(&header[8..16]);
            let uid = parse_hex_u32(&header[16..24]);
            let gid = parse_hex_u32(&header[24..32]);
            let nlink = parse_hex_u32(&header[32..40]);
            let mtime = parse_hex_u32(&header[40..48]);
            let filesize = parse_hex_u32(&header[48..56]) as usize;
            let rdevmajor = parse_hex_u32(&header[72..80]);
            let rdevminor = parse_hex_u32(&header[80..88]);
            let namesize = parse_hex_u32(&header[88..96]) as usize;
            offset += 110;
            let name_bytes = &bytes[offset..offset + namesize - 1];
            assert_eq!(bytes[offset + namesize - 1], 0);
            let name = String::from_utf8(name_bytes.to_vec()).unwrap();
            offset = align4(offset + namesize);
            let data = bytes[offset..offset + filesize].to_vec();
            offset = align4(offset + filesize);
            entries.push(ParsedEntry {
                name: name.clone(),
                mode,
                uid,
                gid,
                nlink,
                mtime,
                rdevmajor,
                rdevminor,
                data,
            });
            if name == "TRAILER!!!" {
                break;
            }
        }
        entries
    }

    #[test]
    fn newc_archive_matches_source_model_metadata_and_padding() {
        let dir =
            std::env::temp_dir().join(format!("yaoshi-initramfs-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("yaoshi-installer");
        fs::write(&bin, b"binary").unwrap();
        let modules = dir.join("modules");
        fs::create_dir_all(modules.join("lib/modules/6.12-test/kernel/drivers/block")).unwrap();
        fs::write(
            modules.join("YAOSHI-MODULES"),
            b"lib/modules/6.12-test/kernel/drivers/block/virtio_blk.ko.xz\n",
        )
        .unwrap();
        fs::write(
            modules.join("lib/modules/6.12-test/kernel/drivers/block/virtio_blk.ko.xz"),
            b"module",
        )
        .unwrap();
        let tree = dir.join("tree");
        let entries = render_installer_base_tree(&tree, &modules).unwrap();
        assert!(tree.join("dev").is_dir());
        assert!(!tree.join("dev/console").exists());
        assert!(!tree.join("init").exists());
        let out = dir.join("installer.cpio");
        write_newc(&entries, &out).unwrap();
        let bytes = fs::read(out).unwrap();
        let parsed = parse_newc(&bytes);
        assert_eq!(
            parsed
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            [
                "YAOSHI-MODULES",
                "dev/",
                "dev/console",
                "lib/",
                "lib/modules/",
                "lib/modules/6.12-test/",
                "lib/modules/6.12-test/kernel/",
                "lib/modules/6.12-test/kernel/drivers/",
                "lib/modules/6.12-test/kernel/drivers/block/",
                "lib/modules/6.12-test/kernel/drivers/block/virtio_blk.ko.xz",
                "proc/",
                "run/",
                "sys/",
                "TRAILER!!!"
            ]
        );
        for entry in &parsed {
            assert_eq!(entry.uid, 0);
            assert_eq!(entry.gid, 0);
            assert_eq!(entry.mtime, 0);
        }
        for entry in parsed.iter().filter(|entry| entry.name.ends_with('/')) {
            assert_eq!(entry.nlink, 2);
        }
        let console = parsed
            .iter()
            .find(|entry| entry.name == "dev/console")
            .unwrap();
        assert_eq!(console.mode, 0o020600);
        assert_eq!(console.nlink, 1);
        assert_eq!(console.rdevmajor, 5);
        assert_eq!(console.rdevminor, 1);
        assert!(console.data.is_empty());
        let app_tree = dir.join("app.tree");
        let app_entries = render_installer_app_tree(&app_tree, &bin).unwrap();
        assert_eq!(fs::read(app_tree.join("init")).unwrap(), b"binary");
        let app_bytes = newc_archive_bytes(&app_entries).unwrap();
        let app_parsed = parse_newc(&app_bytes);
        assert_eq!(
            app_parsed
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["init", "TRAILER!!!"]
        );
        let init = app_parsed
            .iter()
            .find(|entry| entry.name == "init")
            .unwrap();
        assert_eq!(init.mode, 0o100755);
        assert_eq!(init.nlink, 1);
        assert_eq!(init.data, b"binary");
        let _ = fs::remove_dir_all(dir);
    }
}
