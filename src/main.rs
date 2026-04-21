use axum::{
    extract::State,
    routing::post,
    Json,
    Router,
};
use std::sync::Arc;
use serde::{Serialize};
// Assuming your modules are correctly set up in the project
mod models;
mod parser;
mod engine;

use models::{RawCandle, NormalizedCandle};
use engine::SearchEngine;

/// Data structure for the JSON response to Quantower
#[derive(Serialize)]
pub struct MatchResponse {
    pub date: String,
    pub distance: f32,
    pub eod_return: f32,
    pub mfe: f32,
    pub mae: f32,
}

/// Shared application state across threads
struct AppState {
    engine: SearchEngine<f32>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initial Data Loading (Load once at startup)
    let dataset = parser::load_and_process("data/nasdaq_m5.csv", 6)?;
    let shared_state = Arc::new(AppState {
        engine: SearchEngine::new(dataset),
    });

    // 2. Route Definition
    let app = Router::new()
        .route("/search", post(search_handler))
        .with_state(shared_state);

    // 3. Start Server on a custom port (e.g., 9999)
    let addr = "127.0.0.1:9999"; 
    let listener = tokio::net::TcpListener::bind(addr).await?;
    
    println!("--- Pattern Matching Engine Online ---");
    println!("Endpoint: http://{}/search", addr);
    
    axum::serve(listener, app).await?;

    Ok(())
}

/// Handles POST requests from the C# indicator
async fn search_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Vec<RawCandle<f32>>>,
) -> Json<Vec<MatchResponse>> {
    println!("Received {} candles", payload.len());
    // Stampa orari della prima e dell'ultima candela ricevuta
    if payload.len() > 0 {
        let first_time = &payload[0].time;
        let last_time = &payload[payload.len() - 1].time;
        println!("Range: {} -> {}", first_time, last_time);
    }

    if payload.is_empty() {
        return Json(vec![]);
    }

    // Process incoming candles using the centralized normalization logic
    let baseline = payload[0].open;
    let current_pattern: Vec<NormalizedCandle<f32>> = payload
        .iter()
        .map(|r| NormalizedCandle::from_raw(r, baseline))
        .collect();

    // Perform the Euclidean search
    let matches = state.engine.find_matches(&current_pattern, 5);
    
    // Map internal results to JSON-friendly format
    let response: Vec<MatchResponse> = matches.iter().map(|m| MatchResponse {
        date: m.pattern.result.date.clone(),
        distance: m.distance,
        eod_return: m.pattern.result.eod_ret_pct,
        mfe: m.pattern.result.mfe_pct,
        mae: m.pattern.result.mae_pct,
    }).collect();
    println!("Matches found: {}", response.len());
    Json(response)
}