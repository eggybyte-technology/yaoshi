pub const PRODUCT_NAME: &str = "YaoshiInstallerImage";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const STATE_ROOT: &str = ".yaoshi";
pub const CONFIG_PATH: &str = ".yaoshi/yaoshi.toml";
pub const WORK_ROOT: &str = ".yaoshi/work";
pub const OUTPUT_IMAGE: &str = ".yaoshi/out/yaoshi.img";

pub const SECTOR_SIZE: u64 = 512;
pub const LOGICAL_SECTOR_SIZE: u64 = 512;
pub const REQUIRED_BLOCK_SIZE: u64 = 4096;
pub const LEADING_GAP_BYTES: u64 = 1_048_576;
pub const INSTALLER_BOOT_SIZE_BYTES: u64 = 134_217_728;
pub const INSTALLED_ESP_SIZE_BYTES: u64 = 268_435_456;
pub const TRAILING_GAP_BYTES: u64 = 1_048_576;
pub const INSTALLER_BOOT_START: u64 = LEADING_GAP_BYTES;
pub const INSTALLED_ROOT_START: u64 = LEADING_GAP_BYTES + INSTALLED_ESP_SIZE_BYTES;
pub const INSTALLED_ROOT_MINIMUM_BYTES: u64 = 1_073_741_824;
pub const INSTALLED_ROOT_SAFETY_MINIMUM_BYTES: u64 = 536_870_912;
pub const EXT4_BLOCK_SIZE_BYTES: u64 = 4096;
pub const EXT4_INODE_SIZE_BYTES: u64 = 256;
pub const EXT4_INODE_RATIO_BYTES: u64 = 8192;
pub const EXT4_JOURNAL_SIZE_BYTES: u64 = 67_108_864;
pub const SOURCE_DATE_EPOCH: u64 = 1_704_067_200;
pub const MAX_INSTALLER_IMAGE_BYTES: u64 = 131_072 * 1_048_576;
pub const PAYLOAD_EXTENT_MAX_UNCOMPRESSED_BYTES: u64 = 67_108_864;
pub const PAYLOAD_CONTAINER_ALIGNMENT_BYTES: u64 = 512;
pub const TARGET_HEAD_SCRUB_BYTES: u64 = 16_777_216;
pub const TARGET_TAIL_SCRUB_BYTES: u64 = 16_777_216;
pub const INITRAMFS_ZSTD_COMPRESSION_LEVEL: i32 = 3;
pub const PAYLOAD_ZSTD_COMPRESSION_LEVEL: i32 = 1;
pub const ZSTD_COMPRESSION_LEVEL: i32 = INITRAMFS_ZSTD_COMPRESSION_LEVEL;
pub const ZSTD_WINDOW_LOG: u32 = 22;
pub const ZSTD_WINDOW_BYTES: u64 = 4_194_304;

pub const INSTALLER_BOOT_LABEL: &str = "YAOSHI_BOOT";
pub const INSTALLED_ESP_LABEL: &str = "YAOSHI_ESP";
pub const INSTALLED_ROOT_LABEL: &str = "YAOSHI_ROOT";
pub const INSTALLED_ESP_NAME: &str = "YAOSHI_ESP";
pub const INSTALLED_ROOT_NAME: &str = "YAOSHI_ROOT";

pub const EFI_BOOT_PATH: &str = "EFI/BOOT/BOOTX64.EFI";
pub const INSTALLED_KERNEL_PATH: &str = "YAOSHI/BOOT/VMLINUZ";
pub const INSTALLED_INITRD_PATH: &str = "YAOSHI/BOOT/INITRD.IMG";
pub const INSTALLED_KERNEL_RELEASE_PATH: &str = "YAOSHI/BOOT/KERNEL-RELEASE";
pub const INSTALLER_KERNEL_PATH: &str = "YAOSHI/BOOT/VMLINUZ";
pub const INSTALLER_KERNEL_RELEASE_PATH: &str = "YAOSHI/BOOT/KERNEL-RELEASE";
pub const INSTALLER_BASE_INITRAMFS_NEWC: &str = "BASE.CPIO";
pub const INSTALLER_BASE_INITRAMFS_ZSTD: &str = "INSTALLER-BASE.CPIO.ZST";
pub const INSTALLER_BASE_INITRAMFS_FAT_PATH: &str = "YAOSHI/INSTALLER-BASE.CPIO.ZST";
pub const INSTALLER_APP_INITRAMFS_NEWC: &str = "APP.CPIO";
pub const INSTALLER_APP_INITRAMFS_ZSTD: &str = "INSTALLER-APP.CPIO.ZST";
pub const INSTALLER_APP_INITRAMFS_FAT_PATH: &str = "YAOSHI/INSTALLER-APP.CPIO.ZST";
pub const PAYLOAD_MAGIC: &[u8; 16] = b"YAOSHI_PAYLOAD1\0";
pub const ESP_TYPE_GUID: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
pub const X86_64_ROOT_TYPE_GUID: &str = "4f68bce3-e8cd-4db1-96e7-fbcaf984b709";
pub const INSTALLED_DISK_GUID: &str = "f67a39da-9f7a-4b92-a558-7127212cf1f4";
pub const INSTALLED_ESP_PARTITION_GUID: &str = "c6b0b0ea-d3c2-4e84-90da-9514010d9f34";
pub const INSTALLED_ROOT_PARTITION_GUID: &str = "5c5c9f71-bc23-4f8d-80b2-6d4bb64a0f33";
pub const INSTALLED_ROOT_EXT4_UUID: &str = "4d2cfe3a-96cb-43f4-b3b2-2b519d3c0f01";

pub const INSTALLER_LOADER_ENTRY: &str = "title Yaoshi Installer\nlinux /YAOSHI/BOOT/VMLINUZ\ninitrd /YAOSHI/INSTALLER-BASE.CPIO.ZST\ninitrd /YAOSHI/INSTALLER-APP.CPIO.ZST\noptions quiet loglevel=3 yaoshi.mode=installer psi=1 console=ttyS0,115200n8 consoleblank=0 rdinit=/init\n";
pub const INSTALLED_LOADER_ENTRY: &str = "title Yaoshi Debian trixie\nlinux /YAOSHI/BOOT/VMLINUZ\ninitrd /YAOSHI/BOOT/INITRD.IMG\noptions quiet loglevel=3 root=PARTUUID=5c5c9f71-bc23-4f8d-80b2-6d4bb64a0f33 rootwait rw rootfstype=ext4 systemd.unit=multi-user.target psi=1 console=ttyS0,115200n8 console=tty1 consoleblank=0\n";
pub const LOADER_CONF: &str = "default yaoshi.conf\ntimeout 0\neditor no\n";
pub const INSTALLER_LOADER_CONF: &str = "default yaoshi-installer.conf\ntimeout 0\neditor no\n";
pub const INSTALLED_CMDLINE_PREFIX: &str = "quiet loglevel=3 root=PARTUUID=";
pub const INSTALLED_CMDLINE_SUFFIX: &str = " rootwait rw rootfstype=ext4 systemd.unit=multi-user.target psi=1 console=ttyS0,115200n8 console=tty1 consoleblank=0";

pub const DEBIAN_SUITE: &str = "trixie";
pub const DEBIAN_SECURITY_SUITE: &str = "trixie-security";
pub const DEBIAN_ARCH: &str = "amd64";
pub const DEBIAN_COMPONENTS: &str = "main,non-free-firmware";
pub const DEBIAN_PACKAGES: &[&str] = &[
    "systemd",
    "systemd-sysv",
    "udev",
    "ca-certificates",
    "debian-archive-keyring",
    "bash",
    "systemd-repart",
    "systemd-resolved",
    "openssh-server",
    "e2fsprogs",
    "linux-image-amd64",
    "initramfs-tools",
    "systemd-boot-efi-amd64-signed",
];

pub const REQUIRED_COMMANDS: &[&str] = &["cargo"];
