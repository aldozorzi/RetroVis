#[derive(Hash, PartialEq, Eq, Clone)]
pub struct CandleCacheKey {
    pub asset: [u8; 8],
    pub timeframe: [u8; 4],
    pub timestamp: u32, 
}
impl CandleCacheKey {
    pub fn new(asset_str: &str, timeframe_str: &str, timestamp: u32) -> Self {
        Self {
            asset: str_to_fixed(asset_str),
            timeframe: str_to_fixed(timeframe_str),
            timestamp,
        }
    }
}

#[derive(Hash, PartialEq, Eq, Clone)]
pub struct ContextCacheKey {
    pub asset: [u8; 8],
    pub timestamp: u32,       
}
impl ContextCacheKey {
    pub fn new(asset_str: &str, timestamp: u32) -> Self {
        Self {
            asset: str_to_fixed(asset_str),
            timestamp,
        }
    }
}

fn str_to_fixed<const N: usize>(s: &str) -> [u8; N] {
    let mut buf = [0u8; N];
    let src = s.as_bytes();
    let amt = src.len().min(N);
    buf[..amt].copy_from_slice(&src[..amt]);
    buf
}