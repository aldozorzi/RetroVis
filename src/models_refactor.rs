use serde::{Deserialize, Serialize};
use chrono::NaiveTime;
use std::collections::HashMap;
use crate::engine::SearchEngine;
use std::sync::Arc;
use dashmap::DashMap;
use crate::cache::RetroVisCache;
use tokio::sync::broadcast;
use crate::ai_provider::AiProvider;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RawCandle<T> {
    pub date: String,
    pub time: String,
    pub open: T,
    pub high: T,
    pub low: T,
    pub close: T,
    pub _volume: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedCandle<T> {
    pub body_pct: T,
    pub wick_up_pct: T,
    pub wick_down_pct: T,
    pub range_pct: T,
}

impl NormalizedCandle<f32> {
    pub fn from_raw(raw: &RawCandle<f32>, baseline: f32) -> Self {
        let max_oc = raw.open.max(raw.close);
        let min_oc = raw.open.min(raw.close);
        
        Self {
            body_pct: ((raw.close - raw.open) / baseline) * 100.0,
            wick_up_pct: ((raw.high - max_oc) / baseline) * 100.0,
            wick_down_pct: ((min_oc - raw.low) / baseline) * 100.0,
            range_pct: ((raw.high - raw.low) / baseline) * 100.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalDay<T> {
    pub date: String,
    pub candles: Vec<RawCandle<T>>, 
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionConfig {
    pub id: String,
    pub start_time: NaiveTime,
    pub timezone: String,
}

#[derive(Debug, Deserialize)]
pub struct QueryRequest<T> {
    pub symbol: String,
    pub session_id: String,
    pub candles: Vec<RawCandle<T>>,
    pub top_k: usize,
    pub timestamp: i64,
    pub timeframe: String,
    pub session_open: f32,
}

pub struct MatchOutcome {
    pub date: String,
    pub time: String,
    pub distance: f32,
    pub eod_ret_pct: f32,
    pub mfe_pct: f32,
    pub mae_pct: f32,
    /// Return % at the end of the first trading hour (London 09:00 / NY 10:30).
    /// `None` when the EOH candle is not present in the historical data for that day.
    pub eoh_pct: Option<f32>,
}

#[derive(serde::Serialize)]
pub struct MatchesResponse {
    pub summary: AggregateData,
    pub matches: Vec<MatchResponse>,
}
impl From<CachedResult> for MatchesResponse {
    fn from(cached: CachedResult) -> Self {
        Self {
            summary: AggregateData {
                avg_up: cached.avg_up as f32,
                avg_down: cached.avg_down as f32,
                avg_eos: cached.avg_eos as f32,
                avg_eoh: cached.avg_eoh as f32,
                edge_ratio: cached.edge_ratio as f32,
            },
            matches: cached.matches.into_iter().map(MatchResponse::from).collect(),
        }
    }
}


#[derive(Debug, Clone, Serialize)]
pub struct MatchResponse {
    pub date: String,
    pub time: String,
    pub similarity: f32,
    pub eos_pts: f32,
    pub up_pts: f32,
    pub down_pts: f32,
    pub eoh_pts: Option<f32>,
}

impl From<HistoricalMatch> for MatchResponse {
    fn from(m: HistoricalMatch) -> Self {
        Self {
            date: m.date,
            time: "".to_string(), // O gestiscilo se serve
            similarity: m.match_score as f32,
            up_pts: m.up as f32,
            down_pts: m.down as f32,
            eos_pts: m.eos as f32,
            eoh_pts: m.eoh.map(|v| v as f32),
        }
    }
}

#[derive(serde::Serialize, Clone, Copy)]
pub struct AggregateData {
    pub avg_up: f32,
    pub avg_down: f32,
    pub avg_eos: f32,
    pub avg_eoh: f32,
    pub edge_ratio: f32,
}

pub struct AppState {
    pub engines: HashMap<String, SearchEngine<f32>>,
    pub sessions: HashMap<String, SessionConfig>,
    pub cache: Arc<crate::cache::RetroVisCache>,
    pub broker: Arc<dyn AiProvider>,
    pub ai_service: Arc<AiAnalysisService>,
    pub oanda: Arc<crate::oanda::OandaClient>,
}

#[derive(Debug, Serialize)]
pub struct ChartDataResponse {
    pub time: String,
    pub open: f32,
    pub high: f32,
    pub low: f32,
    pub close: f32,
}

pub struct Context {
    pub gap_pct: f32,
    pub pos_in_prev_range: f32,
    pub dist_from_prev_high: f32,
    pub dist_from_intraday_high: f32,
    pub dist_from_intraday_low: f32,
}

impl Context {
    pub fn new(
        session_open: f32,
        prev_close: f32,
        prev_high: f32,
        prev_low: f32,
        intraday_high: f32,
        intraday_low: f32,
    ) -> Self {
        let prev_range = (prev_high - prev_low).max(1e-6);
        Self {
            gap_pct: ((session_open - prev_close) / prev_close) * 100.0,
            pos_in_prev_range: (session_open - prev_low) / prev_range,
            dist_from_prev_high: ((prev_high - session_open) / prev_high) * 100.0,
            dist_from_intraday_high: ((intraday_high - session_open) / intraday_high.max(1e-6)) * 100.0,
            dist_from_intraday_low: ((session_open - intraday_low) / intraday_low.max(1e-6)) * 100.0,
        }
    }

    pub fn distance(&self, other: &Context) -> f32 {
        let mut d = 0.0;
        d += (self.gap_pct - other.gap_pct).powi(2);
        d += (self.pos_in_prev_range - other.pos_in_prev_range).powi(2);
        d += (self.dist_from_prev_high - other.dist_from_prev_high).powi(2);
        d += (self.dist_from_intraday_high - other.dist_from_intraday_high).powi(2);
        d += (self.dist_from_intraday_low - other.dist_from_intraday_low).powi(2);
        d.sqrt()
    }
}

#[derive(Debug, Deserialize)]
pub struct ContextQueryRequest {
    pub symbol: String,
    pub session_id: String,
    pub session_open: f32,
    pub prev_close: f32,
    pub prev_high: f32,
    pub prev_low: f32,
    pub intraday_high: f32,
    pub intraday_low: f32,
    pub top_k: usize,
}

#[derive(Hash, PartialEq, Eq, Clone)]
pub struct CandleCacheKey {
    pub asset: String,
    pub timeframe: String,
    pub session_id: String, 
    pub timestamp: i64, 
}

#[derive(Hash, PartialEq, Eq, Clone)]
pub struct ContextCacheKey {
    pub asset: String,
    pub session_id: String, 
    pub date: String,       
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HistoricalMatch {
    pub date: String,      
    pub match_score: f64,  
    pub up: f64,           
    pub down: f64,        
    pub eos: f64,
    pub eoh: Option<f64>,
}

impl From<MatchResponse> for HistoricalMatch {
    fn from(m: MatchResponse) -> Self {
        Self {
            date: m.date,
            match_score: m.similarity as f64,
            up: m.up_pts as f64,
            down: m.down_pts as f64,
            eos: m.eos_pts as f64,
            eoh: m.eoh_pts.map(|v| v as f64),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedResult {
    pub edge_ratio: f64,
    pub avg_eos: f64,
    pub avg_up: f64,
    pub avg_down: f64,
    pub avg_eoh: f64,
    pub matches: Vec<HistoricalMatch>,
    pub ai_analysis: Option<String>,
}

impl CachedResult {
    pub fn from_aggregates(summary: AggregateData, matches: Vec<MatchResponse>) -> Self {
        Self {
            edge_ratio: summary.edge_ratio as f64,
            avg_eos: summary.avg_eos as f64,
            avg_up: summary.avg_up as f64,
            avg_down: summary.avg_down as f64,
            avg_eoh: summary.avg_eoh as f64,
            matches: matches.into_iter().map(HistoricalMatch::from).collect(),
            ai_analysis: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FullQueryRequest {
    pub symbol: String,
    /// Pattern search depth; capped at 24 for this endpoint.
    pub top_k: usize,
    /// Unix timestamp (UTC) of the reference candle, aligned with `QueryRequest.timestamp`.
    pub timestamp: i64,
    pub timeframe: String,
}

#[derive(Serialize)]
pub struct FullResponse {
    pub summary: AggregateData,
    pub matches: Vec<MatchResponse>,
    pub context: MatchesResponse,
    pub ai_analysis: String,
}

#[derive(serde::Deserialize)]
pub struct AiAnalysisRequest {
    pub symbol: String,
    pub session_id: String,
    pub candles_len: usize,
    pub timestamp: i64,
    pub timeframe: String,
    pub current_price_action: f64,
    pub session_open: f64,
}

pub struct AiAnalysisService {
    // Abstract Provider to allow for different implementations (e.g., Gemini, custom LLM, etc.)
    pub provider: Arc<dyn AiProvider>,
    // Coalescing map to track in-flight requests and their subscribers
    pub in_flight: DashMap<CandleCacheKey, broadcast::Sender<String>>,
    // Global cache manager reference for storing results after successful analysis
    pub cache: Arc<RetroVisCache>,
}