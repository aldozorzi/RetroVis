use axum::{
    extract::{State, Query},
    Json,
};
use std::sync::Arc;
use std::collections::HashMap;

use crate::models::{
    AppState, ContextQueryRequest, MatchesResponse,
    QueryRequest, RawCandle,
};
use crate::service::{
    compute_candle_history, compute_context, 
};

pub async fn search_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Json<MatchesResponse> {
    let history = compute_candle_history(
        &state, &req.symbol, &req.timeframe, &req.session_id,
        req.timestamp, &req.candles, req.session_open, req.top_k,
    );
    Json(MatchesResponse::from(history.into_iter().last().expect("No candles provided")))
}

pub async fn context_matches_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ContextQueryRequest>,
) -> Json<MatchesResponse> {
    let result = compute_context(
        &state, &req.symbol, &req.session_id,
        req.timestamp,          // was Utc::now() — fixed bug
        req.session_open, req.prev_close, req.prev_high,
        req.prev_low, req.intraday_high, req.intraday_low,
        req.top_k,
    );
    Json(MatchesResponse::from(result))
}

pub async fn day_details_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<Vec<RawCandle<f32>>> {
    let symbol = params.get("symbol").cloned().unwrap_or_else(|| "NQ".to_string());
    let date_str = params.get("date").cloned().unwrap_or_default();
    let date: u32 = date_str.replace('-', "").parse().unwrap_or(0);
    let timeframe = params.get("timeframe").map(|s| s.as_str()).unwrap_or("M5");
    let timeframe_seconds = crate::cache::RetroVisCache::timeframe_to_seconds(timeframe);
    let timeframe_min = (timeframe_seconds / 60) as u32;

    if let Some(engine) = state.engines.get(&symbol) {
        return Json(engine.get_day_candles(date, timeframe_min, true));
    }

    Json(vec![])
}
