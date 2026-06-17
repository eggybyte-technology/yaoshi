use serde::{Deserialize, Serialize};

use crate::{Sha256Hex, YaoshiError, YaoshiResult, canonical_json_bytes};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtentKind {
    NonZero,
    Zero,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredExtent {
    pub component: String,
    pub target_logical_offset: u64,
    pub source_offset: u64,
    pub length: u64,
    pub kind: ExtentKind,
    pub semantic_sha256: Sha256Hex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredExtentGraph {
    pub grammar: String,
    pub required_block_size: u64,
    pub extents: Vec<RequiredExtent>,
}

impl RequiredExtentGraph {
    pub fn new(extents: Vec<RequiredExtent>) -> YaoshiResult<Self> {
        validate_extents(&extents)?;
        Ok(Self {
            grammar: "yaoshi.required-extent-graph.v1".to_string(),
            required_block_size: crate::REQUIRED_BLOCK_SIZE,
            extents,
        })
    }

    pub fn canonical_json(&self) -> YaoshiResult<Vec<u8>> {
        let value = serde_json::to_value(self).map_err(|e| YaoshiError::internal(e.to_string()))?;
        canonical_json_bytes(&value)
    }

    pub fn digest(&self) -> YaoshiResult<Sha256Hex> {
        Ok(Sha256Hex::digest_bytes(&self.canonical_json()?))
    }
}

fn validate_extents(extents: &[RequiredExtent]) -> YaoshiResult<()> {
    let mut previous_end = 0u64;
    for (index, extent) in extents.iter().enumerate() {
        if extent.length == 0
            || extent.target_logical_offset % crate::REQUIRED_BLOCK_SIZE != 0
            || extent.length % crate::REQUIRED_BLOCK_SIZE != 0
            || extent.length > crate::PAYLOAD_EXTENT_MAX_UNCOMPRESSED_BYTES
        {
            return Err(YaoshiError::image(
                "required extent alignment or length invalid",
            ));
        }
        if index > 0 && extent.target_logical_offset < previous_end {
            return Err(YaoshiError::image(
                "required extents must be ascending and non-overlapping",
            ));
        }
        previous_end = extent
            .target_logical_offset
            .checked_add(extent.length)
            .ok_or_else(|| YaoshiError::image("required extent end overflows"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extent_graph_rejects_unaligned_ranges() {
        let err = RequiredExtentGraph::new(vec![RequiredExtent {
            component: "gpt".to_string(),
            target_logical_offset: 1,
            source_offset: 0,
            length: 4096,
            kind: ExtentKind::NonZero,
            semantic_sha256: Sha256Hex::digest_bytes(b"x"),
        }])
        .unwrap_err();
        assert_eq!(err.kind(), crate::ExitKind::Image);
    }
}
