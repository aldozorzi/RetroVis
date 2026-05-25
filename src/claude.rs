use serde_json::{json, Value};
use std::time::Duration;
use crate::ai_provider::AiProvider;
use crate::http_client::{HttpClient, HttpError};
// RIMOSSO: use async_trait::async_trait;
// RIMOSSO: use reqwest::Client;

pub struct ClaudeBroker {
    api_key: String,
    http: HttpClient,
}

impl ClaudeBroker {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: HttpClient::new(Duration::from_secs(20)),
        }
    }
}

impl AiProvider for ClaudeBroker {
    // RIMOSSO: #[async_trait] — se AiProvider usa async fn stabili (Rust ≥ 1.75)
    async fn request_analysis(&self, data: Value) -> Result<String, String> {
        let url = "https://api.anthropic.com/v1/messages";
        let symbol = data["symbol"].as_str().unwrap_or("N/A");
        let report = self.format_trading_report(&data);

        let payload = json!({
            "model": "claude-haiku-4-5",
            "max_tokens": 1024,
            "system": self.system_prompt(),
            "messages": [{ "role": "user", "content": report }],
            "temperature": 0.0
        });

        let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;

        let headers = [
            ("x-api-key",          self.api_key.as_str()),
            ("anthropic-version",  "2023-06-01"),
            ("content-type",       "application/json"),
        ];

        let backoffs = [500u64, 1000, 0];

        for (attempt, delay) in backoffs.iter().enumerate() {
            println!("REQ Claude | Symbol: {} | Attempt: {}", symbol, attempt + 1);

            match self.http.post_raw(url, &headers, body.clone(), Duration::from_secs(15)).await {
                Ok((status, text)) if (200..300).contains(&status) => {
                    let json: Value = serde_json::from_str(&text)
                        .map_err(|e| format!("JSON_PARSE_ERR: {}", e))?;

                    let full_text = json["content"][0]["text"]  // path Anthropic
                        .as_str()
                        .ok_or_else(|| format!("MALFORMED_RESPONSE: {}", json))?;

                    let clean_text = if let Some(idx) = full_text.find("</internal_analysis>") {
                        &full_text[idx + "</internal_analysis>".len()..]
                    } else {
                        full_text
                    };
                    return Ok(clean_text.trim().to_string());
                }

                // 429 rate limit, 529 Anthropic overload, tutti i 5xx
                Ok((status @ 429, _))
                | Ok((status @ 529, _))
                | Ok((status, _)) if (500..600).contains(&status) => {
                    if attempt < backoffs.len() - 1 {
                        println!("RETRY Claude | Symbol: {} | Status: {} | Delay: {}ms", symbol, status, delay);
                        tokio::time::sleep(Duration::from_millis(*delay)).await;
                    }
                    continue;
                }

                Ok((status, _)) => return Err(format!("API_ERROR | Status: {}", status)),

                Err(e) if e.is_timeout() => {
                    println!("TIMEOUT Claude | Symbol: {} | No response after 15s", symbol);
                    if attempt == backoffs.len() - 1 { return Err("TIMEOUT_FINAL_FAIL".into()); }
                    tokio::time::sleep(Duration::from_millis(*delay)).await;
                }

                Err(e) => {
                    println!("NET_ERROR Claude | Symbol: {} | Error: {}", symbol, e);
                    if attempt == backoffs.len() - 1 { return Err(e.to_string()); }
                    tokio::time::sleep(Duration::from_millis(*delay)).await;
                }
            }
        }

        Err("MAX_RETRIES_EXCEEDED".into())
    }
}