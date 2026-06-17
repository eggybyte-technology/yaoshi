use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use yaoshi_common::{YaoshiError, YaoshiResult};

#[derive(Debug, Clone)]
pub struct DebianBootArtifacts {
    pub root: PathBuf,
    pub kernel_release: String,
    pub systemd_boot_efi: PathBuf,
    pub vmlinuz: PathBuf,
    pub initrd_img: PathBuf,
    pub config: PathBuf,
    pub modules: PathBuf,
}

impl DebianBootArtifacts {
    pub fn discover(root: &Path) -> YaoshiResult<Self> {
        let release_path = root.join("KERNEL-RELEASE");
        let kernel_release = fs::read_to_string(&release_path)
            .map_err(|e| YaoshiError::build(format!("read Debian kernel release: {e}")))?
            .trim()
            .to_string();
        if kernel_release.is_empty() || !kernel_release.is_ascii() {
            return Err(YaoshiError::build(
                "Debian kernel release is empty or non-ASCII",
            ));
        }
        let artifacts = Self {
            root: root.to_path_buf(),
            kernel_release: kernel_release.clone(),
            systemd_boot_efi: root.join("systemd-bootx64.efi.signed"),
            vmlinuz: root.join("vmlinuz"),
            initrd_img: root.join("initrd.img"),
            config: root.join("config"),
            modules: root.join("modules").join(kernel_release),
        };
        artifacts.validate()?;
        Ok(artifacts)
    }

    pub fn validate(&self) -> YaoshiResult<()> {
        require_mz(&self.vmlinuz, "Debian kernel image")?;
        require_mz(&self.systemd_boot_efi, "Debian systemd-boot fallback EFI")?;
        require_nonempty_file(&self.initrd_img, "Debian initramfs")?;
        let config = fs::read_to_string(&self.config)
            .map_err(|e| YaoshiError::build(format!("read Debian kernel config: {e}")))?;
        if !config.is_ascii() {
            return Err(YaoshiError::build("Debian kernel config is not ASCII"));
        }
        for required in [
            "CONFIG_BLK_DEV_INITRD=y",
            "CONFIG_RD_ZSTD=y",
            "CONFIG_DEVTMPFS=y",
            "CONFIG_MODULES=y",
            "CONFIG_VT=y",
            "CONFIG_VT_CONSOLE=y",
            "CONFIG_SERIAL_8250=y",
            "CONFIG_SERIAL_8250_CONSOLE=y",
        ] {
            if !config.lines().any(|line| line == required) {
                return Err(YaoshiError::build(format!(
                    "Debian kernel config missing {required}"
                )));
            }
        }
        if !self.modules.join("modules.dep").is_file()
            || !self.modules.join("modules.builtin").is_file()
        {
            return Err(YaoshiError::build(
                "Debian module tree missing modules.dep or modules.builtin",
            ));
        }
        Ok(())
    }
}

pub fn sha256_file_hex(path: &Path) -> YaoshiResult<String> {
    let bytes =
        fs::read(path).map_err(|e| YaoshiError::build(format!("read artifact for sha256: {e}")))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    Ok(out)
}

fn require_nonempty_file(path: &Path, name: &str) -> YaoshiResult<()> {
    let meta = fs::metadata(path).map_err(|e| YaoshiError::build(format!("stat {name}: {e}")))?;
    if !meta.is_file() || meta.len() == 0 {
        return Err(YaoshiError::build(format!(
            "{name} is not a non-empty file"
        )));
    }
    Ok(())
}

fn require_mz(path: &Path, name: &str) -> YaoshiResult<()> {
    let bytes = fs::read(path).map_err(|e| YaoshiError::build(format!("read {name}: {e}")))?;
    if !bytes.starts_with(b"MZ") {
        return Err(YaoshiError::build(format!("{name} does not begin with MZ")));
    }
    Ok(())
}
