//! An in-memory compact-block cache. `sync::run` downloads a batch, scans it, then deletes
//! it, so the cache only ever holds one batch.

use std::sync::{Arc, Mutex};

use zcash_client_backend::data_api::chain::{error::Error as ChainError, BlockCache, BlockSource};
use zcash_client_backend::data_api::scanning::ScanRange;
use zcash_client_backend::proto::compact_formats::CompactBlock;
use zcash_protocol::consensus::BlockHeight;

#[derive(Debug)]
pub struct CacheError;

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("block cache error")
    }
}

impl std::error::Error for CacheError {}

#[derive(Clone, Default)]
pub struct MemBlockCache {
    blocks: Arc<Mutex<Vec<CompactBlock>>>,
}

fn height(b: &CompactBlock) -> BlockHeight {
    BlockHeight::from_u32(b.height as u32)
}

impl BlockSource for MemBlockCache {
    type Error = CacheError;

    fn with_blocks<F, WalletErrT>(
        &self,
        from_height: Option<BlockHeight>,
        limit: Option<usize>,
        mut with_block: F,
    ) -> Result<(), ChainError<WalletErrT, Self::Error>>
    where
        F: FnMut(CompactBlock) -> Result<(), ChainError<WalletErrT, Self::Error>>,
    {
        let mut blocks: Vec<CompactBlock> = {
            let cached = self.blocks.lock().map_err(|_| ChainError::BlockSource(CacheError))?;
            cached.iter().filter(|b| from_height.is_none_or(|h| height(b) >= h)).cloned().collect()
        };
        blocks.sort_by_key(|b| b.height);
        for block in blocks.into_iter().take(limit.unwrap_or(usize::MAX)) {
            with_block(block)?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl BlockCache for MemBlockCache {
    fn get_tip_height(&self, range: Option<&ScanRange>) -> Result<Option<BlockHeight>, Self::Error> {
        let cached = self.blocks.lock().map_err(|_| CacheError)?;
        Ok(cached.iter().map(height).filter(|h| range.is_none_or(|r| r.block_range().contains(h))).max())
    }

    async fn read(&self, range: &ScanRange) -> Result<Vec<CompactBlock>, Self::Error> {
        let cached = self.blocks.lock().map_err(|_| CacheError)?;
        let mut blocks: Vec<CompactBlock> = cached.iter().filter(|b| range.block_range().contains(&height(b))).cloned().collect();
        blocks.sort_by_key(|b| b.height);
        Ok(blocks)
    }

    async fn insert(&self, mut compact_blocks: Vec<CompactBlock>) -> Result<(), Self::Error> {
        self.blocks.lock().map_err(|_| CacheError)?.append(&mut compact_blocks);
        Ok(())
    }

    async fn delete(&self, range: ScanRange) -> Result<(), Self::Error> {
        self.blocks.lock().map_err(|_| CacheError)?.retain(|b| !range.block_range().contains(&height(b)));
        Ok(())
    }
}
