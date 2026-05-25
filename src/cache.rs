use dashmap::DashMap;
use crate::models::{CachedResult, CandleCacheKey, ContextCacheKey};

pub struct RetroVisCache {
    // Risultati delle singole candele (la storia della sessione)
    pub candle_results: DashMap<CandleCacheKey, CachedResult>,
    // Risultato del contesto (fisso per asset/sessione/giorno)
    pub context_results: DashMap<ContextCacheKey, CachedResult>,
}

impl RetroVisCache {
    pub fn new() -> Self {
        Self {
            candle_results: DashMap::new(),
            context_results: DashMap::new(),
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
    ) -> Option<CachedResult> { // Restituisce Option perché il dato potrebbe non esserci
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
        session_id: String,
        timestamp: u32,
        calc_fn: F,
    ) -> CachedResult
    where
        F: FnOnce() -> CachedResult,
    {
        let key = CandleCacheKey {
            asset,
            timeframe,
            session_id,
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
        session_id: &str,
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
                session_id: session_id.to_string(),
                timestamp: target_ts as u32,
            };

            if let Some(res) = self.candle_results.get(&key) {
                history.push(res.clone());
            }
        }
        history.reverse();
        history
    }

    pub fn get_ai_analysis(&self, key: &CandleCacheKey) -> Option<String> {
        self.candle_results.get(key).and_then(|entry| entry.ai_analysis.clone())
    }

    pub fn save_ai_analysis(&self, key: &CandleCacheKey, analysis: String) {
        if let Some(mut entry) = self.candle_results.get_mut(key) {
            entry.ai_analysis = Some(analysis);
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