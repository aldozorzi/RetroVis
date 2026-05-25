use async_trait::async_trait;
use serde_json::{json, Value};
use reqwest::Client;
use crate::ai_provider::AiProvider;
use crate::determination_engine::DeterminationEngine;

use std::time::Duration;
// use tokio::time::timeout;

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

    
}

#[async_trait]
impl AiProvider for GeminiBroker {
    async fn request_analysis(&self, data: Value) -> Result<String, String> {
        // let model = "gemini-3.5-flash";
        let model = "gemini-3.1-flash-lite";
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model,
            self.api_key
        );

        let symbol = data["symbol"].as_str().unwrap_or("N/A");
        let (verdict, bias, market_state, observation) = DeterminationEngine::evaluate(&data);
        let report = self.format_trading_report(&data);

        let payload = json!({
            "systemInstruction": { "parts": { "text": self.system_prompt(&verdict, &bias, &market_state, &observation) } },
            "contents": { "parts": { "text": report } },
            "generationConfig": { 
                "temperature": 0.0,
                "topP": 0.1
            }
        });
        // println!("Report {:#?}", report);
        let backoffs = [500, 1000, 0];
        
        for (attempt, delay) in backoffs.iter().enumerate() {
            println!("REQ | Symbol: {} | Attempt: {}", symbol, attempt + 1);

            let resp = self.http_client
            .post(&url)
            .body(serde_json::to_string(&payload).map_err(|e| e.to_string())?)
            .timeout(Duration::from_secs(15))
            .send()
            .await;

            match resp {
                Ok(r) => { 
                    let status = r.status();
                    if status.is_success() {
                        let text = r.text().await.map_err(|e| format!("HTTP_TEXT_ERR: {}", e))?;
                        let json: Value = serde_json::from_str(&text).map_err(|e| format!("JSON_PARSE_ERR: {}", e))?;
                        
                        let full_text = json["candidates"][0]["content"]["parts"][0]["text"]
                            .as_str()
                            .ok_or_else(|| format!("MALFORMED_RESPONSE: {}", json))?;

                        // Taglia tutto ciò che si trova prima di </internal_analysis> (incluso il tag stesso)
                        let clean_text = if let Some(idx) = full_text.find("</internal_analysis>") {
                            &full_text[idx + "</internal_analysis>".len()..]
                        } else {
                            full_text
                        };
                        return Ok(clean_text.trim().to_string());
                    }

                    if status.as_u16() == 503 || status.as_u16() == 429 {
                        if attempt < backoffs.len() - 1 {
                            println!("RETRY Claude | Symbol: {} | Status: {} | Delay: {}ms", symbol, status, delay);
                            tokio::time::sleep(Duration::from_millis(*delay)).await;
                        }
                        continue;
                    }
                    return Err(format!("API_ERROR | Status: {}", status));
                }
                Err(e) => {
                    // reqwest restituisce un errore anche in caso di timeout nativo
                    if e.is_timeout() {
                        println!("TIMEOUT | Symbol: {} | No response after 15s", symbol);
                        if attempt == backoffs.len() - 1 { return Err("TIMEOUT_FINAL_FAIL".into()); }
                    } else {
                        println!("NET_ERROR | Symbol: {} | Error: {}", symbol, e);
                        if attempt == backoffs.len() - 1 { return Err(e.to_string()); }
                    }
                    tokio::time::sleep(Duration::from_millis(*delay)).await;
                }
            }
        }

        Err("MAX_RETRIES_EXCEEDED".into())
    }
}