use axum::{
    extract::State,
    routing::post,
    Json,
    Router,
};
use std::sync::Arc;
use std::collections::HashMap;
use chrono::NaiveTime;

mod models;
mod parser;
mod engine;

use models::{QueryRequest,AppState,MatchResponse};
use engine::SearchEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engines = HashMap::new();

    // Carichiamo i vari asset (NQ, GC, ecc.)
    // In futuro questi percorsi possono essere letti da un file di config
    let nq_data = parser::load_and_process("data/nasdaq_m5.csv")?;
    engines.insert("NQ".to_string(), SearchEngine::new(nq_data));

    /*let gc_data = parser::load_and_process("data/gold_m5.csv")?;
    engines.insert("GC".to_string(), SearchEngine::new(gc_data));*/

    // Configurazione sessioni (rimane invariata)
    let mut sessions = HashMap::new();
    sessions.insert("LONDON_OPEN".to_string(), models::SessionConfig {
        id: "LONDON_OPEN".to_string(),
        start_time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
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
    });

    let app = Router::new()
        .route("/search", post(search_handler))
        .with_state(shared_state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 9999));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Handler per le richieste POST dall'indicatore C#
async fn search_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<QueryRequest<f32>>,
) -> Json<Vec<MatchResponse>> {
    
    // --- LOG DI DEBUG ---
    println!("--- NUOVA RICHIESTA RICEVUTA ---");
    println!("Simbolo: {}", payload.symbol);
    println!("Sessione: {}", payload.session_id);
    println!("Numero candele inviate: {}", payload.candles.len());
    println!("Span: {} - {}", payload.candles.first().unwrap().time, payload.candles.last().unwrap().time);
    
    if let Some(first) = payload.candles.first() {
        println!("Prima candela inviata: {} {}", first.date, first.time);
    }
    // --------------------

    // 1. SELEZIONE DEL MOTORE IN BASE AL SYMBOL (Risolve il warning)
    let engine = match state.engines.get(&payload.symbol) {
        Some(e) => e,
        None => {
            eprintln!("Simbolo non supportato: {}", payload.symbol);
            return Json(vec![]);
        }
    };

    // 2. Selezione della sessione
    let session_config = match state.sessions.get(&payload.session_id) {
        Some(config) => config,
        None => return Json(vec![]),
    };

    if payload.candles.is_empty() { return Json(vec![]); }

    let baseline = payload.candles[0].open;
    let current_pattern: Vec<models::NormalizedCandle<f32>> = payload.candles
        .iter()
        .map(|r| models::NormalizedCandle::from_raw(r, baseline))
        .collect();

    // 3. Eseguiamo la ricerca sul motore specifico dell'asset richiesto
    let matches = engine.find_matches(&current_pattern, 5, session_config);
    
    let response: Vec<MatchResponse> = matches.iter().map(|m| MatchResponse {
        date: m.date.clone(),
        time: m.time.clone(),
        distance: m.distance,
        eod_return: m.eod_ret_pct,
        mfe: m.mfe_pct,
        mae: m.mae_pct,
    }).collect();

    Json(response)
}