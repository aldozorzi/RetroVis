use serde::Deserialize;
use crate::models::RawCandle; 

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