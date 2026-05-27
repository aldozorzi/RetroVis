use dashmap::DashMap;
use crate::models::{CachedResult, CandleCacheKey, ContextCacheKey};
use std::sync::Arc;

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
        session_id: &str, 
        timestamp: u32, 
        calc_fn: F
    ) -> CachedResult 
    where 
        F: FnOnce() -> CachedResult 
    {
        let key = ContextCacheKey {
            asset: asset.to_string(),
            session_id: session_id.to_string(),
            timestamp,
        };

        self.context_results.entry(key)
            .or_insert_with(calc_fn)
            .clone()
    }

    pub fn get_context(
        &self, 
        asset: &str, 
        session_id: &str, 
        timestamp: u32
    ) -> Option<CachedResult> { 
        self.evict_if_new_day();
        let key = ContextCacheKey {
            asset: asset.to_string(),
            session_id: session_id.to_string(),
            timestamp,
        };

        // Usiamo .get() per una lettura concorrente condivisa
        self.context_results
            .get(&key)
            .map(|r| r.value().clone()) // Accediamo al valore nel Ref e lo cloniamo
    }

    pub fn get_or_calculate_candle<F>(
        &self,
        asset: String,
        timeframe: String,
        timestamp: u32,
        calc_fn: F,
    ) -> CachedResult
    where
        F: FnOnce() -> CachedResult,
    {
        self.evict_if_new_day();
        let key = CandleCacheKey {
            asset,
            timeframe,
            timestamp,
        };

        self.candle_results.entry(key)
            .or_insert_with(calc_fn)
            .clone()
    }

    #[allow(dead_code)]
    pub fn get_session_history(
        &self,
        asset: &str,
        tf: &str,
        current_ts: u32,
        limit: usize,
    ) -> Vec<CachedResult> {
        let mut history = Vec::new();
        let step: i64 = Self::timeframe_to_seconds(tf); 

        for i in 0..limit {
            let target_ts = current_ts as i64 - (i as i64 * step);
            let key = CandleCacheKey {
                asset: asset.to_string(),
                timeframe: tf.to_string(),
                timestamp: target_ts as u32,
            };

            if let Some(res) = self.candle_results.get(&key) {
                history.push(res.clone());
            }
        }
        history.reverse();
        history
    }

    fn evict_if_new_day(&self) {
        let today_start = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0).unwrap()
            .and_utc()
            .timestamp() as u32;

        // carica il valore attuale senza lock
        let last = self.last_eviction.load(std::sync::atomic::Ordering::Relaxed);

        if last < today_start {
            // nuovo giorno — aggiorna il marker ed esegui la pulizia
            self.last_eviction.store(today_start, std::sync::atomic::Ordering::Relaxed);
            self.candle_results.retain(|k, _| k.timestamp >= today_start);
            self.context_results.retain(|k, _| k.timestamp >= today_start);
            println!("CACHE EVICT | nuovo giorno, sessioni precedenti rimosse");
        }
    }

    pub fn get_ai_analysis(&self, key: &CandleCacheKey) -> Option<String> {
        self.candle_results.get(key).and_then(|entry| {
            entry.ai_analysis.as_ref().map(|arc_str| (**arc_str).clone())
        })
    }

    pub fn save_ai_analysis(&self, key: &CandleCacheKey, analysis: String) {
        if let Some(mut entry) = self.candle_results.get_mut(key) {
            entry.ai_analysis = Some(Arc::new(analysis));
        }
    }

    pub(crate) fn timeframe_to_seconds(tf: &str) -> i64 {
        match tf {
            "M1" => 60,
            "M5" => 300,
            "M15" => 900,
            "M30" => 1800,
            "H1" => 3600,
            _ => 300,
        }
    }

    
}