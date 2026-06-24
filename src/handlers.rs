use axum::{
    extract::{State, Query},
    Json,
    response::IntoResponse,
};
use std::sync::Arc;
use std::collections::HashMap;
use serde_json::json;

use crate::models::{
    AppState, AiAnalysisRequest, CandleCacheKey, ContextQueryRequest, MatchesResponse,
    QueryRequest, RawCandle,
};
use crate::service::{
    build_ai_payload, compute_candle_history, compute_context, fetch_or_compute_context,
    fetch_or_compute_history,
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

pub async fn ai_analysis_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AiAnalysisRequest>,
) -> impl IntoResponse {
    let key = CandleCacheKey::new(&req.symbol, &req.timeframe, req.timestamp);

    let history = fetch_or_compute_history(&state, &req);
    let context = fetch_or_compute_context(&state, &req);
    let ai_data = build_ai_payload(&req, &history, &context);

    match state.ai_service.get_analysis(key, ai_data).await {
        Ok(analysis) => Json(json!({ "status": "success", "analysis": analysis })),
        Err(e) => {
            eprintln!("AI analysis error: {}", e);
            Json(json!({ "status": "error", "message": format!("Analysis error: {}", e) }))
        }
    }
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
