use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelDiskRef {
    pub sysfs_path: PathBuf,
    pub major_minor: String,
    pub kernel_name: String,
    pub dev_path: PathBuf,
    pub logical_block_size: u64,
    pub byte_size: u64,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub stable_disk_id: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateStatus {
    Selectable,
    InstallerMedia,
    InstalledTarget,
    BlockedByInstalledTarget,
    TooSmall,
    UnsupportedSectorSize,
    NoStableId,
    ReadError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingPartitionTable {
    Gpt,
    Mbr,
    None,
    Unrecognized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDiskCandidate {
    pub disk: KernelDiskRef,
    pub status: CandidateStatus,
    pub existing: ExistingPartitionTable,
}
