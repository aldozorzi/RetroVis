use std::collections::HashMap;
use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::broadcast;
use crate::engine::SearchEngine;
use crate::cache::RetroVisCache;
use crate::ai_provider::AiProvider;
use crate::models::{SessionConfig, CandleCacheKey};

pub struct AppState {
    pub engines: HashMap<String, SearchEngine>,
    pub sessions: HashMap<String, SessionConfig>,
    pub cache: Arc<crate::cache::RetroVisCache>,
    pub broker: Arc<dyn AiProvider>,
    pub ai_service: Arc<AiAnalysisService>,
    // pub oanda: Arc<crate::oanda::OandaClient>,
}

pub struct AiAnalysisService {
    // Abstract Provider to allow for different implementations (e.g., Gemini, custom LLM, etc.)
    pub provider: Arc<dyn AiProvider>,
    // Coalescing map to track in-flight requests and their subscribers
    pub in_flight: DashMap<CandleCacheKey, broadcast::Sender<String>>,
    // Global cache manager reference for storing results after successful analysis
    pub cache: Arc<RetroVisCache>,
}