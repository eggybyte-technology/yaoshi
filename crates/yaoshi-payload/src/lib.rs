use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use yaoshi_common::{
    INSTALLED_ESP_LABEL, INSTALLED_ESP_NAME, INSTALLED_ROOT_LABEL, INSTALLED_ROOT_NAME,
    PAYLOAD_CONTAINER_ALIGNMENT_BYTES, PAYLOAD_EXTENT_MAX_UNCOMPRESSED_BYTES, PAYLOAD_MAGIC,
    PAYLOAD_ZSTD_COMPRESSION_LEVEL, REQUIRED_BLOCK_SIZE, SECTOR_SIZE, VERSION, YaoshiError,
    YaoshiResult, round_up,
};

pub const PRELUDE_LEN: usize = 512;
pub const EXTENT_ENTRY_SIZE: usize = 128;
pub const FORMAT_VERSION: u32 = 1;
pub const COMPRESSION_ZSTD: u32 = 1;
pub const KIND_RAW: u8 = 0;
pub const KIND_ZSTD: u8 = 1;
pub const KIND_ZERO: u8 = 2;
pub const ZSTD_LEVEL: i32 = PAYLOAD_ZSTD_COMPRESSION_LEVEL;
pub const ZSTD_WINDOW_LOG: u32 = yaoshi_common::ZSTD_WINDOW_LOG;

include!("payload/build.rs");
include!("payload/validate.rs");
include!("payload/write.rs");
include!("payload/tests.rs");
