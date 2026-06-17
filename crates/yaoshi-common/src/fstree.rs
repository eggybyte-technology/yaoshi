use std::collections::BTreeMap;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{Sha256Hex, YaoshiError, YaoshiResult, canonical_json_bytes};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FsTreeNodeKind {
    Directory,
    Regular { content_sha256: Sha256Hex },
    Symlink { target: Vec<u8> },
    CharacterDevice { major: u32, minor: u32 },
    BlockDevice { major: u32, minor: u32 },
    Fifo,
    Socket,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsTreeNode {
    pub path: String,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub xattrs: BTreeMap<String, Vec<u8>>,
    #[serde(flatten)]
    pub kind: FsTreeNodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsTreeManifest {
    pub grammar: String,
    pub nodes: Vec<FsTreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFsTree {
    pub manifest: FsTreeManifest,
    pub file_objects: BTreeMap<Sha256Hex, Vec<u8>>,
}

impl FsTreeManifest {
    pub fn new(nodes: Vec<FsTreeNode>) -> YaoshiResult<Self> {
        validate_nodes(&nodes)?;
        Ok(Self {
            grammar: "yaoshi.fs-tree.v1".to_string(),
            nodes,
        })
    }

    pub fn canonical_json(&self) -> YaoshiResult<Vec<u8>> {
        let value = serde_json::to_value(self).map_err(|e| YaoshiError::internal(e.to_string()))?;
        canonical_json_bytes(&value)
    }

    pub fn digest(&self) -> YaoshiResult<Sha256Hex> {
        Ok(Sha256Hex::digest_bytes(&self.canonical_json()?))
    }
}

pub fn read_host_tree(root: &Path) -> YaoshiResult<HostFsTree> {
    let root_meta = fs::symlink_metadata(root)
        .map_err(|e| YaoshiError::build(format!("stat fs-tree root: {e}")))?;
    if !root_meta.is_dir() {
        return Err(YaoshiError::build("fs-tree root must be a directory"));
    }
    let mut nodes = Vec::new();
    let mut file_objects = BTreeMap::new();
    collect_host_tree(root, root, "/", &mut nodes, &mut file_objects)?;
    nodes.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    Ok(HostFsTree {
        manifest: FsTreeManifest::new(nodes)?,
        file_objects,
    })
}

fn collect_host_tree(
    root: &Path,
    current: &Path,
    product_path: &str,
    nodes: &mut Vec<FsTreeNode>,
    file_objects: &mut BTreeMap<Sha256Hex, Vec<u8>>,
) -> YaoshiResult<()> {
    let meta = fs::symlink_metadata(current)
        .map_err(|e| YaoshiError::build(format!("stat fs-tree node {}: {e}", current.display())))?;
    let file_type = meta.file_type();
    let mode = meta.mode() & 0o7777;
    let uid = meta.uid();
    let gid = meta.gid();
    let kind = if file_type.is_dir() {
        FsTreeNodeKind::Directory
    } else if file_type.is_file() {
        let bytes = fs::read(current).map_err(|e| {
            YaoshiError::build(format!("read fs-tree file {}: {e}", current.display()))
        })?;
        let digest = Sha256Hex::digest_bytes(&bytes);
        file_objects.entry(digest.clone()).or_insert(bytes);
        FsTreeNodeKind::Regular {
            content_sha256: digest,
        }
    } else if file_type.is_symlink() {
        let target = fs::read_link(current).map_err(|e| {
            YaoshiError::build(format!("read fs-tree symlink {}: {e}", current.display()))
        })?;
        FsTreeNodeKind::Symlink {
            target: target.as_os_str().as_bytes().to_vec(),
        }
    } else if file_type.is_char_device() {
        let (major, minor) = linux_device_major_minor(meta.rdev());
        FsTreeNodeKind::CharacterDevice { major, minor }
    } else if file_type.is_block_device() {
        let (major, minor) = linux_device_major_minor(meta.rdev());
        FsTreeNodeKind::BlockDevice { major, minor }
    } else if file_type.is_fifo() {
        FsTreeNodeKind::Fifo
    } else if file_type.is_socket() {
        FsTreeNodeKind::Socket
    } else {
        return Err(YaoshiError::build("unsupported fs-tree node type"));
    };
    nodes.push(FsTreeNode {
        path: product_path.to_string(),
        mode,
        uid,
        gid,
        xattrs: BTreeMap::new(),
        kind,
    });
    if file_type.is_dir() {
        let mut entries = Vec::new();
        for entry in fs::read_dir(current).map_err(|e| {
            YaoshiError::build(format!("read fs-tree directory {}: {e}", current.display()))
        })? {
            let entry =
                entry.map_err(|e| YaoshiError::build(format!("read fs-tree entry: {e}")))?;
            entries.push(entry.path());
        }
        entries.sort_by(|a, b| a.as_os_str().as_bytes().cmp(b.as_os_str().as_bytes()));
        for child in entries {
            let rel = child
                .strip_prefix(root)
                .map_err(|e| YaoshiError::internal(format!("strip fs-tree root prefix: {e}")))?;
            let rel_bytes = rel.as_os_str().as_bytes();
            if !rel_bytes.is_ascii() {
                return Err(YaoshiError::build("fs-tree path is not ASCII"));
            }
            let rel_string = rel.to_string_lossy().replace('\\', "/");
            let child_product_path = format!("/{rel_string}");
            validate_absolute_product_path(&child_product_path)?;
            collect_host_tree(root, &child, &child_product_path, nodes, file_objects)?;
        }
    }
    Ok(())
}

fn linux_device_major_minor(rdev: u64) -> (u32, u32) {
    let major = ((rdev >> 8) & 0x0fff) | ((rdev >> 32) & !0x0fff);
    let minor = (rdev & 0x00ff) | ((rdev >> 12) & !0x00ff);
    (major as u32, minor as u32)
}

fn validate_nodes(nodes: &[FsTreeNode]) -> YaoshiResult<()> {
    let mut last = None::<&str>;
    for node in nodes {
        validate_absolute_product_path(&node.path)?;
        if let Some(prev) = last
            && prev.as_bytes() >= node.path.as_bytes()
        {
            return Err(YaoshiError::build(
                "fs-tree nodes must be sorted by unique ascending path bytes",
            ));
        }
        last = Some(&node.path);
    }
    Ok(())
}

pub fn validate_absolute_product_path(path: &str) -> YaoshiResult<()> {
    if !path.starts_with('/') || path.len() > 1 && path.ends_with('/') {
        return Err(YaoshiError::build("invalid fs-tree absolute path"));
    }
    if !path.is_ascii()
        || path
            .split('/')
            .any(|component| component == "." || component == "..")
    {
        return Err(YaoshiError::build(
            "fs-tree path contains non-ASCII or dot component",
        ));
    }
    if path.len() > 1 && path.split('/').skip(1).any(str::is_empty) {
        return Err(YaoshiError::build("fs-tree path contains empty component"));
    }
    Ok(())
}

pub fn fs_tree_storage_manifest_value(
    manifest: &FsTreeManifest,
) -> YaoshiResult<serde_json::Value> {
    let digest = manifest.digest()?;
    Ok(json!({
        "digest": digest.as_str(),
        "grammar": manifest.grammar,
        "node_count": manifest.nodes.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "yaoshi-common-fstree-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn rejects_unsorted_or_relative_paths() {
        let node = FsTreeNode {
            path: "etc/hostname".to_string(),
            mode: 0o100644,
            uid: 0,
            gid: 0,
            xattrs: BTreeMap::new(),
            kind: FsTreeNodeKind::Regular {
                content_sha256: Sha256Hex::digest_bytes(b"yaoshi\n"),
            },
        };
        assert!(FsTreeManifest::new(vec![node]).is_err());
    }

    #[test]
    fn manifest_digest_uses_canonical_metadata() {
        let manifest = FsTreeManifest::new(vec![FsTreeNode {
            path: "/etc/hostname".to_string(),
            mode: 0o100644,
            uid: 0,
            gid: 0,
            xattrs: BTreeMap::new(),
            kind: FsTreeNodeKind::Regular {
                content_sha256: Sha256Hex::digest_bytes(b"yaoshi\n"),
            },
        }])
        .unwrap();
        assert_eq!(manifest.digest().unwrap().as_str().len(), 64);
    }

    #[test]
    fn reads_host_tree_with_content_objects_and_symlinks() {
        let root = temp_root("read-host-tree");
        fs::create_dir_all(root.join("etc")).unwrap();
        fs::write(root.join("etc/hostname"), b"yaoshi\n").unwrap();
        fs::write(root.join("etc/hostname-copy"), b"yaoshi\n").unwrap();
        symlink("/etc/hostname", root.join("host-link")).unwrap();

        let tree = read_host_tree(&root).unwrap();
        let paths = tree
            .manifest
            .nodes
            .iter()
            .map(|node| node.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "/",
                "/etc",
                "/etc/hostname",
                "/etc/hostname-copy",
                "/host-link"
            ]
        );
        assert_eq!(tree.file_objects.len(), 1);
        assert!(tree.manifest.nodes.iter().any(|node| matches!(
            &node.kind,
            FsTreeNodeKind::Symlink { target } if target == b"/etc/hostname"
        )));

        fs::remove_dir_all(root).unwrap();
    }
}
