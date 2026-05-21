use indoc::indoc;
use serde_json::Value;

#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    fn format_trading_report(&self, session_data: &Value) -> String {
        let symbol = session_data["symbol"].as_str().unwrap_or("N/A");
        let session = session_data["session_id"].as_str().unwrap_or("N/A");
        let history = &session_data["evolution_history"];
        let context = &session_data["context"];
        let current_price_action = session_data["current_price_action"].as_f64().unwrap_or(0.0);
        let session_open = session_data["session_open"].as_f64().unwrap_or(0.0);

        let mut report = format!("### MARKET DATA - SYMBOL: {} | SESSION: {}\n\n", symbol, session);

        if let Some(evolutions) = history.as_array() {
            if let Some(latest) = evolutions.last() {
                let edge_raw = latest["edge_ratio"].as_f64().unwrap_or(0.0);
                let avg_up = latest["avg_up"].as_f64().unwrap_or(0.0);
                let avg_down = latest["avg_down"].as_f64().unwrap_or(0.0).abs();
                let current_signed_edge = if avg_up >= avg_down { edge_raw.abs() } else { -edge_raw.abs() };
                let current_move = current_price_action - session_open;

                report.push_str("--- CANDLE STATS ---\n");
                report.push_str(&format!("NOW_POSITION: {:.2}\n", current_price_action));
                report.push_str(&format!("CURRENT_MOVE: {:.2}\n", current_move));
                report.push_str(&format!("CANDLE_EDGE: {:.2}\n", current_signed_edge));
                report.push_str(&format!("CANDLE_AVG_UP: {:.1}\n", avg_up));
                report.push_str(&format!("CANDLE_AVG_DOWN: {:.1}\n\n", -avg_down));
            }

            if let Some(latest) = evolutions.last() {
                report.push_str("--- HISTORICAL CANDLE MATCHES ---\n");
                if let Some(matches) = latest["matches"].as_array() {
                    for m in matches.iter().take(5) {
                        report.push_str(&format!(
                            "Date: {} | Score: {:.1}% | Up: {:.1} | Down: {:.1}\n",
                            m["date"].as_str().unwrap_or("N/A"),
                            m["match_score"].as_f64().unwrap_or(0.0),
                            m["up"].as_f64().unwrap_or(0.0),
                            m["down"].as_f64().unwrap_or(0.0)
                        ));
                    }
                }
                report.push_str("\n");
            }
        }

        report.push_str("--- CONTEXT STATS ---\n");
        let c_edge = context["edge_ratio"].as_f64().unwrap_or(0.0);
        let c_up = context["avg_up"].as_f64().unwrap_or(0.0);
        let c_down = context["avg_down"].as_f64().unwrap_or(0.0).abs();
        let signed_c_edge = if c_up >= c_down { c_edge.abs() } else { -c_edge.abs() };

        report.push_str(&format!("CONTEXT_EDGE: {:.2}\n", signed_c_edge));
        report.push_str(&format!("CONTEXT_AVG_UP: {:.1}\n", c_up));
        report.push_str(&format!("CONTEXT_AVG_DOWN: {:.1}\n", -c_down));
        
        report.push_str("--- HISTORICAL CONTEXT MATCHES ---\n");
        if let Some(matches) = context["matches"].as_array() {
            for m in matches.iter().take(3) {
                report.push_str(&format!(
                    "Date: {} | Score: {:.1}% | Up: {:.1} | Down: {:.1}\n",
                    m["date"].as_str().unwrap_or("N/A"),
                    m["match_score"].as_f64().unwrap_or(0.0),
                    m["up"].as_f64().unwrap_or(0.0),
                    m["down"].as_f64().unwrap_or(0.0)
                ));
            }
        }

        report
    }

fn system_prompt(&self, verdict: &str, bias: &str, market_state: &str, observation: &str) -> String {
    format!(
        indoc! {"
            You are a technical financial writer. Your only task is to write a concise, professional market summary based on the DETERMINISTIC VALUES provided below. Do not alter the VERDICT, BIAS, MARKET STATE, or CRITICAL OBSERVATION. No prose outside the fields.

            DETERMINISTIC INPUTS:
            - FINAL VERDICT: {verdict}
            - BIAS: {bias}
            - MARKET STATE: {market_state}
            - CRITICAL OBSERVATION: {observation}

            OUTPUT STRUCTURE (STRICTLY FOLLOW THIS LAYOUT, NO EXTRA LINES):

            BULL ARGUMENT: [Write a 10-word technical summary of the upside based on the data]
            BEAR ARGUMENT: [Write a 10-word technical summary of the downside based on the data]
            DOMINANT MOMENTUM: [State if momentum is Aligned or Conflicted]

            BIAS: {bias}
            CONVICTION: [High/Medium/Low based on the verdict]
            MARKET STATE: {market_state}

            CRITICAL OBSERVATION: {observation}

            FINAL VERDICT: {verdict}
        "},
        verdict = verdict,
        bias = bias,
        market_state = market_state,
        observation = observation
    )
}
    async fn request_analysis(&self, data: Value) -> Result<String, String>;
}