use dashmap::DashMap;
use crate::models::{CachedResult, CandleCacheKey, ContextCacheKey};

pub struct RetroVisCache {
    pub candle_results: DashMap<CandleCacheKey, CachedResult>,
    pub context_results: DashMap<ContextCacheKey, CachedResult>,
    last_eviction: std::sync::atomic::AtomicU32,
}

impl RetroVisCache {
    pub fn new() -> Self {
        Self {
            candle_results: DashMap::new(),
            context_results: DashMap::new(),
            last_eviction: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub fn get_or_calculate_context<F>(
        &self, 
        asset: &str, 
        timestamp: u32, 
        calc_fn: F
    ) -> CachedResult 
    where 
        F: FnOnce() -> CachedResult 
    {
        self.evict_if_new_day();
        let key = ContextCacheKey::new(asset, timestamp);

        self.context_results.entry(key)
            .or_insert_with(calc_fn)
            .clone()
    }

    

    pub fn get_or_calculate_candle<F>(
        &self,
        asset: &str,
        timeframe: &str,
        timestamp: u32,
        calc_fn: F,
    ) -> CachedResult
    where
        F: FnOnce() -> CachedResult,
    {
        self.evict_if_new_day();
        let key = CandleCacheKey::new(&asset, &timeframe, timestamp);

        self.candle_results.entry(key)
            .or_insert_with(calc_fn)
            .clone()
    }

    fn evict_if_new_day(&self) {
        let today_start = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0).unwrap()
            .and_utc()
            .timestamp() as u32;

        let last = self.last_eviction.load(std::sync::atomic::Ordering::Relaxed);

        if last < today_start {
            self.last_eviction.store(today_start, std::sync::atomic::Ordering::Relaxed);
            self.candle_results.retain(|k, _| k.timestamp >= today_start);
            self.context_results.retain(|k, _| k.timestamp >= today_start);
            println!("CACHE EVICT | new day, removed previous entries");
        }
    }

    pub(crate) fn timeframe_to_seconds(tf: &str) -> i64 {
        match tf {
            "M1" => 60,
            "M5" => 300,
            "M10" => 600,
            "M15" => 900,
            "M30" => 1800,
            "H1" => 3600,
            _ => 300,
        }
    }

    
}