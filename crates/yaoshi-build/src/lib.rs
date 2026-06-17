use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;
use yaoshi_common::*;

mod runtime_binary;
mod templates;

pub const CURRENT_IMAGE_STAMP_PATH: &str = ".yaoshi/out/yaoshi.img.current";

const DEFAULT_DEBIAN_MIRROR: &str = "https://mirrors.aliyun.com/debian";
const DEFAULT_DEBIAN_SECURITY_MIRROR: &str = "https://mirrors.aliyun.com/debian-security";
const BUILD_SYSTEM_ASSETS_PATH: &str = ".yaoshi/vendor";
const DEBIAN_PACKAGE_ROOT_GRAMMAR: &str = "yaoshi.debian-package-root.v1";
const CUSTOMIZED_ROOT_GRAMMAR: &str = "yaoshi.customized-root.v1";
const ROOT_BRIDGE_OVERLAY_GRAMMAR: &str = "yaoshi.root-bridge-overlay.v1";
const INSTALLED_ROOT_SOURCE_GRAMMAR: &str = "yaoshi.installed-root-source.v1";
const INSTALLED_ROOT_EXT4_GRAMMAR: &str = "yaoshi.installed-root-ext4.v1";
const RUNTIME_BINARY_GRAMMAR: &str = "yaoshi.runtime-binary-musl.v1";
const FAT32_GRAMMAR: &str = "yaoshi.fat32.v1";
const PAYLOAD_GRAMMAR: &str = "yaoshi.payload.v1";
const VIRTUAL_TARGET_GRAPH_GRAMMAR: &str = "yaoshi.virtual-installed-target-graph.v1";
const FINAL_INSTALLER_GRAMMAR: &str = "yaoshi.final-installer-mbr.v1";
const COMPOSITE_FILE_GRAMMAR: &str = "yaoshi.composite-file.v1";
const INSTALLED_ESP_REQUIRED_FILE_SET: &[&str] = &[
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
];

const DEFAULT_INSTALLED_PACKAGES: &[&str] = &[
    "curl",
    "git",
    "sudo",
    "less",
    "nano",
    "iproute2",
    "procps",
    "psmisc",
    "iputils-ping",
    "nftables",
    "openssh-client",
    "rsync",
];

const BUILD_TIME_IDENTITY_CLEANUP_RULE_VERSION: &str = "1";
const FIRST_BOOT_RUNTIME_GRAMMAR_VERSION: &str = "yaoshi.first-boot-runtime.v1";

const MMDEBSTRAP_APT_RETRIES: &str = r#"Acquire::Retries "3""#;
const MMDEBSTRAP_APT_HTTP_TIMEOUT: &str = r#"Acquire::http::Timeout "30""#;
const MMDEBSTRAP_APT_HTTPS_TIMEOUT: &str = r#"Acquire::https::Timeout "30""#;
const MMDEBSTRAP_APT_NO_RECOMMENDS: &str = r#"Apt::Install-Recommends "false""#;
const MMDEBSTRAP_APT_NO_SUGGESTS: &str = r#"Apt::Install-Suggests "false""#;

include!("pipeline/api.rs");
include!("pipeline/config.rs");
include!("pipeline/runtime_root.rs");
include!("pipeline/artifacts.rs");
include!("pipeline/cache.rs");
include!("pipeline/helpers.rs");
include!("pipeline/tests.rs");
