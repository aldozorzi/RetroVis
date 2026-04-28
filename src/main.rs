use axum::{
    extract::{State, Query},
    routing::{post, get},
    Json,
    Router,
    response::{Html}
};
use std::sync::Arc;
use std::collections::HashMap;
use chrono::NaiveTime;

mod models;
mod parser;
mod engine;
mod cache;

use models::{QueryRequest,AppState,FullResponse,MatchResponse, MatchOutcome,ContextQueryRequest,Context,AggregateData,CachedResult,NormalizedCandle};
use engine::SearchEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    clear_terminal();
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

    let shared_state = Arc::new(AppState {
        engines,
        sessions,
        cache: Arc::new(crate::cache::RetroVisCache::new()),
    });

    let app = Router::new()
        .route("/search", post(search_handler))
        .route("/day-details", get(day_details_handler))
        .route("/visualize", get(visualize_handler))
        .route("/context_matches", post(context_matches_handler))
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
    Json(req): Json<QueryRequest<f32>>,
) -> Json<FullResponse> {
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
        println!("Recupero dati");
        let cached_res = state.cache.get_or_calculate_candle(
            req.symbol.clone(),
            req.timeframe.clone(),
            req.session_id.clone(),
            current_ts,
            || {
                println!("Calcolando");
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
    println!("DAti recuperati, costruzione risposta...");
    Json(FullResponse::from(last_result.expect("No candles provided")))
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
) -> Json<FullResponse> {
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

    Json(FullResponse::from(cached_res))
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