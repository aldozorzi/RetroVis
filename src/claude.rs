use async_trait::async_trait;
use serde_json::{json, Value};
use reqwest::Client;
use crate::ai_provider::AiProvider;

use std::time::Duration;

pub struct ClaudeBroker {
    api_key: String,
    http_client: Client,
}

impl ClaudeBroker {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http_client: Client::new(),
        }
    }
}

#[async_trait]
impl AiProvider for ClaudeBroker {
    async fn request_analysis(&self, data: Value) -> Result<String, String> {
        let model = "claude-haiku-4-5";
        let url = "https://api.anthropic.com/v1/messages";

        let symbol = data["symbol"].as_str().unwrap_or("N/A");
        let report = self.format_trading_report(&data);
        
        // Struttura payload ufficiale Anthropic
        let payload = json!({
            "model": model,
            "max_tokens": 1024,
            "system": self.system_prompt(),
            "messages": [
                {
                    "role": "user",
                    "content": report
                }
            ],
            "temperature": 0.0
        });

        let backoffs = [500, 1000, 0];
        
        for (attempt, delay) in backoffs.iter().enumerate() {
            println!("REQ Claude | Symbol: {} | Attempt: {}", symbol, attempt + 1);

            let resp = self.http_client
                .post(url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .body(serde_json::to_string(&payload)?)
                .timeout(Duration::from_secs(15))
                .send()
                .await;

            match resp {
                Ok(r) => { 
                    let status = r.status();
                    if status.is_success() {
                        let text = r.text().await.map_err(|e| format!("HTTP_TEXT_ERR: {}", e))?;
                        let json: Value = serde_json::from_str(&text).map_err(|e| format!("JSON_PARSE_ERR: {}", e))?;
                        
                        // Parsing del blocco di risposta di Anthropic: content[0].text
                        let full_text = json["content"][0]["text"]
                            .as_str()
                            .ok_or_else(|| format!("MALFORMED_RESPONSE: {}", json))?;

                        // Taglia l'analisi interna se presente
                        let clean_text = if let Some(idx) = full_text.find("</internal_analysis>") {
                            &full_text[idx + "</internal_analysis>".len()..]
                        } else {
                            full_text
                        };
                        return Ok(clean_text.trim().to_string());
                    }

                    // 429 è Rate Limit, 529 è il codice specifico di Anthropic per Overloaded Server
                    if status.as_u16() == 529 || status.as_u16() == 429 || status.is_server_error() {
                        if attempt < backoffs.len() - 1 {
                            println!("RETRY Claude | Symbol: {} | Status: {} | Delay: {}ms", symbol, status, delay);
                            tokio::time::sleep(Duration::from_millis(*delay)).await;
                        }
                        continue;
                    }
                    return Err(format!("API_ERROR | Status: {}", status));
                }
                Err(e) => {
                    if e.is_timeout() {
                        println!("TIMEOUT Claude | Symbol: {} | No response after 15s", symbol);
                        if attempt == backoffs.len() - 1 { return Err("TIMEOUT_FINAL_FAIL".into()); }
                    } else {
                        println!("NET_ERROR Claude | Symbol: {} | Error: {}", symbol, e);
                        if attempt == backoffs.len() - 1 { return Err(e.to_string()); }
                    }
                    tokio::time::sleep(Duration::from_millis(*delay)).await;
                }
            }
        }

        Err("MAX_RETRIES_EXCEEDED".into())
    }
}