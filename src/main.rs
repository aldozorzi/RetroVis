use axum::{
    routing::{post, get},
    Router,
};
use std::sync::Arc;
use std::collections::HashMap;
use chrono::NaiveTime;
use dotenvy::dotenv;
use std::thread;

mod models;
mod parser;
mod engine;
mod cache;
mod stats;

mod service;
mod handlers;
mod chart_data;
mod visualize;


use models::AppState;
use engine::SearchEngine;
use handlers::{context_matches_handler, day_details_handler, search_handler};
use visualize::visualize_handler;

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

    let shared_state = Arc::new(AppState {
        engines,
        sessions,
        cache,
    });

    let app = Router::new()
        .route("/search", post(search_handler))
        .route("/day-details", get(day_details_handler))
        .route("/visualize", get(visualize_handler))
        .route("/context-search", post(context_matches_handler))
        .with_state(shared_state);
    println!("Server listening at http://127.0.0.1:9999");
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

fn clear_terminal() {
    print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
}
