use crate::models::{
    AppState, AggregateData, CachedResult, Context, MatchOutcome, MatchResponse,
    NormalizedCandle, RawCandle, SessionConfig,
};
use crate::stats::{median, weighted_projection};
use crate::engine::SearchEngine;
use serde_json::json;

pub fn compute_candle_history(
    state: &AppState,
    symbol: &str,
    timeframe: &str,
    session_id: &str,
    timestamp: u32,
    candles: &[RawCandle<f32>],
    session_open: f32,
    top_k: usize,
) -> Vec<CachedResult> {
    let timeframe_seconds = crate::cache::RetroVisCache::timeframe_to_seconds(timeframe);
    let (engine, session) = lookup_engine_and_session(state, symbol, session_id);

    (1..=candles.len()).map(|i| {
        let slice      = &candles[0..i];
        let current_ts = timestamp as i64 - ((candles.len() - i) as i64 * timeframe_seconds);

        state.cache.get_or_calculate_candle(
            symbol, timeframe,
            current_ts as u32,
            || {
                let normalized: Vec<NormalizedCandle<f32>> = slice.iter()
                    .map(|raw| NormalizedCandle::from_raw(raw, session_open))
                    .collect();
                let matches = engine.find_matches(
                    &normalized, top_k.min(10), session, (timeframe_seconds / 60) as u32
                );
                let (summary, custom_matches) = calculate_aggregates(&matches, session_open);
                CachedResult::from_aggregates(summary, custom_matches)
            }
        )
    }).collect()
}

pub fn compute_context(
    state: &AppState,
    symbol: &str,
    session_id: &str,
    timestamp: u32,
    session_open: f32,
    prev_close: f32,
    prev_high: f32,
    prev_low: f32,
    intraday_high: f32,
    intraday_low: f32,
    top_k: usize,
) -> CachedResult {
    let (engine, session) = lookup_engine_and_session(state, symbol, session_id);

    state.cache.get_or_calculate_context(symbol, timestamp, || {
        let ctx = Context::new(
            session_open, prev_close, prev_high, prev_low, intraday_high, intraday_low,
        );
        let matches = engine.find_context_matches(&ctx, top_k.min(10), session, timestamp);
        let (summary, custom_matches) = calculate_aggregates(&matches, session_open);
        CachedResult::from_aggregates(summary, custom_matches)
    })
}

pub fn calculate_aggregates(matches: &[MatchOutcome], current_price: f32) -> (AggregateData, Vec<MatchResponse>) {
    let response_matches: Vec<MatchResponse> = matches.iter().map(|m| {
        let up = (m.mfe_pct / 100.0) * current_price;
        let down = (m.mae_pct / 100.0) * current_price;
        let eos = (m.eod_ret_pct / 100.0) * current_price;
        let eoh_pts = (m.eoh_pct / 100.0) * current_price;
        let next_up_pts = (m.next_candle_up_pct.unwrap_or(0.0) / 100.0) * current_price;
        let next_down_pts = (m.next_candle_down_pct.unwrap_or(0.0) / 100.0) * current_price;

        MatchResponse {
            timestamp: m.timestamp,
            similarity: 1.0 / (1.0 + m.distance) * 100.0,
            up_pts: up,
            down_pts: down,
            eos_pts: eos,
            eoh_pts,
            next_candle_up: next_up_pts,
            next_candle_down: next_down_pts,
        }
    }).collect();

    let up_mags: Vec<f32> = response_matches.iter().map(|m| m.up_pts.abs()).collect();
    let down_mags: Vec<f32> = response_matches.iter().map(|m| m.down_pts.abs()).collect();
    let similarities: Vec<f32> = response_matches.iter().map(|m| m.similarity).collect();
    let mut eos_values: Vec<f32> = response_matches.iter().map(|m| m.eos_pts).collect();
    let mut eoh_values: Vec<f32> = response_matches.iter().map(|m| m.eoh_pts).collect();

    let proj = weighted_projection(&up_mags, &down_mags, &similarities);

    (
        AggregateData {
            avg_up: proj.avg_up,
            avg_down: proj.avg_down,
            avg_eos: median(&mut eos_values),
            avg_eoh: median(&mut eoh_values),
            edge_ratio: proj.edge_ratio,
            bias_pct: proj.bias_pct,
            confidence: (proj.confidence * 100.0).round() as u8,
        },
        response_matches
    )
}

pub fn lookup_engine_and_session<'a>(
    state: &'a AppState,
    symbol: &str,
    session_id: &str,
) -> (&'a SearchEngine, &'a SessionConfig) {
    let engine = state.engines.get(symbol).expect("Ticker not found");
    let session = state.sessions.get(session_id).expect("Session not found");
    (engine, session)
}

/// Cache-aside: reuse cached per-candle results when present, otherwise compute fresh.
pub fn fetch_or_compute_history(state: &AppState, req: &crate::models::AiAnalysisRequest) -> Vec<CachedResult> {
    let cached = state.cache.get_session_history(
        &req.symbol, &req.timeframe, req.timestamp, req.candles.len()
    );
    if !cached.is_empty() {
        return cached;
    }
    compute_candle_history(
        state, &req.symbol, &req.timeframe, &req.session_id,
        req.timestamp, &req.candles, req.session_open as f32, req.top_k,
    )
}

/// Cache-aside for the context match: reuse if present, otherwise compute fresh.
pub fn fetch_or_compute_context(state: &AppState, req: &crate::models::AiAnalysisRequest) -> CachedResult {
    state.cache.get_context(&req.symbol, req.timestamp).unwrap_or_else(|| {
        compute_context(
            state, &req.symbol, &req.session_id, req.timestamp,
            req.session_open as f32, req.prev_close, req.prev_high,
            req.prev_low, req.intraday_high, req.intraday_low, req.top_k,
        )
    })
}

pub fn build_ai_payload(req: &crate::models::AiAnalysisRequest, history: &[CachedResult], context: &CachedResult) -> serde_json::Value {
    json!({
        "symbol": req.symbol,
        "session_id": req.session_id,
        "current_price_action": req.current_price_action,
        "evolution_history": history,
        "context": context,
        "session_open": req.session_open,
    })
}
