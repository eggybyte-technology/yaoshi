use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{YaoshiError, YaoshiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputKind {
    File,
    Tree,
    CompositeFile,
}

impl OutputKind {
    pub const fn cache_dir_name(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Tree => "tree",
            Self::CompositeFile => "composite-file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OutputRef {
    pub kind: OutputKind,
    pub digest: Sha256Hex,
}

impl OutputRef {
    pub fn new(kind: OutputKind, digest: Sha256Hex) -> Self {
        Self { kind, digest }
    }

    pub fn object_path(&self, cache_root: &Path) -> PathBuf {
        cache_root
            .join("object")
            .join(self.kind.cache_dir_name())
            .join(self.digest.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256Hex(String);

impl Sha256Hex {
    pub fn parse(raw: impl Into<String>) -> YaoshiResult<Self> {
        let raw = raw.into();
        if is_lower_sha256_hex(&raw) {
            Ok(Self(raw))
        } else {
            Err(YaoshiError::internal(
                "invalid lowercase SHA-256 hex digest",
            ))
        }
    }

    pub fn digest_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Self(hex_digest(&hasher.finalize()))
    }

    pub fn digest_canonical_json(value: &serde_json::Value) -> YaoshiResult<Self> {
        let bytes = canonical_json_bytes(value)?;
        Ok(Self::digest_bytes(&bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Sha256Hex {
    type Error = YaoshiError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<Sha256Hex> for String {
    fn from(value: Sha256Hex) -> Self {
        value.0
    }
}

pub fn is_lower_sha256_hex(raw: &str) -> bool {
    raw.len() == 64
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn canonical_json_bytes(value: &serde_json::Value) -> YaoshiResult<Vec<u8>> {
    fn sort(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut out = serde_json::Map::new();
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort();
                for key in keys {
                    out.insert(key.clone(), sort(&map[key]));
                }
                serde_json::Value::Object(out)
            }
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.iter().map(sort).collect())
            }
            _ => value.clone(),
        }
    }

    serde_json::to_vec(&sort(value))
        .map_err(|e| YaoshiError::internal(format!("serialize canonical JSON: {e}")))
}

pub fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_json_sorts_object_keys_without_whitespace() {
        let value = json!({"z": 1, "a": {"b": 2, "a": 1}});
        assert_eq!(
            canonical_json_bytes(&value).unwrap(),
            br#"{"a":{"a":1,"b":2},"z":1}"#
        );
    }

    #[test]
    fn output_ref_path_uses_kind_and_digest_only() {
        let digest =
            Sha256Hex::parse("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                .unwrap();
        let out = OutputRef::new(OutputKind::CompositeFile, digest);
        assert_eq!(
            out.object_path(Path::new(".yaoshi/cache")),
            Path::new(
                ".yaoshi/cache/object/composite-file/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            )
        );
    }
}
