use axum::{
    extract::{State, Query},
    routing::{post, get},
    Json,
    Router,
    response::{Html, IntoResponse},
};
use std::sync::Arc;
use std::collections::HashMap;
use chrono::NaiveTime;
use serde_json::json;
use dotenvy::dotenv;
use std::env;
use std::thread;

mod models;
mod parser;
mod engine;
mod cache;
mod gemini;
mod stats; 
mod determination_engine;
mod ai_analysis;
mod ai_provider;
mod http_client;

use models::{QueryRequest,AppState,MatchesResponse,MatchResponse, MatchOutcome,ContextQueryRequest,Context,AggregateData,CachedResult,NormalizedCandle,AiAnalysisRequest,CandleCacheKey,AiAnalysisService, RawCandle, SessionConfig};
use stats::{median, edge_ratio, bias_pct}; 
use crate::ai_provider::AiProvider;
use gemini::GeminiBroker;
use engine::SearchEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    clear_terminal();
    println!("----- STARTING RetroVis SERVER -----");
    dotenv().ok();
    let mut engines = HashMap::new();

    let nq = thread::spawn(|| parser::load_and_process("data/nq-M5.csv", "NQ").map_err(|e| e.to_string()));
    let gc = thread::spawn(|| parser::load_and_process("data/gc-M5.csv", "GC").map_err(|e| e.to_string()));
    let ym = thread::spawn(|| parser::load_and_process("data/ym-M5.csv", "YM").map_err(|e| e.to_string()));
    
    let nq_5m = nq.join().unwrap()?;
    engines.insert("NQ".to_string(), SearchEngine::new(nq_5m));

    let gc_5m = gc.join().unwrap()?;
    engines.insert("GC".to_string(), SearchEngine::new(gc_5m));  

    let ym_5m = ym.join().unwrap()?;
    engines.insert("YM".to_string(), SearchEngine::new(ym_5m));

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

    let cache = Arc::new(cache::RetroVisCache::new());
    let broker: Arc<dyn AiProvider> = Arc::new(GeminiBroker::new(env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY")));
    let ai_service = Arc::new(AiAnalysisService::new(
        broker.clone(), 
        cache.clone()
    ));
    
    let shared_state = Arc::new(AppState {
        engines,
        sessions,
        cache,
        broker: broker,
        ai_service: ai_service,
        //oanda,
    });

    let app = Router::new()
        .route("/search", post(search_handler))
        .route("/day-details", get(day_details_handler))
        .route("/visualize", get(visualize_handler))
        .route("/context-search", post(context_matches_handler))
        .route("/get-ai-analysis", post(ai_analysis_handler))
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

async fn search_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Json<MatchesResponse> {
    let history = compute_candle_history(
        &state, &req.symbol, &req.timeframe, &req.session_id,
        req.timestamp, &req.candles, req.session_open, req.top_k,
    );
    Json(MatchesResponse::from(history.into_iter().last().expect("No candles provided")))
}

async fn context_matches_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ContextQueryRequest>,
) -> Json<MatchesResponse> {
    let result = compute_context(
        &state, &req.symbol, &req.session_id,
        req.timestamp,          // era Utc::now() — bug corretto
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

    // Cache-aside: usa la cache se disponibile, altrimenti calcola autonomamente
    let full_history = {
        let h = state.cache.get_session_history(
            &req.symbol, &req.timeframe, req.timestamp, req.candles.len()
        );
        if h.is_empty() {
            compute_candle_history(
                &state, &req.symbol, &req.timeframe, &req.session_id,
                req.timestamp, &req.candles, req.session_open as f32, req.top_k,
            )
        } else { h }
    };
    let context_res = match state.cache.get_context(&req.symbol, req.timestamp) {
        Some(c) => c,
        None => compute_context(
            &state, &req.symbol, &req.session_id, req.timestamp,
            req.session_open as f32, req.prev_close, req.prev_high,
            req.prev_low, req.intraday_high, req.intraday_low, req.top_k,
        ),
    };

    let ai_data = json!({
        "symbol": req.symbol,
        "session_id": req.session_id,
        "current_price_action": req.current_price_action,
        "evolution_history": full_history,
        "context": context_res,
        "session_open": req.session_open,
    });

    match state.ai_service.get_analysis(key, ai_data).await {
        Ok(analysis) => Json(json!({ "status": "success", "analysis": analysis })),
        Err(e) => {
            eprintln!("Errore analisi AI: {}", e);
            Json(json!({ "status": "error", "message": format!("Errore analisi: {}", e) }))
        }
    }
}

fn compute_candle_history(
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

fn compute_context(
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
        /*println!("Calcolo contesto per {} - {} at {}: session_open={}, prev_close={}, prev_high={}, prev_low={}, intraday_high={}, intraday_low={}",
            symbol, session_id, timestamp, session_open, prev_close, prev_high, prev_low, intraday_high, intraday_low
        );*/
        let matches = engine.find_context_matches(&ctx, top_k.min(10), session, timestamp);
        let (summary, custom_matches) = calculate_aggregates(&matches, session_open);
        CachedResult::from_aggregates(summary, custom_matches)
    })
}

fn calculate_aggregates(matches: &[MatchOutcome], current_price: f32) -> (AggregateData, Vec<MatchResponse>) {
    let mut mfe_values: Vec<f32> = Vec::new();
    let mut mae_values: Vec<f32> = Vec::new();
    let mut eos_values: Vec<f32> = Vec::new();
    let mut eoh_values: Vec<f32> = Vec::new();
    let mut up_down_pairs: Vec<(f32, f32)> = Vec::new();
    let mut response_matches = Vec::with_capacity(matches.len());

    for m in matches.iter() {
        let up = (m.mfe_pct / 100.0) * current_price;
        let down = (m.mae_pct / 100.0) * current_price;
        let eos = (m.eod_ret_pct / 100.0) * current_price;
        let eoh_pts = (m.eoh_pct / 100.0) * current_price;
        let next_up_pts = (m.next_candle_up_pct.unwrap_or(0.0) / 100.0) * current_price;
        let next_down_pts = (m.next_candle_down_pct.unwrap_or(0.0) / 100.0) * current_price;

        up_down_pairs.push((up, down.abs()));

        mfe_values.push(up.abs());
        mae_values.push(down.abs());
        eos_values.push(eos);
        eoh_values.push(eoh_pts);

        response_matches.push(MatchResponse {
            timestamp: m.timestamp,
            similarity: 1.0 / (1.0 + m.distance) * 100.0,
            up_pts: up,
            down_pts: down,
            eos_pts: eos,
            eoh_pts,
            next_candle_up: next_up_pts,
            next_candle_down: next_down_pts,
        });
    }

    let avg_up   = median(&mut mfe_values);
    let avg_down = median(&mut mae_values);
    let avg_eos  = median(&mut eos_values);
    let avg_eoh  = median(&mut eoh_values);

    (
        AggregateData {
            avg_up,
            avg_down,
            avg_eos,
            avg_eoh,
            edge_ratio: edge_ratio(avg_up, avg_down),
            bias_pct: bias_pct(up_down_pairs),
        },
        response_matches
    )
}

fn lookup_engine_and_session<'a>(
    state: &'a AppState,
    symbol: &str,
    session_id: &str,
) -> (&'a SearchEngine, &'a SessionConfig) {
    let engine = state.engines.get(symbol).expect("Ticker not found");
    let session = state.sessions.get(session_id).expect("Session not found");
    (engine, session)
}

async fn day_details_handler(
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
        const timeframeParam = urlParams.get('timeframe') || 'M5';
        
        if (dateParam) document.getElementById('info').innerText = `${symbolParam} - ${dateParam}`;

        const chart = LightweightCharts.createChart(document.getElementById('chart'), {
            layout: { background: { color: '#131722' }, textColor: '#d1d4dc' },
            grid: { vertLines: { color: '#2b2e3a' }, horzLines: { color: '#2b2e3a' } },
            crosshair: {
                mode: LightweightCharts.CrosshairMode.Normal,
            },
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

        console.log(`/day-details?symbol=${symbolParam}&date=${dateParam}&timeframe=${timeframeParam}`);
        fetch(`/day-details?symbol=${symbolParam}&date=${dateParam}&timeframe=${timeframeParam}`)
            .then(res => res.json())
            .then(data => {
                if (!data || data.length === 0) {
                    document.getElementById('info').innerText = "Dati non trovati per: " + dateParam;
                    return;
                }
                
                const formattedData = data.map(d => {
                    return {
                        time: Number(d.timestamp), 
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