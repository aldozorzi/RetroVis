use async_trait::async_trait;
use serde_json::Value;
use crate::models::AiProvider;


pub struct OllamaBroker {
    model: String,
    endpoint: String,
}

impl OllamaBroker {
    pub fn new() -> Self {
        let model = "qwen2.5:1.5b".to_string();
        Self {
            model: model.to_string(),
            endpoint: "http://localhost:11434/v1/chat/completions".to_string(),
        }
    }
}

#[async_trait]
impl AiProvider for OllamaBroker {
    async fn request_analysis(&self, data: Value) -> Result<String, String> {
        let client = reqwest::Client::new();
        /*
        println!("Sending request to Ollama for symbol: {:?}", data["symbol"].as_str().unwrap_or("N/A"));
        println!("Payload: {}", data);
        println!("System Prompt: {}", self.system_prompt());
        */
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": self.system_prompt()},
                {"role": "user", "content": data.to_string()}
            ],
            "temperature": 0.1
        });

        let response = client.post(&self.endpoint)
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let text = response.text().await.map_err(|e| e.to_string())?;
        let res_json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        
        res_json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Errore nel parsing della risposta locale".to_string())
    }
}