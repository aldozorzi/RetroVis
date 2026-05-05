use axum::{
    extract::{State, Query},
    routing::{post, get},
    Json,
    Router,
    response::{Html, IntoResponse},
    http::StatusCode,
};
use std::sync::Arc;
use std::collections::HashMap;
use chrono::NaiveTime;
use serde_json::json;
use dotenvy::dotenv;
use std::env;
use chrono::DateTime;

mod models;
mod parser;
mod engine;
mod cache;
mod gemini;
//mod ollama;
mod ai_analysis;
mod oanda;
mod full_search;
mod ai_provider;

use models::{QueryRequest,AppState,MatchesResponse,MatchResponse, MatchOutcome,ContextQueryRequest,Context,AggregateData,CachedResult,NormalizedCandle,AiAnalysisRequest,CandleCacheKey,AiAnalysisService,FullQueryRequest,FullResponse, RawCandle};
use crate::ai_provider::AiProvider;
use crate::oanda::candle_start_utc_ny_row;
use gemini::GeminiBroker;
//use ollama::OllamaBroker;
use engine::SearchEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    clear_terminal();
    dotenv().ok();
    let mut engines = HashMap::new();

    let nq_data = parser::load_and_process("data/nq-M5.csv","NQ")?;
    engines.insert("NQ".to_string(), SearchEngine::new(nq_data));

    let gc_data = parser::load_and_process("data/gc-M5.csv","GC")?;
    engines.insert("GC".to_string(), SearchEngine::new(gc_data));

    let mut sessions = HashMap::new();
    sessions.insert("LONDON_OPEN".to_string(), models::SessionConfig {
        id: "LONDON_OPEN".to_string(),
        start_time: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        timezone: "Europe/London".to_string(),
    });
    sessions.insert("NY_OPEN".to_string(), models::SessionConfig {
        id: "NY_OPEN".to_string(),
        start_time: NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        timezone: "America/New_York".to_string(),
    });

    // Passala al broker
    let cache = Arc::new(cache::RetroVisCache::new());
    let broker: Arc<dyn AiProvider> = Arc::new(GeminiBroker::new(env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY")));
    //let broker: Arc<dyn AiProvider> = Arc::new(OllamaBroker::new());
    let ai_service = Arc::new(AiAnalysisService::new(
        broker.clone(), 
        cache.clone()
    ));

    let oanda_key = match env::var("OANDA_API_KEY") {
        Ok(k) => k,
        Err(_) => return Err("OANDA_API_KEY mancante".into()),
    };
    let oanda_url = env::var("OANDA_REST_URL").ok();
    let oanda = Arc::new(oanda::OandaClient::new(oanda_key, oanda_url));

    let shared_state = Arc::new(AppState {
        engines,
        sessions,
        cache,
        broker: broker,
        ai_service: ai_service,
        oanda,
    });

    let app = Router::new()
        .route("/search", post(search_handler))
        .route("/full-search", post(full_search_handler))
        .route("/day-details", get(day_details_handler))
        .route("/visualize", get(visualize_handler))
        .route("/context_matches", post(context_matches_handler))
        .route("/get_ai_analysis", post(ai_analysis_handler))
        .with_state(shared_state);
    println!("Server in ascolto su http://127.0.0.1:9999");
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 9999));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    

    Ok(())
}
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Impossibile installare il gestore CTRL+C");
    println!("\nChiusura del server in corso...");
}
async fn full_search_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FullQueryRequest>,
) -> impl IntoResponse {
    let tf_sec = cache::RetroVisCache::timeframe_to_seconds(&req.timeframe);
    let top_k_pattern = req.top_k.min(24);
    let top_k_ctx = req.top_k.min(10);
    let utc_ts = match full_search::ny_local_epoch_to_utc(req.timestamp) {
        Some(dt) => dt.timestamp(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "timestamp NY non valido o inesistente (transizione DST)" })),
            ).into_response();
        }
    };

    if state.engines.get(&req.symbol).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "simbolo non disponibile per il motore storico" })),
        )
            .into_response();
    }

    let min_bars = 450usize;
    let candles = match state
        .oanda
        .fetch_candles_for_full_search(&req.symbol, &req.timeframe, utc_ts, min_bars)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let candles: Vec<RawCandle<f32>> = candles.into_iter().map(|mut c| {
        c.open  = (c.open  * 4.0).round() / 4.0;
        c.high  = (c.high  * 4.0).round() / 4.0;
        c.low   = (c.low   * 4.0).round() / 4.0;
        c.close = (c.close * 4.0).round() / 4.0;
        c
    }).collect();

    let ref_idx = match full_search::find_reference_index(&candles, utc_ts, tf_sec) {
        Ok(i) => i,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let ref_start = match candle_start_utc_ny_row(&candles[ref_idx]) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let session_hint = match full_search::resolve_auto_session_id(ref_start) {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "nessuna sessione attiva (AUTO) per questo istante" })),
            )
                .into_response();
        }
    };

    let open_idx = match full_search::find_session_open_index_backward(&candles, ref_idx, session_hint, 200) {
        Ok(i) => i,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let open_start = match candle_start_utc_ny_row(&candles[open_idx]) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let effective_session = full_search::effective_session_from_open_bar(open_start);
    let session_id = effective_session.to_string();

    let pattern_end_idx = if ref_start.timestamp() == utc_ts {
        (ref_idx + 1).min(candles.len())
    } else {
        ref_idx
    };

    let pattern = match full_search::pattern_candles_exclusive_ref(&candles, open_idx, pattern_end_idx) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    /*
    // DEBUG CANDELE SESSIONE PATTERN
    for (i, c) in pattern.iter().enumerate() {
        if let Ok(t) = candle_start_utc_ny_row(c) {
            println!("[pattern {:>3}] UTC={} O={:.2} H={:.2} L={:.2} C={:.2}",
                i, t.format("%Y-%m-%d %H:%M"), c.open, c.high, c.low, c.close);
        }
    }
    println!("pattern len={}",
        pattern.len());
    // FINE DEBUG CANDELE SESSIONE PATTERN
    // --------------------------------
    */
    if pattern.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "serve almeno una candela dopo l'apertura di sessione" })),
        )
            .into_response();
    }

    let session_open = candles[open_idx].open;
    let last_pat_start = match candle_start_utc_ny_row(&pattern[pattern.len() - 1]) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    // Ancoraggio cache come in Quantower: timestamp dell'ultima candela nel pattern (bar open UTC).
    let search_anchor_ts = last_pat_start.timestamp();

    let mut last_matches_response: Option<MatchesResponse> = None;
    let timeframe_seconds = tf_sec;

    for i in 1..=pattern.len() {
        let historical_slice = &pattern[0..i];
        let current_ts =
            search_anchor_ts - ((pattern.len() - i) as i64 * timeframe_seconds);

        let cached_res = state.cache.get_or_calculate_candle(
            req.symbol.clone(),
            req.timeframe.clone(),
            session_id.clone(),
            current_ts,
            || {
                let engine = match state.engines.get(&req.symbol) {
                    Some(e) => e,
                    None => {
                        return CachedResult::from_aggregates(
                            AggregateData {
                                avg_up: 0.0,
                                avg_down: 0.0,
                                avg_eos: 0.0,
                                edge_ratio: 0.0,
                            },
                            vec![],
                        );
                    }
                };
                let session = match state.sessions.get(&session_id) {
                    Some(s) => s,
                    None => {
                        return CachedResult::from_aggregates(
                            AggregateData {
                                avg_up: 0.0,
                                avg_down: 0.0,
                                avg_eos: 0.0,
                                edge_ratio: 0.0,
                            },
                            vec![],
                        );
                    }
                };

                let normalized: Vec<NormalizedCandle<f32>> = historical_slice
                    .iter()
                    .map(|raw| NormalizedCandle::from_raw(raw, session_open))
                    .collect();

                let matches = engine.find_matches(&normalized, top_k_pattern, session);
                let (summary, custom_matches) = calculate_aggregates(&matches, session_open);

                CachedResult::from_aggregates(summary, custom_matches)
            },
        );

        last_matches_response = Some(MatchesResponse::from(cached_res));
    }

    let (summary, matches) = match last_matches_response {
        Some(m) => (m.summary, m.matches),
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "nessun risultato pattern" })),
            )
                .into_response();
        }
    };

    let ctx_inputs = match full_search::compute_context_inputs(&candles, open_idx, effective_session) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let ctx_date = ctx_inputs.context_cache_date.clone();
    let symbol = req.symbol.clone();

    let context_cached = state.cache.get_or_calculate_context(&symbol, &session_id, &ctx_date, || {
        let Some(engine) = state.engines.get(&req.symbol) else {
            return CachedResult::from_aggregates(
                AggregateData {
                    avg_up: 0.0,
                    avg_down: 0.0,
                    avg_eos: 0.0,
                    edge_ratio: 0.0,
                },
                vec![],
            );
        };
        let Some(session) = state.sessions.get(&session_id) else {
            return CachedResult::from_aggregates(
                AggregateData {
                    avg_up: 0.0,
                    avg_down: 0.0,
                    avg_eos: 0.0,
                    edge_ratio: 0.0,
                },
                vec![],
            );
        };

        let current_context = Context::new(
            ctx_inputs.session_open,
            ctx_inputs.prev_close,
            ctx_inputs.prev_high,
            ctx_inputs.prev_low,
            ctx_inputs.intraday_high,
            ctx_inputs.intraday_low,
        );

        let matches = engine.find_context_matches(&current_context, top_k_ctx, session);
        let (summary, custom_matches) = calculate_aggregates(&matches, ctx_inputs.session_open);
        CachedResult::from_aggregates(summary, custom_matches)
    });

    let context_response = MatchesResponse::from(context_cached);

    let last_close = pattern[pattern.len() - 1].close as f64;
    let key = CandleCacheKey {
        asset: req.symbol.clone(),
        timeframe: req.timeframe.clone(),
        session_id: session_id.clone(),
        timestamp: search_anchor_ts,
    };

    let full_history = state.cache.get_session_history(
        &req.symbol,
        &req.timeframe,
        &session_id,
        search_anchor_ts,
        pattern.len(),
    );

    let context_res = state
        .cache
        .get_context(&req.symbol, &session_id, &ctx_date);

    let ai_data = json!({
        "symbol": req.symbol,
        "session_id": session_id,
        "current_price_action": last_close,
        "evolution_history": full_history,
        "context": context_res,
        "session_open": session_open as f64,
    });

    let ai_analysis = match state.ai_service.get_analysis(key, ai_data).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("full-search AI: {}", e);
            String::new()
        }
    };

    Json(FullResponse {
        summary,
        matches,
        context: context_response,
        ai_analysis,
    })
    .into_response()
}

async fn search_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest<f32>>,
) -> Json<MatchesResponse> {
    /*
    // --- LOG DI DEBUG ---
    println!("--- NUOVA RICHIESTA RICEVUTA ---");
    println!("Simbolo: {}", req.symbol);
    println!("Sessione: {}", req.session_id);
    println!("Numero candele inviate: {}", req.candles.len());
    println!("Span: {} - {}", req.candles.first().unwrap().time, req.candles.last().unwrap().time);
    
    if let Some(first) = req.candles.first() {
        println!("Prima candela inviata: {} {}", first.date, first.time);
    }
    // --------------------
    */

    let mut last_result = None;
    let timeframe_seconds = crate::cache::RetroVisCache::timeframe_to_seconds(&req.timeframe);

    for i in 1..=req.candles.len() {
        let historical_slice = &req.candles[0..i];
        let current_ts = req.timestamp - ((req.candles.len() - i) as i64 * timeframe_seconds);

        let cached_res = state.cache.get_or_calculate_candle(
            req.symbol.clone(),
            req.timeframe.clone(),
            req.session_id.clone(),
            current_ts,
            || {

                let engine = state.engines.get(&req.symbol).expect("Ticker not found");
                let session = state.sessions.get(&req.session_id).expect("Session not found");

                let normalized: Vec<NormalizedCandle<f32>> = historical_slice
                    .iter()
                    .map(|raw| NormalizedCandle::from_raw(raw, req.session_open))
                    .collect();

                let matches = engine.find_matches(&normalized, req.top_k.min(10), session);
                let (summary, custom_matches) = calculate_aggregates(&matches, req.session_open);

                CachedResult::from_aggregates(summary, custom_matches)
            }
        );

        last_result = Some(cached_res);
    }
    let response = MatchesResponse::from(last_result.expect("No candles provided"));
    
    Json(response)
}

pub async fn ai_analysis_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AiAnalysisRequest>,
) -> impl IntoResponse {

    let key = CandleCacheKey {
        asset: req.symbol.clone(),
        timeframe: req.timeframe.clone(),
        session_id: req.session_id.clone(),
        timestamp: req.timestamp,
    };

    let full_history = state.cache.get_session_history(
        &req.symbol,
        &req.timeframe,
        &req.session_id,
        req.timestamp,
        req.candles_len
    );

    let date_str = DateTime::from_timestamp(req.timestamp, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default();

    let context_res = state.cache.get_context(
        &req.symbol,
        &req.session_id,
        &date_str
    );

    let ai_data = json!({
        "symbol": req.symbol,
        "session_id": req.session_id,
        "current_price_action": req.current_price_action,
        "evolution_history": full_history,
        "context": context_res,
        "session_open": req.session_open,
    });

    // Delegate the analysis to the AiAnalysisService, which will handle caching, coalescing and provider calls internally.
    match state.ai_service.get_analysis(key, ai_data).await {
        Ok(analysis) => {
            Json(json!({ 
                "status": "success", 
                "analysis": analysis 
            }))
        },
        Err(e) => {
            eprintln!("Errore durante l'analisi AI: {}", e);
            Json(json!({ 
                "status": "error", 
                "message": format!("Errore analisi: {}", e) 
            }))
        }
    }
}

async fn day_details_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<Vec<models::ChartDataResponse>> {
    let symbol = params.get("symbol").cloned().unwrap_or_else(|| "NQ".to_string());
    let date = params.get("date").cloned().unwrap_or_default();

    if let Some(engine) = state.engines.get(&symbol) {
        return Json(engine.get_day_candles(&date,true));
    }

    Json(vec![])
}

async fn context_matches_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ContextQueryRequest>,
) -> Json<MatchesResponse> {
    /*
    println!(">>> /context_matches ricevuto: symbol={}, session={}", req.symbol, req.session_id);
    println!("    session_open={}, prev_close={}, prev_high={}, prev_low={}", req.session_open, req.prev_close, req.prev_high, req.prev_low);
    println!("    intraday_high={}, intraday_low={}", req.intraday_high, req.intraday_low);
    */
    let symbol = req.symbol.clone();
    let session_id = req.session_id.clone();
    let date = chrono::Local::now().format("%Y-%m-%d").to_string(); 

    let cached_res = state.cache.get_or_calculate_context(
        &symbol, 
        &session_id, 
        &date, 
        || {
            let engine = state.engines.get(&req.symbol).expect("Ticker not found");
            let session = state.sessions.get(&req.session_id).expect("Session not found");

            let current_context = Context::new(
                req.session_open, req.prev_close, req.prev_high, 
                req.prev_low, req.intraday_high, req.intraday_low,
            );

            let matches = engine.find_context_matches(&current_context, req.top_k.min(10), session);

            let (summary, custom_matches) = calculate_aggregates(&matches, req.session_open);

            CachedResult::from_aggregates(summary, custom_matches)
        }
    );

    Json(MatchesResponse::from(cached_res))
}

fn calculate_aggregates(matches: &[MatchOutcome], current_price: f32) -> (AggregateData, Vec<MatchResponse>) {
    let mut sum_mfe = 0.0;
    let mut sum_mae = 0.0;
    let mut sum_eos = 0.0;

    let matches: Vec<MatchResponse> = matches.iter().map(|m| {
        let up = (m.mfe_pct / 100.0) * current_price;
        let down = (m.mae_pct / 100.0) * current_price;
        let eos = (m.eod_ret_pct / 100.0) * current_price;
        
        sum_mfe += up.abs();
        sum_mae += down.abs();
        sum_eos += eos;

        MatchResponse {
            date: m.date.clone(),
            time: m.time.clone(),
            similarity: 1.0 / (1.0 + m.distance) * 100.0,
            up_pts: up,
            down_pts: down,
            eos_pts: eos,
        }
    }).collect();

    let count = matches.len() as f32;
    let avg_up = if count > 0.0 { sum_mfe / count } else { 0.0 };
    let avg_down = if count > 0.0 { sum_mae / count } else { 0.0 };
    let avg_eos = if count > 0.0 { sum_eos / count } else { 0.0 };
    
    let long_edge = avg_up / (if avg_down == 0.0 { 1.0 } else { avg_down });
    let short_edge = avg_down / (if avg_up == 0.0 { 1.0 } else { avg_up });

    (
        AggregateData {
            avg_up,
            avg_down,
            avg_eos,
            edge_ratio: long_edge.max(short_edge),
        },
        matches
    )
}

async fn visualize_handler() -> Html<String> {
    Html(r#"
<!DOCTYPE html>
<html>
<head>
    <title>RetroVis Historical Chart</title>
    <script src="https://unpkg.com/lightweight-charts/dist/lightweight-charts.standalone.production.js"></script>
    <style>
        body { background: #131722; margin: 0; padding: 0; overflow: hidden; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; }
        #chart { height: 100vh; width: 100vw; }
        #info { position: absolute; top: 15px; left: 15px; z-index: 10; color: #d1d4dc; background: rgba(28, 32, 48, 0.8); padding: 8px 12px; border-radius: 4px; border: 1px solid #363c4e; pointer-events: none; }
    </style>
</head>
<body>
    <div id="info">Caricamento...</div>
    <div id="chart"></div>
    <script>
        const urlParams = new URLSearchParams(window.location.search);
        const dateParam = urlParams.get('date');
        const symbolParam = urlParams.get('symbol') || 'NQ';
        
        if (dateParam) document.getElementById('info').innerText = `${symbolParam} - ${dateParam}`;

        const chart = LightweightCharts.createChart(document.getElementById('chart'), {
            layout: { background: { color: '#131722' }, textColor: '#d1d4dc' },
            grid: { vertLines: { color: '#2b2e3a' }, horzLines: { color: '#2b2e3a' } },
            timeScale: { timeVisible: true, secondsVisible: false, borderColor: '#485c7b' },
            rightPriceScale: { borderColor: '#485c7b' },
            timeScale: {
                timeVisible: true,
                secondsVisible: false,
            },
        });

        const candleSeries = chart.addSeries(LightweightCharts.CandlestickSeries, {
            upColor: '#26a69a', 
            downColor: '#ef5350', 
            borderVisible: false,
            wickUpColor: '#26a69a', 
            wickDownColor: '#ef5350'
        });

        fetch(`/day-details?symbol=${symbolParam}&date=${dateParam}`)
            .then(res => res.json())
            .then(data => {
                if (!data || data.length === 0) {
                    document.getElementById('info').innerText = "Dati non trovati per: " + dateParam;
                    return;
                }
                
                const formattedData = data.map(d => {

                const parts = d.time.split(' ');
                const datePart = parts[0]; // 2023-11-09
                const timePart = parts[1]; // 09:30:00

                const dt = new Date(`${datePart}T${timePart}Z`); 
                
                return {
                    time: Math.floor(dt.getTime() / 1000), 
                    open: d.open, 
                    high: d.high, 
                    low: d.low, 
                    close: d.close
                };
            }).sort((a, b) => a.time - b.time);
                
                candleSeries.setData(formattedData);

                const dateGroups = {};
                formattedData.forEach(d => {
                    const dateKey = new Date(d.time * 1000).toISOString().slice(0, 10);
                    if (!dateGroups[dateKey]) dateGroups[dateKey] = [];
                    dateGroups[dateKey].push(d);
                });

                markers = [];
                Object.keys(dateGroups).forEach(dateKey => {
                    const dayCandles = dateGroups[dateKey];

                    const nyOpenCandle = dayCandles.find(d => {
                        const date = new Date(d.time * 1000);
                        return date.getUTCHours() === 9 && date.getUTCMinutes() === 30;
                    });

                    const londonOpenTime = getLondonOpenInNY(dateKey);
                    const londonOpenCandle = dayCandles.find(d => {
                        const date = new Date(d.time * 1000);
                        const timeStr = date.getUTCHours().toString().padStart(2, '0') + ":" +
                                        date.getUTCMinutes().toString().padStart(2, '0') + ":00";
                        return timeStr === londonOpenTime;
                    });

                    if (londonOpenCandle) {
                        markers.push({
                            time: londonOpenCandle.time,
                            position: 'aboveBar',
                            color: '#f6d32d',
                            shape: 'arrowDown',
                            text: 'LONDON OPEN',
                            size: 2
                        });
                    }

                    if (nyOpenCandle) {
                        markers.push({
                            time: nyOpenCandle.time,
                            position: 'aboveBar',
                            color: '#4caf50',
                            shape: 'arrowDown',
                            text: 'NY OPEN',
                            size: 2
                        });
                    }
                });

                markers.sort((a, b) => a.time - b.time);
                LightweightCharts.createSeriesMarkers(candleSeries, markers);
                const secondDayStart = formattedData.find(d => {
                    const dateKey = new Date(d.time * 1000).toISOString().slice(0, 10);
                    return dateKey === dateParam;
                });

                if (secondDayStart) {
                    chart.timeScale().setVisibleRange({
                        from: secondDayStart.time - 1800, // 30 min prima dell'apertura del secondo giorno
                        to: formattedData[formattedData.length - 1].time
                    });
                } else {
                    chart.timeScale().fitContent();
                }
            })
            .catch(err => {
                document.getElementById('info').innerText = "Errore di connessione al server Rust";
                console.error(err);
            });

            function getLondonOpenInNY(dateStr) {
                // 1. Creiamo un istante temporale "neutro" (UTC) per le 08:00 di quel giorno
                const baseDate = new Date(`${dateStr}T08:00:00Z`);

                // 2. Funzione per ottenere l'offset in minuti di un fuso rispetto a UTC
                const getOffset = (tz, date) => {
                    const parts = new Intl.DateTimeFormat('en-US', {
                        timeZone: tz,
                        timeZoneName: 'short',
                        year: 'numeric', month: 'numeric', day: 'numeric',
                        hour: 'numeric', minute: 'numeric', second: 'numeric',
                        hour12: false
                    }).formatToParts(date);
                    
                    // Calcoliamo la differenza tra l'ora "trasformata" e quella UTC
                    const dt = new Date(date.toLocaleString('en-US', { timeZone: tz }));
                    return (dt - date) / 60000;
                };

                // 3. Calcoliamo gli offset reali di Londra e NY rispetto a UTC in quel momento
                const londonOffset = getOffset('Europe/London', baseDate);
                const nyOffset = getOffset('America/New_York', baseDate);

                // 4. Se Londra è a +0 (GMT) e NY è a -4 (EDT), la differenza è 4 ore.
                // L'apertura di Londra (08:00) meno la differenza ci dà l'ora di NY.
                const diffHours = (londonOffset - nyOffset) / 60;
                const targetHour = 8 - diffHours;

                // Formattiamo il risultato come HH:MM:00
                return `${String(Math.floor(targetHour)).padStart(2, '0')}:00:00`;
            }
    </script>
</body>
</html>
"#.to_string())
}

fn clear_terminal() {
    print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
}