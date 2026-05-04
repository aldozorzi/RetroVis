use async_trait::async_trait;
use serde_json::{json, Value};
use reqwest::Client;
use crate::models::AiProvider;

use std::time::Duration;
use tokio::time::timeout;

pub struct GeminiBroker {
    api_key: String,
    http_client: Client,
}

impl GeminiBroker {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http_client: Client::new(),
        }
    }

    fn format_trading_report(session_data: &Value) -> String {
        let symbol = session_data["symbol"].as_str().unwrap_or("N/A");
        let session = session_data["session_id"].as_str().unwrap_or("N/A");
        let history = &session_data["evolution_history"];
        let context = &session_data["context"];
        let current_price_action = session_data["current_price_action"].as_f64().unwrap_or(0.0);
        let session_open = session_data["session_open"].as_f64().unwrap_or(0.0);

        //println!("session_open:{}", session_open);

        let mut report = format!("### QUANTITATIVE STATISTICAL ANALYSIS - SYMBOL: {}\n", symbol);
        report.push_str(&format!("SESSION: {}\n\n", session));

        if let Some(evolutions) = history.as_array() {
            if let Some(latest) = evolutions.last() {
                let edge_raw = latest["edge_ratio"].as_f64().unwrap_or(0.0);
                let avg_up = latest["avg_up"].as_f64().unwrap_or(0.0);
                let avg_down = latest["avg_down"].as_f64().unwrap_or(0.0).abs();
                
                // Il segno è determinato esclusivamente dal confronto tra spinta Up e Down
                let current_signed_edge = if avg_up >= avg_down { edge_raw.abs() } else { -edge_raw.abs() };
                let current_eos = latest["avg_eos"].as_f64().unwrap_or(0.0);

                report.push_str("--- MANDATORY CURRENT DATA POINTS ---\n");
                report.push_str(&format!("IDENTIFIED_CURRENT_NOW: {:.2}\n", current_price_action));
                report.push_str(&format!("IDENTIFIED_CURRENT_MOVE: {:.2}\n", current_price_action - session_open));
                report.push_str(&format!("IDENTIFIED_CURRENT_EDGE: {:.2}\n", current_signed_edge));
                report.push_str(&format!("IDENTIFIED_CURRENT_EOS: {:.2}\n", current_eos));
                report.push_str(&format!("CURRENT_VOLATILITY_RATIO: {:.1}/{:.1}\n\n", avg_up, -avg_down));
            }

            report.push_str("--- RECENT EVOLUTION HISTORY ---\n");
            for evo in evolutions.iter().rev().take(3).rev() {
                let edge_raw = evo["edge_ratio"].as_f64().unwrap_or(0.0);
                let up = evo["avg_up"].as_f64().unwrap_or(0.0);
                let down = evo["avg_down"].as_f64().unwrap_or(0.0).abs();
                let signed = if up >= down { edge_raw.abs() } else { -edge_raw.abs() };
                
                report.push_str(&format!(
                    "- Edge Ratio: {:.2} | Avg EOS: {:.2} | Up/Down: {:.1}/{:.1}\n",
                    signed,
                    evo["avg_eos"].as_f64().unwrap_or(0.0),
                    up,
                    -down
                ));
            }

            if let Some(latest) = evolutions.last() {
                report.push_str("\n--- HISTORICAL MATCH DETAILS (TOP PRECISION) ---\n");
                if let Some(matches) = latest["matches"].as_array() {
                    for m in matches.iter().take(5) {
                        report.push_str(&format!(
                            "Data: {} | Match: {:.1}% | Up: {:.1} | Down: {:.1} | EOS: {:.1}\n",
                            m["date"].as_str().unwrap_or("N/A"),
                            m["match_score"].as_f64().unwrap_or(0.0),
                            m["up"].as_f64().unwrap_or(0.0),
                            m["down"].as_f64().unwrap_or(0.0),
                            m["eos"].as_f64().unwrap_or(0.0)
                        ));
                    }
                }
            }
        }

        report.push_str("\n--- STRUCTURAL CONTEXT DATA ---");
        let c_edge = context["edge_ratio"].as_f64().unwrap_or(0.0);
        let c_up = context["avg_up"].as_f64().unwrap_or(0.0);
        let c_down = context["avg_down"].as_f64().unwrap_or(0.0).abs();
        let signed_c_edge = if c_up >= c_down { c_edge.abs() } else { -c_edge.abs() };

        report.push_str(&format!("\nCONTEXT_EDGE: {:.2}", signed_c_edge));
        report.push_str(&format!("\nCONTEXT_EOS: {:.2}", context["avg_eos"].as_f64().unwrap_or(0.0)));
        
        report.push_str("\n\nTOP CONTEXT MATCHES:");
        if let Some(matches) = context["matches"].as_array() {
            for m in matches.iter().take(3) {
                report.push_str(&format!(
                    "\n- Date: {} | EOS: {:.1} | Score: {:.1}%",
                    m["date"].as_str().unwrap_or("N/A"),
                    m["eos"].as_f64().unwrap_or(0.0),
                    m["match_score"].as_f64().unwrap_or(0.0)
                ));
            }
        }

        report.push_str("\n\nSTRICT INSTRUCTION: Compare IDENTIFIED_CURRENT_EDGE (Momentum) with CONTEXT_EDGE (Structure). If they disagree, the market is in conflict.");

        report
    }
}

#[async_trait]
impl AiProvider for GeminiBroker {
    async fn request_analysis(&self, data: Value) -> Result<String, String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-lite-preview:generateContent?key={}",
            self.api_key
        );

        let symbol = data["symbol"].as_str().unwrap_or("N/A");
        let report = Self::format_trading_report(&data);
        let payload = json!({
            "systemInstruction": { "parts": { "text": self.system_prompt() } },
            "contents": { "parts": { "text": report } },
            "generationConfig": { "temperature": 0.5 }
        });
        //println!("Report {:#?}", report);
        let backoffs = [500, 1000, 3000];
        
        for (attempt, delay) in backoffs.iter().enumerate() {
            println!("REQ | Symbol: {} | Attempt: {}", symbol, attempt + 1);

            let request_timeout = Duration::from_secs(15);

            let resp = timeout(
                request_timeout, 
                self.http_client.post(&url).json(&payload).send()
            ).await;

            match resp {
                // 1. Il timeout NON è scattato, abbiamo un risultato dalla richiesta HTTP
                Ok(http_result) => {
                    match http_result {
                        Ok(r) => { // Abbiamo una Response valida
                            let status = r.status();
                            if status.is_success() {
                                let json = r.json::<Value>().await.map_err(|e| format!("JSON_PARSE_ERR: {}", e))?;
                                return json["candidates"][0]["content"]["parts"][0]["text"]
                                    .as_str()
                                    .map(|s| s.to_string())
                                    .ok_or_else(|| format!("MALFORMED_RESPONSE: {}", json));
                            }

                            if status.as_u16() == 503 || status.as_u16() == 429 {
                                println!("RETRY | Symbol: {} | Status: {} | Delay: {}ms", symbol, status, delay);
                                tokio::time::sleep(Duration::from_millis(*delay)).await;
                                continue;
                            }
                            return Err(format!("API_ERROR | Status: {}", status));
                        }
                        Err(e) => { // Errore di rete (es. DNS, connessione rifiutata)
                            println!("NET_ERROR | Symbol: {} | Error: {}", symbol, e);
                            if attempt == backoffs.len() - 1 { return Err(e.to_string()); }
                            tokio::time::sleep(Duration::from_millis(*delay)).await;
                        }
                    }
                }
                // 2. Il timeout È scattato (Elapsed)
                Err(_) => {
                    println!("TIMEOUT | Symbol: {} | No response after {:?}ms", symbol, request_timeout);
                    if attempt == backoffs.len() - 1 { return Err("TIMEOUT_FINAL_FAIL".into()); }
                    tokio::time::sleep(Duration::from_millis(*delay)).await;
                }
            }
        }

        Err("MAX_RETRIES_EXCEEDED".into())
    }
}