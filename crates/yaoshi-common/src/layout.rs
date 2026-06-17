use uuid::Uuid;

use crate::constants::{
    INSTALLED_DISK_GUID, INSTALLED_ESP_PARTITION_GUID, INSTALLED_ESP_SIZE_BYTES,
    INSTALLED_ROOT_PARTITION_GUID, INSTALLED_ROOT_START, INSTALLER_BOOT_SIZE_BYTES,
    LEADING_GAP_BYTES, SECTOR_SIZE, TRAILING_GAP_BYTES,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionLayout {
    pub number: u8,
    pub start_byte: u64,
    pub byte_size: u64,
}

impl PartitionLayout {
    pub fn start_lba(&self) -> u64 {
        self.start_byte / SECTOR_SIZE
    }

    pub fn sector_count(&self) -> u64 {
        self.byte_size / SECTOR_SIZE
    }
}

#[derive(Debug, Clone)]
pub struct InstalledGptLayout {
    pub disk_guid: Uuid,
    pub esp_guid: Uuid,
    pub root_guid: Uuid,
    pub root_bytes: u64,
}

impl InstalledGptLayout {
    pub fn new(root_bytes: u64) -> Self {
        Self::fixed(root_bytes)
    }

    pub fn fixed(root_bytes: u64) -> Self {
        Self::with_guids(
            root_bytes,
            Uuid::parse_str(INSTALLED_DISK_GUID).expect("fixed installed disk GUID parses"),
            Uuid::parse_str(INSTALLED_ESP_PARTITION_GUID)
                .expect("fixed installed ESP partition GUID parses"),
            Uuid::parse_str(INSTALLED_ROOT_PARTITION_GUID)
                .expect("fixed installed root partition GUID parses"),
        )
    }

    pub fn with_guids(root_bytes: u64, disk_guid: Uuid, esp_guid: Uuid, root_guid: Uuid) -> Self {
        Self {
            disk_guid,
            esp_guid,
            root_guid,
            root_bytes,
        }
    }

    pub fn image_bytes(&self) -> u64 {
        LEADING_GAP_BYTES + INSTALLED_ESP_SIZE_BYTES + self.root_bytes + TRAILING_GAP_BYTES
    }

    pub fn esp_partition(&self) -> PartitionLayout {
        PartitionLayout {
            number: 1,
            start_byte: LEADING_GAP_BYTES,
            byte_size: INSTALLED_ESP_SIZE_BYTES,
        }
    }

    pub fn root_partition(&self) -> PartitionLayout {
        PartitionLayout {
            number: 2,
            start_byte: INSTALLED_ROOT_START,
            byte_size: self.root_bytes,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallerMbrLayout {
    pub installed_system_bytes: u64,
}

impl InstallerMbrLayout {
    pub fn image_bytes(&self) -> u64 {
        LEADING_GAP_BYTES
            + INSTALLER_BOOT_SIZE_BYTES
            + self.installed_system_bytes
            + TRAILING_GAP_BYTES
    }

    pub fn boot_partition(&self) -> PartitionLayout {
        PartitionLayout {
            number: 1,
            start_byte: LEADING_GAP_BYTES,
            byte_size: INSTALLER_BOOT_SIZE_BYTES,
        }
    }

    pub fn payload_partition(&self) -> PartitionLayout {
        PartitionLayout {
            number: 2,
            start_byte: LEADING_GAP_BYTES + INSTALLER_BOOT_SIZE_BYTES,
            byte_size: self.installed_system_bytes,
        }
    }
}
