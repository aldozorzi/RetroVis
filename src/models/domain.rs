use serde::{Deserialize, Serialize};
use chrono::NaiveTime;
use std::ops::Range;

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct RawCandle<T> {
    pub timestamp: u32,
    pub open: T,
    pub high: T,
    pub low: T,
    pub close: T,
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
            gap_pct: ((session_open - prev_close) / prev_close.max(1e-6)) * 100.0,
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