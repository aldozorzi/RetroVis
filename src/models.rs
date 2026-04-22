use serde::{Deserialize, Serialize};
use chrono::NaiveTime;
use std::collections::HashMap;
use crate::engine::SearchEngine;



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
}

pub struct MatchOutcome {
    pub date: String,
    pub time: String,
    pub distance: f32,
    pub eod_ret_pct: f32,
    pub mfe_pct: f32,
    pub mae_pct: f32,
}

#[derive(Debug,Serialize)]
pub struct MatchResponse {
    pub date: String,
    pub time: String,
    pub distance: f32,
    pub eod_return: f32,
    pub mfe: f32,
    pub mae: f32,
}

pub struct AppState {
    pub engines: HashMap<String, SearchEngine<f32>>,
    pub sessions: HashMap<String, SessionConfig>,
}

#[derive(Debug, Serialize)]
pub struct ChartDataResponse {
    pub time: String,
    pub open: f32,
    pub high: f32,
    pub low: f32,
    pub close: f32,
}