use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::broadcast;
use serde_json::Value;
use crate::models::{CandleCacheKey, AiAnalysisService};
use crate::ai_provider::AiProvider;
use crate::cache::RetroVisCache;

impl AiAnalysisService {
    pub fn new(provider: Arc<dyn AiProvider>, cache: Arc<RetroVisCache>) -> Self {
        Self {
            provider,
            in_flight: DashMap::new(),
            cache,
        }
    }

    pub async fn get_analysis(&self, key: CandleCacheKey, ai_data: Value) -> Result<String, String> {
        // Cache checking
        if let Some(cached) = self.cache.get_ai_analysis(&key) {
            println!("Cache hit for {:?}", key.asset);
            return Ok(cached);
        }

        // Coalescing: if there's already an in-flight request for this key, subscribe to it
        if let Some(tx) = self.in_flight.get(&key) {
            println!("Coalescing request for {:?}", key.asset);
            let mut rx = tx.subscribe();
            drop(tx); 
            return rx.recv().await.map_err(|_| "Errore nella ricezione del broadcast".to_string());
        }

        // First request for this key, create a new broadcast channel and insert it into the in-flight map
        let (tx, _) = broadcast::channel(1);
        self.in_flight.insert(key.clone(), tx.clone());

        let result = self.provider.request_analysis(ai_data).await;

        match result {
            Ok(text) => {
                self.cache.save_ai_analysis(&key, text.clone());
                let _ = tx.send(text.clone());
                self.in_flight.remove(&key);
                Ok(text)
            },
            Err(e) => {
                self.in_flight.remove(&key);
                Err(e)
            }
        }
    }
}