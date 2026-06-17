use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

use crc32fast::Hasher;
use fatfs::{FileSystem, FormatVolumeOptions, FsOptions};
use fscommon::BufStream;
use object::Object;
use serde_json::json;
use uuid::Uuid;
use yaoshi_common::{
    EFI_BOOT_PATH, ESP_TYPE_GUID, INSTALLED_CMDLINE_PREFIX, INSTALLED_CMDLINE_SUFFIX,
    INSTALLED_ESP_LABEL, INSTALLED_ESP_NAME, INSTALLED_ESP_PARTITION_GUID,
    INSTALLED_ESP_SIZE_BYTES, INSTALLED_INITRD_PATH, INSTALLED_KERNEL_PATH,
    INSTALLED_KERNEL_RELEASE_PATH, INSTALLED_ROOT_LABEL, INSTALLED_ROOT_NAME,
    INSTALLED_ROOT_PARTITION_GUID, INSTALLER_APP_INITRAMFS_FAT_PATH,
    INSTALLER_BASE_INITRAMFS_FAT_PATH, INSTALLER_BOOT_LABEL, INSTALLER_BOOT_SIZE_BYTES,
    INSTALLER_KERNEL_PATH, INSTALLER_KERNEL_RELEASE_PATH, LEADING_GAP_BYTES,
    MAX_INSTALLER_IMAGE_BYTES, PAYLOAD_EXTENT_MAX_UNCOMPRESSED_BYTES, PAYLOAD_MAGIC,
    REQUIRED_BLOCK_SIZE, SECTOR_SIZE, Sha256Hex, TRAILING_GAP_BYTES, X86_64_ROOT_TYPE_GUID,
    YaoshiError, YaoshiResult, canonical_json_bytes,
};
use yaoshi_common::{InstalledGptLayout, InstallerMbrLayout, PartitionLayout};

include!("image/types.rs");
include!("image/mbr_io.rs");
include!("image/gpt.rs");
include!("image/fat32.rs");
include!("image/validation.rs");
include!("image/tests.rs");
