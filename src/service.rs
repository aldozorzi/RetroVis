use crate::models::{
    AppState, AggregateData, CachedResult, Context, MatchOutcome, MatchResponse,
    NormalizedCandle, RawCandle, SessionConfig,
};
use crate::stats::{median, weighted_projection};
use crate::engine::SearchEngine;

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
        let last_close = slice.last().map(|c| c.close).unwrap_or(session_open);

        state.cache.get_or_calculate_candle(
            symbol, timeframe,
            current_ts as u32,
            || {
                let normalized: Vec<NormalizedCandle<f32>> = slice.iter()
                    .map(|raw| NormalizedCandle::from_raw(raw, session_open))
                    .collect();
                let valid_dates = state.news_dates.get(session_id);
                let matches = engine.find_matches(
                    &normalized, top_k.min(10), session, (timeframe_seconds / 60) as u32, valid_dates
                );
                let (summary, custom_matches) = calculate_aggregates(&matches, session_open, last_close);
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
        let valid_dates = state.news_dates.get(session_id);
        let matches = engine.find_context_matches(&ctx, top_k.min(10), session, timestamp, valid_dates);
        let (summary, custom_matches) = calculate_aggregates(&matches, session_open, session_open);
        CachedResult::from_aggregates(summary, custom_matches)
    })
}

pub fn calculate_aggregates(matches: &[MatchOutcome], session_open: f32, last_close: f32) -> (AggregateData, Vec<MatchResponse>) {
    // Session metrics are calculated from session_open baseline
    // Next candle metrics are calculated from last_close baseline
    let response_matches: Vec<MatchResponse> = matches.iter().map(|m| {
        let up = (m.mfe_pct / 100.0) * session_open;
        let down = (m.mae_pct / 100.0) * session_open;
        let eos = (m.eod_ret_pct / 100.0) * session_open;
        let eoh_pts = (m.eoh_pct / 100.0) * session_open;
        let next_up_pts = (m.next_candle_up_pct.unwrap_or(0.0) / 100.0) * last_close;
        let next_down_pts = (m.next_candle_down_pct.unwrap_or(0.0) / 100.0) * last_close;

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

    // Adjust session averages to be relative to last_close instead of session_open
    let price_offset = last_close - session_open;
    let adjusted_avg_up = proj.avg_up - price_offset;
    let adjusted_avg_down = proj.avg_down + price_offset;

    // Recalculate edge_ratio based on adjusted values
    let adjusted_edge_ratio = {
        let long_edge = adjusted_avg_up / (if adjusted_avg_down == 0.0 { 1.0 } else { adjusted_avg_down });
        let short_edge = adjusted_avg_down / (if adjusted_avg_up == 0.0 { 1.0 } else { adjusted_avg_up });
        long_edge.max(short_edge)
    };

    (
        AggregateData {
            avg_up: adjusted_avg_up,
            avg_down: adjusted_avg_down,
            avg_eos: median(&mut eos_values),
            avg_eoh: median(&mut eoh_values),
            edge_ratio: adjusted_edge_ratio,
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