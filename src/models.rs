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
pub struct QueryRequest<T> {
    pub symbol: String,
    pub session_id: String,
    pub candles: Vec<RawCandle<T>>,
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
            summary: AggregateData {
                avg_up: cached.avg_up,
                avg_down: cached.avg_down,
                avg_eos: cached.avg_eos,
                edge_ratio: cached.edge_ratio,
                avg_eoh: cached.avg_eoh,
                bias_pct: cached.session_bias_pct,
            },
            matches: cached.matches.iter().cloned().map(MatchResponse::from).collect(),
            next_candle: NextCandleData {
                avg_up: cached.avg_next_up,
                avg_down: cached.avg_next_down,
                edge_ratio: cached.next_edge_ratio,
                bias_pct: cached.next_bias_pct
            },
        }
    }
}


#[derive(Debug, Clone, Serialize)]
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

impl From<HistoricalMatch> for MatchResponse {
    fn from(m: HistoricalMatch) -> Self {
        Self {
            timestamp: m.timestamp as u32,
            similarity: m.match_score as f32,
            up_pts: m.up as f32,
            down_pts: m.down as f32,
            eos_pts: m.eos as f32,
            eoh_pts: m.eoh as f32,
            next_candle_up: m.next_candle_up_pct,
            next_candle_down: m.next_candle_down_pct,
        }
    }
}

#[derive(Debug, serde::Serialize, Clone, Copy)]
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

#[derive(Debug, Serialize)]
pub struct ChartDataResponse {
    pub timestamp: u32,
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
        let mut asset = [0u8; 8];
        let src_a = asset_str.as_bytes();
        let amt_a = src_a.len().min(8);
        asset[..amt_a].copy_from_slice(&src_a[..amt_a]);

        let mut timeframe = [0u8; 4];
        let src_t = timeframe_str.as_bytes();
        let amt_t = src_t.len().min(4);
        timeframe[..amt_t].copy_from_slice(&src_t[..amt_t]);

        Self { asset, timeframe, timestamp }
    }
}

#[derive(Hash, PartialEq, Eq, Clone)]
pub struct ContextCacheKey {
    pub asset: [u8; 8],
    pub timestamp: u32,       
}
impl ContextCacheKey {
    pub fn new(asset_str: &str, timestamp: u32) -> Self {
        let mut asset = [0u8; 8];
        let src = asset_str.as_bytes();
        let amt = src.len().min(8);
        asset[..amt].copy_from_slice(&src[..amt]);

        Self { asset, timestamp }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HistoricalMatch {
    pub timestamp: u32,      
    pub match_score: f32,  
    pub up: f32,           
    pub down: f32,        
    pub eos: f32,        
    pub eoh: f32, 
    pub next_candle_up_pct: f32,
    pub next_candle_down_pct: f32,      
}

impl From<MatchResponse> for HistoricalMatch {
    fn from(m: MatchResponse) -> Self {
        Self {
            timestamp: m.timestamp as u32,
            match_score: m.similarity,
            up: m.up_pts,
            down: m.down_pts,
            eos: m.eos_pts,
            eoh: m.eoh_pts,
            next_candle_up_pct: m.next_candle_up,
            next_candle_down_pct: m.next_candle_down,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedResult {
    pub edge_ratio: f32,
    pub avg_eos: f32,
    pub avg_up: f32,
    pub avg_down: f32,
    pub matches: Arc<Vec<HistoricalMatch>>,
    pub ai_analysis: Option<Arc<String>>,
    pub avg_eoh: f32,
    pub avg_next_up: f32, 
    pub avg_next_down: f32,
    pub next_edge_ratio: f32,
    pub next_bias_pct: f32,
    pub session_bias_pct: f32,
}

impl CachedResult {
    pub fn from_aggregates(summary: AggregateData, matches: Vec<MatchResponse>) -> Self {
        let (avg_next_up, avg_next_down) = if !matches.is_empty() {
            let mut ups: Vec<f32> = matches.iter().map(|m| m.next_candle_up).collect();
            let mut downs: Vec<f32> = matches.iter().map(|m| m.next_candle_down).collect();
            ups.sort_by(|a, b| a.partial_cmp(b).unwrap());
            downs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            (ups[ups.len() / 2], downs[downs.len() / 2])
        } else {
            (0.0, 0.0)
        };

        let long_edge = avg_next_up / (if avg_next_down == 0.0 { 1.0 } else { avg_next_down });
        let short_edge = avg_next_down / (if avg_next_up == 0.0 { 1.0 } else { avg_next_up });
        let next_edge_ratio = long_edge.max(short_edge);

        let mut next_bullish = 0usize;
        let mut next_bearish = 0usize;
        for m in matches.iter() {
            if m.next_candle_up > m.next_candle_down { next_bullish += 1; }
            else if m.next_candle_down > m.next_candle_up { next_bearish += 1; }
        }

        let next_total = (next_bullish + next_bearish) as f32;
        let next_bias_pct = if next_total > 0.0 {
            if next_bullish >= next_bearish {
                next_bullish as f32 / next_total * 100.0
            } else {
                -(next_bearish as f32 / next_total * 100.0)
            }
        } else { 0.0 };

        let historical_matches: Vec<HistoricalMatch> = matches.into_iter().map(HistoricalMatch::from).collect();

        Self {
            edge_ratio: summary.edge_ratio,
            avg_eos: summary.avg_eos,
            avg_up: summary.avg_up,
            avg_down: summary.avg_down,
            avg_eoh: summary.avg_eoh,
            matches: Arc::new(historical_matches),
            ai_analysis: None,
            avg_next_up,
            avg_next_down,
            next_edge_ratio,
            next_bias_pct,
            session_bias_pct: summary.bias_pct,
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