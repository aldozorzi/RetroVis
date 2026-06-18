use serde::{Deserialize, Serialize, Serializer};
use chrono::NaiveTime;
use std::collections::HashMap;
use crate::engine::SearchEngine;
use std::sync::Arc;
use dashmap::DashMap;
use crate::cache::RetroVisCache;
use tokio::sync::broadcast;
use crate::ai_provider::AiProvider;
use std::ops::Range;
use crate::stats::{median, edge_ratio, bias_pct};


#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RawCandle<T> {
    pub timestamp: u32,
    pub open: T,
    pub high: T,
    pub low: T,
    pub close: T,
    //pub _volume: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedCandle<T> {
    pub body_pct: T,
    pub wick_up_pct: T,
    pub wick_down_pct: T,
    pub range_pct: T,
    pub timestamp: u32
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
            timestamp: raw.timestamp,
        }
    }
}

pub struct CandleStore {
    pub candles: Vec<RawCandle<f32>>,
    pub days: Vec<(u32, Range<usize>)>,  // (date_num, range in candles)
}

impl CandleStore {
    pub fn len(&self) -> usize { self.days.len() }
    pub fn is_empty(&self) -> bool { self.days.is_empty() }

    pub fn get(&self, idx: usize) -> Option<(u32, &[RawCandle<f32>])> {
        self.days.get(idx).map(|(d, r)| (*d, &self.candles[r.clone()]))
    }

    pub fn find_by_date(&self, date: u32) -> Option<usize> {
        self.days.binary_search_by_key(&date, |(d, _)| *d).ok()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionConfig {
    pub id: String,
    pub start_time: NaiveTime,
    pub timezone: String,
}

#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub symbol: String,
    pub session_id: String,
    pub candles: Vec<RawCandle<f32>>,
    pub top_k: usize,
    pub timestamp: u32,
    pub timeframe: String,
    pub session_open: f32,
}

pub struct MatchOutcome {
    pub timestamp: u32,
    pub distance: f32,
    pub eod_ret_pct: f32,
    pub mfe_pct: f32,
    pub mae_pct: f32,
    pub eoh_pct: f32,
    pub next_candle_up_pct: Option<f32>,
    pub next_candle_down_pct: Option<f32>,
}

#[derive(Debug, serde::Serialize)]
pub struct MatchesResponse {
    pub summary: AggregateData,
    pub matches: Vec<MatchResponse>,
    pub next_candle: NextCandleData,
}
impl From<CachedResult> for MatchesResponse {
    fn from(cached: CachedResult) -> Self {
        Self {
            summary: cached.summary,
            matches: (*cached.matches).clone(),
            next_candle: cached.next_candle,
        }
    }
}


#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MatchResponse {
    pub timestamp: u32,
    pub similarity: f32,
    pub eos_pts: f32,
    pub up_pts: f32,
    pub down_pts: f32,
    pub eoh_pts: f32,
    pub next_candle_up: f32,  
    pub next_candle_down: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct AggregateData {
    pub avg_up: f32,
    pub avg_down: f32,
    pub avg_eos: f32,
    pub edge_ratio: f32,
    pub avg_eoh: f32,
    #[serde(serialize_with = "serialize_f32_as_int")]
    pub bias_pct: f32,
}

#[derive(Debug, serde::Serialize, Clone, Copy, serde::Deserialize)]
pub struct NextCandleData {
    pub avg_up: f32,
    pub avg_down: f32,
    pub edge_ratio: f32,
    #[serde(serialize_with = "serialize_f32_as_int")]
    pub bias_pct: f32,
}

pub struct AppState {
    pub engines: HashMap<String, SearchEngine>,
    pub sessions: HashMap<String, SessionConfig>,
    pub cache: Arc<crate::cache::RetroVisCache>,
    pub broker: Arc<dyn AiProvider>,
    pub ai_service: Arc<AiAnalysisService>,
    // pub oanda: Arc<crate::oanda::OandaClient>,
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
    pub timestamp: u32, 
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

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedResult {
    pub summary: AggregateData,
    pub matches: Arc<Vec<MatchResponse>>,
    pub ai_analysis: Option<Arc<String>>,
    pub next_candle: NextCandleData,
}

impl CachedResult {
    pub fn from_aggregates(summary: AggregateData, matches: Vec<MatchResponse>) -> Self {
        let mut ups: Vec<f32>   = matches.iter().map(|m| m.next_candle_up).collect();
        let mut downs: Vec<f32> = matches.iter().map(|m| m.next_candle_down).collect();

        let avg_next_up   = median(&mut ups);
        let avg_next_down = median(&mut downs);
        let next_edge_ratio = edge_ratio(avg_next_up, avg_next_down);
        let next_bias_pct = bias_pct(matches.iter().map(|m| (m.next_candle_up, m.next_candle_down)));

        Self {
            summary,
            matches: Arc::new(matches),
            ai_analysis: None,
            next_candle: NextCandleData {
                avg_up: avg_next_up,
                avg_down: avg_next_down,
                edge_ratio: next_edge_ratio,
                bias_pct: next_bias_pct,
            },
        }
    }
}

#[derive(serde::Deserialize)]
pub struct AiAnalysisRequest {
    pub symbol: String,
    pub session_id: String,
    pub candles: Vec<RawCandle<f32>>,
    pub timestamp: u32,
    pub timeframe: String,
    pub current_price_action: f32,
    pub session_open: f32,
    pub prev_close: f32,
    pub prev_high: f32,
    pub prev_low: f32,
    pub intraday_high: f32,
    pub intraday_low: f32,
    pub top_k: usize,
}

pub struct AiAnalysisService {
    // Abstract Provider to allow for different implementations (e.g., Gemini, custom LLM, etc.)
    pub provider: Arc<dyn AiProvider>,
    // Coalescing map to track in-flight requests and their subscribers
    pub in_flight: DashMap<CandleCacheKey, broadcast::Sender<String>>,
    // Global cache manager reference for storing results after successful analysis
    pub cache: Arc<RetroVisCache>,
}

// Funzione helper che esegue il cast a intero durante la serializzazione
fn serialize_f32_as_int<S>(value: &f32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    // Usa .round() se vuoi arrotondare, oppure rimuovilo se vuoi solo troncare
    serializer.serialize_i32(value.round() as i32)
}

fn str_to_fixed<const N: usize>(s: &str) -> [u8; N] {
    let mut buf = [0u8; N];
    let src = s.as_bytes();
    let amt = src.len().min(N);
    buf[..amt].copy_from_slice(&src[..amt]);
    buf
}