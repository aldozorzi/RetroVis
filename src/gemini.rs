use async_trait::async_trait;
use serde_json::{json, Value};
use reqwest::Client;
use crate::ai_provider::AiProvider;

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

    
}

#[async_trait]
impl AiProvider for GeminiBroker {
    async fn request_analysis(&self, data: Value) -> Result<String, String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-lite-preview:generateContent?key={}",
            self.api_key
        );

        let symbol = data["symbol"].as_str().unwrap_or("N/A");
        let report = self.format_trading_report(&data);
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