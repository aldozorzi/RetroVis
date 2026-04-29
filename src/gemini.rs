use async_trait::async_trait;
use serde_json::{json, Value};
use reqwest::Client;
use crate::models::AiProvider;

const SYSTEM_PROMPT: &str = r#"
CORE ANALYSIS LOGIC:

1. STRUCTURAL CONFLUENCE (MANDATORY):
   - Compare IDENTIFIED_CURRENT_EDGE with CONTEXT_EDGE. 
   - If signs are the same (e.g., both Negative): ALIGNED STRUCTURE. This is your foundation.
   - If signs differ: STRUCTURAL CONFLICT. (Verdict: STAY ASIDE).

2. PRICE-EDGE RELATIONSHIP (THE OPPORTUNITY):
   - Compare the Statistical Bias with IDENTIFIED_CURRENT_NOW.
   - SCENARIO A (SQUEEZE/MEAN REVERSION): If Stats are SHORT but Price is POSITIVE (or vice versa), this is a "PREMIUM ENTRY ZONE". The price is overextended against the statistical expectation.
   - SCENARIO B (CONFIRMATION): If Stats and Price move together, it's a standard trend follow.

3. CONVICTION SCORING:
   - HIGH CONVICTION: Level 1 is ALIGNED + Level 2 is SCENARIO A (Price is fighting the stats, creating a high-value mean reversion opportunity) and both IDENTIFIED_CURRENT_EDGE and CONTEXT_EDGE are above 2.
   - MEDIUM CONVICTION: Level 1 is ALIGNED + Level 2 is SCENARIO B (Standard alignment).

OUTPUT FORMAT:
### STRATEGIC VALIDATION
- BIAS: [SHORT / LONG / NEUTRAL]
- CONVICTION: [High / Medium / Low]
- MARKET STATE: [Mean Reversion Opportunity / Trend Following / Conflict]
- CRITICAL OBSERVATION: [1-2 sentences. Explicitly state if the price (NOW) is offering a "Premium Entry" by moving against the aligned statistical bias.]
- FINAL VERDICT: [TRADE / MONITOR / STAY ASIDE]

STRICT: "TRADE" is encouraged when Edge and Context align and both IDENTIFIED_CURRENT_EDGE and CONTEXT_EDGE are above 2, ESPECIALLY if the current price (NOW) is moving in the opposite direction (Squeeze).
"#;

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
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-flash-preview:generateContent?key={}",
            self.api_key
        );

        let prompt_text = Self::format_trading_report(&data);

        let payload = json!({
            "systemInstruction": {
                "parts": { "text": SYSTEM_PROMPT }
            },
            "contents": {
                "parts": { "text": prompt_text }
            },
            "generationConfig": {
                "temperature": 0.5,
            }
        });
        println!("Sending request to Gemini for symbol: {:?}", data["symbol"].as_str().unwrap_or("N/A"));
        let resp = self.http_client.post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Errore network: {}", e))?
            .json::<Value>()
            .await
            .map_err(|e| format!("Errore parsing JSON: {}", e))?;

        // Estrazione sicura del testo
        resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("Risposta Gemini non valida: {}", resp))
    }
}