use async_trait::async_trait;
use serde_json::{json, Value};
use crate::http_client::HttpClient;
use crate::ai_provider::AiProvider;
use crate::determination_engine::DeterminationEngine;

use std::time::Duration;
// use tokio::time::timeout;

pub struct GeminiBroker {
    api_key: String,
    http_client: HttpClient,
}

impl GeminiBroker {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http_client: HttpClient::new(Duration::from_secs(20)),
        }
    }

    
}

#[async_trait]
impl AiProvider for GeminiBroker {
    async fn request_analysis(&self, data: Value) -> Result<String, String> {
        let model = "gemini-3.1-flash-lite";
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, self.api_key
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

        let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        let backoffs = [500u64, 1000, 0];

        for (attempt, delay) in backoffs.iter().enumerate() {
            println!("REQ | Symbol: {} | Attempt: {}", symbol, attempt + 1);

            let resp = self.http_client
                .post_raw(&url, &[], body.clone(), Duration::from_secs(15))
                .await;

            match resp {
                Ok((status, text)) if (200..300).contains(&status) => {
                    let json: Value = serde_json::from_str(&text)
                        .map_err(|e| format!("JSON_PARSE_ERR: {}", e))?;

                    let full_text = json["candidates"][0]["content"]["parts"][0]["text"]
                        .as_str()
                        .ok_or_else(|| format!("MALFORMED_RESPONSE: {}", json))?;

                    let clean_text = if let Some(idx) = full_text.find("</internal_analysis>") {
                        &full_text[idx + "</internal_analysis>".len()..]
                    } else {
                        full_text
                    };
                    return Ok(clean_text.trim().to_string());
                }

                Ok((status @ 503, _)) | Ok((status @ 429, _)) => {
                    if attempt < backoffs.len() - 1 {
                        println!("RETRY Gemini | Symbol: {} | Status: {} | Delay: {}ms", symbol, status, delay);
                        tokio::time::sleep(Duration::from_millis(*delay)).await;
                    }
                    continue;
                }

                Ok((status, _)) => {
                    return Err(format!("API_ERROR | Status: {}", status));
                }

                // DIFFERENZA: e.is_timeout() funziona identicamente tramite HttpError
                Err(e) if e.is_timeout() => {
                    println!("TIMEOUT | Symbol: {} | No response after 15s", symbol);
                    if attempt == backoffs.len() - 1 {
                        return Err("TIMEOUT_FINAL_FAIL".into());
                    }
                    tokio::time::sleep(Duration::from_millis(*delay)).await;
                }

                Err(e) => {
                    println!("NET_ERROR | Symbol: {} | Error: {}", symbol, e);
                    if attempt == backoffs.len() - 1 {
                        return Err(e.to_string());
                    }
                    tokio::time::sleep(Duration::from_millis(*delay)).await;
                }
            }
        }

        Err("MAX_RETRIES_EXCEEDED".into())
    }
}