use std::fs;
use std::path::Path;

use object::Object;
use yaoshi_common::{YaoshiError, YaoshiResult};

pub fn check_elf64_static_x86_64(path: &Path) -> YaoshiResult<()> {
    let bytes = fs::read(path).map_err(|e| YaoshiError::build(format!("read ELF: {e}")))?;
    let file = object::File::parse(bytes.as_slice())
        .map_err(|e| YaoshiError::build(format!("parse ELF: {e}")))?;
    if file.architecture() != object::Architecture::X86_64 || !file.is_64() {
        return Err(YaoshiError::build("runtime binary is not ELF64 x86-64"));
    }
    if elf64_has_pt_interp(&bytes)? {
        return Err(YaoshiError::build(
            "runtime binary contains a PT_INTERP program header",
        ));
    }
    Ok(())
}

fn elf64_has_pt_interp(bytes: &[u8]) -> YaoshiResult<bool> {
    const EI_CLASS: usize = 4;
    const EI_DATA: usize = 5;
    const ELFCLASS64: u8 = 2;
    const ELFDATA2LSB: u8 = 1;
    const PT_INTERP: u32 = 3;

    if bytes.len() < 64 || &bytes[0..4] != b"\x7fELF" {
        return Err(YaoshiError::build("runtime binary is not an ELF file"));
    }
    if bytes[EI_CLASS] != ELFCLASS64 || bytes[EI_DATA] != ELFDATA2LSB {
        return Err(YaoshiError::build(
            "runtime binary is not ELF64 little-endian",
        ));
    }

    let phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap()) as usize;
    let phentsize = u16::from_le_bytes(bytes[54..56].try_into().unwrap()) as usize;
    let phnum = u16::from_le_bytes(bytes[56..58].try_into().unwrap()) as usize;
    if phentsize < 56 {
        return Err(YaoshiError::build("invalid ELF program header size"));
    }

    for idx in 0..phnum {
        let offset = phoff
            .checked_add(
                idx.checked_mul(phentsize)
                    .ok_or_else(|| YaoshiError::build("ELF program header offset overflows"))?,
            )
            .ok_or_else(|| YaoshiError::build("ELF program header offset overflows"))?;
        let end = offset
            .checked_add(4)
            .ok_or_else(|| YaoshiError::build("ELF program header offset overflows"))?;
        if end > bytes.len() {
            return Err(YaoshiError::build("ELF program header exceeds file size"));
        }
        if u32::from_le_bytes(bytes[offset..end].try_into().unwrap()) == PT_INTERP {
            return Ok(true);
        }
    }
    Ok(false)
}
