use serde_json::Value;
use std::time::Duration;
use crate::models::AiProvider;
use crate::http_client::HttpClient;
// RIMOSSO: use async_trait::async_trait;

pub struct OllamaBroker {
    model: String,
    endpoint: String,
    http: HttpClient,           // era creato dentro request_analysis ad ogni chiamata
}

impl OllamaBroker {
    pub fn new() -> Self {
        let model = "qwen2.5:1.5b".to_string();
        Self {
            model,
            endpoint: "http://localhost:11434/v1/chat/completions".to_string(),
            http: HttpClient::new(Duration::from_secs(30)), // locale, timeout generoso
        }
    }
}

impl AiProvider for OllamaBroker {
    async fn request_analysis(&self, data: Value) -> Result<String, String> {
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system",  "content": self.system_prompt() },
                { "role": "user",    "content": data.to_string() }
            ],
            "temperature": 0.1
        });

        let (_, text) = self.http
            .post_raw(
                &self.endpoint,
                &[("content-type", "application/json")],
                payload.to_string(),
                Duration::from_secs(30),
            )
            .await
            .map_err(|e| e.to_string())?;

        let res_json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;

        res_json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Errore nel parsing della risposta locale".to_string())
    }
}