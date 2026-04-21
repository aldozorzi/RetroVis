// src/models.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
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
pub struct DayResult<T> {
    pub date: String,
    pub eod_ret_pct: T,
    pub mfe_pct: T,
    pub mae_pct: T,
    pub open_0930: T, // Ora è coerente con il resto
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalPattern<T> {
    pub result: DayResult<T>,
    pub candles: Vec<NormalizedCandle<T>>,
}

#[derive(Debug)]
pub struct MatchOutcome<'a> {
    pub pattern: &'a HistoricalPattern<f32>,
    pub distance: f32,
}