#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MbrEntry {
    pub number: u8,
    pub mbr_type: u8,
    pub start_lba: u32,
    pub sector_count: u32,
}

pub type MbrPartition = MbrEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallerMbr {
    pub boot_partition: MbrEntry,
    pub payload_partition: MbrEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptPartition {
    pub number: u32,
    pub type_guid: Uuid,
    pub unique_guid: Uuid,
    pub start_lba: u64,
    pub end_lba: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptHeader {
    pub current_lba: u64,
    pub alternate_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: Uuid,
    pub partition_entries_lba: u64,
    pub partition_entry_count: u32,
    pub partition_entry_size: u32,
    pub partition_entries_crc32: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptDisk {
    pub disk_guid: Uuid,
    pub primary_header: GptHeader,
    pub backup_header: GptHeader,
    pub primary_partition_entries: Vec<u8>,
    pub backup_partition_entries: Vec<u8>,
    pub accepted_image_span: u64,
    pub partitions: Vec<GptPartition>,
}

#[derive(Debug, Clone)]
pub struct TreeFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetWriteExtentKind {
    Data,
    Zero,
}

impl TargetWriteExtentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Zero => "zero",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetWriteExtent {
    pub logical_offset: u64,
    pub len: u64,
    pub kind: TargetWriteExtentKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetWritePlan {
    pub target_image_bytes: u64,
    pub block_size: u64,
    pub extents: Vec<TargetWriteExtent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptRequiredExtent {
    pub name: &'static str,
    pub target_logical_offset: u64,
    pub length: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptRequiredExtentBytes {
    pub name: &'static str,
    pub target_logical_offset: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct VirtualInstalledTargetGraph {
    layout: InstalledGptLayout,
    installed_esp_fat32: PathBuf,
    installed_root_ext4: PathBuf,
    gpt_extents: Vec<GptRequiredExtentBytes>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualTargetExtent {
    pub logical_offset: u64,
    pub bytes: Vec<u8>,
    pub kind: TargetWriteExtentKind,
}

impl VirtualInstalledTargetGraph {
    pub fn new(
        layout: InstalledGptLayout,
        installed_esp_fat32: impl Into<PathBuf>,
        installed_root_ext4: impl Into<PathBuf>,
    ) -> YaoshiResult<Self> {
        let installed_esp_fat32 = installed_esp_fat32.into();
        let installed_root_ext4 = installed_root_ext4.into();
        let esp_len = fs::metadata(&installed_esp_fat32)
            .map_err(|e| YaoshiError::image(format!("stat installed ESP FAT32: {e}")))?
            .len();
        let root_len = fs::metadata(&installed_root_ext4)
            .map_err(|e| YaoshiError::image(format!("stat installed root ext4: {e}")))?
            .len();
        if esp_len != layout.esp_partition().byte_size {
            return Err(YaoshiError::image(
                "installed ESP FAT32 size does not match target graph layout",
            ));
        }
        if root_len != layout.root_partition().byte_size {
            return Err(YaoshiError::image(
                "installed root ext4 size does not match target graph layout",
            ));
        }
        Ok(Self {
            gpt_extents: gpt_required_extent_bytes(&layout)?,
            layout,
            installed_esp_fat32,
            installed_root_ext4,
        })
    }

    pub fn layout(&self) -> &InstalledGptLayout {
        &self.layout
    }

    pub fn target_image_bytes(&self) -> u64 {
        self.layout.image_bytes()
    }

    pub fn for_each_extent(
        &self,
        mut f: impl FnMut(VirtualTargetExtent) -> YaoshiResult<()>,
    ) -> YaoshiResult<()> {
        let mut esp = File::open(&self.installed_esp_fat32)
            .map_err(|e| YaoshiError::image(format!("open installed ESP FAT32: {e}")))?;
        let mut root = File::open(&self.installed_root_ext4)
            .map_err(|e| YaoshiError::image(format!("open installed root ext4: {e}")))?;
        let mut current: Option<VirtualTargetExtent> = None;
        let mut offset = 0u64;
        while offset < self.layout.image_bytes() {
            let bytes = self.block_bytes_at(offset, &mut esp, &mut root)?;
            let kind = if bytes.iter().all(|b| *b == 0) {
                TargetWriteExtentKind::Zero
            } else {
                TargetWriteExtentKind::Data
            };
            if let Some(extent) = current.as_mut()
                && extent.kind == kind
                && extent.logical_offset + extent.bytes.len() as u64 == offset
                && extent.bytes.len() as u64 + REQUIRED_BLOCK_SIZE
                    <= PAYLOAD_EXTENT_MAX_UNCOMPRESSED_BYTES
            {
                extent.bytes.extend_from_slice(&bytes);
            } else {
                if let Some(extent) = current.take() {
                    f(extent)?;
                }
                current = Some(VirtualTargetExtent {
                    logical_offset: offset,
                    bytes,
                    kind,
                });
            }
            offset += REQUIRED_BLOCK_SIZE;
        }
        if let Some(extent) = current {
            f(extent)?;
        }
        Ok(())
    }

    fn block_bytes_at(
        &self,
        offset: u64,
        esp: &mut File,
        root: &mut File,
    ) -> YaoshiResult<Vec<u8>> {
        let mut block = vec![0u8; REQUIRED_BLOCK_SIZE as usize];
        let block_end = offset + REQUIRED_BLOCK_SIZE;
        for extent in &self.gpt_extents {
            overlay_range(
                &mut block,
                offset,
                block_end,
                extent.target_logical_offset,
                &extent.bytes,
            )?;
        }
        let esp_part = self.layout.esp_partition();
        if ranges_overlap(
            offset,
            block_end,
            esp_part.start_byte,
            esp_part.start_byte + esp_part.byte_size,
        ) {
            overlay_file_range(&mut block, offset, block_end, esp_part.start_byte, esp)?;
        }
        let root_part = self.layout.root_partition();
        if ranges_overlap(
            offset,
            block_end,
            root_part.start_byte,
            root_part.start_byte + root_part.byte_size,
        ) {
            overlay_file_range(&mut block, offset, block_end, root_part.start_byte, root)?;
        }
        Ok(block)
    }
}

pub struct BoundedRegion<R> {
    inner: R,
    start: u64,
    len: u64,
    pos: u64,
}

impl<R: Read + Seek> BoundedRegion<R> {
    pub fn new(inner: R, start: u64, len: u64) -> Self {
        Self {
            inner,
            start,
            len,
            pos: 0,
        }
    }
}

impl<R: Read + Seek> Read for BoundedRegion<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        let remaining = (self.len - self.pos) as usize;
        let limit = remaining.min(buf.len());
        self.inner.seek(SeekFrom::Start(self.start + self.pos))?;
        let read = self.inner.read(&mut buf[..limit])?;
        self.pos += read as u64;
        Ok(read)
    }
}

impl<R: Read + Seek> Seek for BoundedRegion<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let next = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::End(offset) => if offset >= 0 {
                self.len.checked_add(offset as u64)
            } else {
                self.len.checked_sub(offset.unsigned_abs())
            }
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "bounded seek overflow")
            })?,
            SeekFrom::Current(offset) => if offset >= 0 {
                self.pos.checked_add(offset as u64)
            } else {
                self.pos.checked_sub(offset.unsigned_abs())
            }
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "bounded seek overflow")
            })?,
        };
        if next > self.len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "bounded seek outside region",
            ));
        }
        self.pos = next;
        Ok(self.pos)
    }
}

impl<R: Read + Seek> Write for BoundedRegion<R> {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "bounded region is read-only",
        ))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
