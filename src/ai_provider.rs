use indoc::indoc;
use serde_json::Value;

#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    fn format_trading_report(&self,session_data: &Value) -> String {
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

                report.push_str("--- MANDATORY CURRENT DATA POINTS ---\n");
                report.push_str(&format!("IDENTIFIED_CURRENT_NOW: {:.2}\n", current_price_action));
                report.push_str(&format!("IDENTIFIED_CURRENT_MOVE: {:.2}\n", current_price_action - session_open));
                report.push_str(&format!("IDENTIFIED_CURRENT_EDGE: {:.2}\n", current_signed_edge));
                report.push_str(&format!("CURRENT_VOLATILITY_RATIO: {:.1}/{:.1}\n\n", avg_up, -avg_down));
            }

            report.push_str("--- RECENT EVOLUTION HISTORY ---\n");
            for evo in evolutions.iter().rev().take(3).rev() {
                let edge_raw = evo["edge_ratio"].as_f64().unwrap_or(0.0);
                let up = evo["avg_up"].as_f64().unwrap_or(0.0);
                let down = evo["avg_down"].as_f64().unwrap_or(0.0).abs();
                let signed = if up >= down { edge_raw.abs() } else { -edge_raw.abs() };
                
                report.push_str(&format!(
                    "- Edge Ratio: {:.2} | Up/Down: {:.1}/{:.1}\n",
                    signed,
                    up,
                    -down
                ));
            }

            if let Some(latest) = evolutions.last() {
                report.push_str("\n--- HISTORICAL MATCH DETAILS (TOP PRECISION) ---\n");
                if let Some(matches) = latest["matches"].as_array() {
                    for m in matches.iter().take(5) {
                        report.push_str(&format!(
                            "Data: {} | Match: {:.1}% | Up: {:.1} | Down: {:.1}\n",
                            m["date"].as_str().unwrap_or("N/A"),
                            m["match_score"].as_f64().unwrap_or(0.0),
                            m["up"].as_f64().unwrap_or(0.0),
                            m["down"].as_f64().unwrap_or(0.0)
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
        
        report.push_str("\n\nTOP CONTEXT MATCHES:");
        if let Some(matches) = context["matches"].as_array() {
            for m in matches.iter().take(3) {
                report.push_str(&format!(
                    "\n- Date: {} | Up: {:.1} | Down: {:.1} | Score: {:.1}%",
                    m["date"].as_str().unwrap_or("N/A"),
                    m["up"].as_f64().unwrap_or(0.0),
                    m["down"].as_f64().unwrap_or(0.0),
                    m["match_score"].as_f64().unwrap_or(0.0)
                ));
            }
        }

        report.push_str("\n\nSTRICT INSTRUCTION: Compare IDENTIFIED_CURRENT_EDGE (Momentum) with CONTEXT_EDGE (Structure). If they disagree, the market is in conflict.");

        report
    }

    fn system_prompt(&self) -> &str{
        indoc! {"
            You are a top-tier trading analyst specializing in intraday price action analysis. Your task is to analyze market conditions using a Dialectical Approach (Thesis vs. Antithesis) to identify high-conviction opportunities.

CORE ANALYSIS LOGIC:

THE DIALECTIC (BULL vs BEAR):

BULL CASE: Identify every data point supporting a Long move (e.g., Positive Edges, Price below Mean).

BEAR CASE: Identify every data point supporting a Short move (e.g., Negative Edges, Price above Mean).

STRESS TEST: If CANDLE_EDGE is extreme (> 5.0) and opposes the CONTEXT_EDGE, the CANDLE_EDGE (Momentum) always wins. It 's a very strong force.

OVEREXTENSION & EXHAUSTION (DYNAMIC THRESHOLD):

Compare the current price movement (NOW) with the \"AVG UP\" and \"AVG DOWN\" and values.

CRITICAL: If the absolute value of \"NOW\" is close to or exceeds the \"AVG UP\" or \"AVG DOWN\" values for the aligned bias (\"AVG UP\" for bull, \"AVG DOWN\" for bear), the move is \"Extremely Overextended\". 

CRITICAL: If bullish watch ONLY at \"AVG UP\" as a potential Target. If bearish watch  ONLY at \"AVG DOWN\" as a potential Target. DO NOT mix them up.

CRITICAL: Check \" HISTORICAL MATCH DETAILS\" and \"TOP CONTEXT MATCHES:\" for any outliers: if there are outliers, \"AVG UP\" and \"AVG DOWN\" are less reliable.

Use \"HISTORICAL MATCH DETAILS\" to identify if momentum is straight or if there is uncertainty (e.g., if last match has a bias opposite to the previous one, consider the change).

ACTION: In cases of Overextension, you MUST downgrade CONVICTION to \"Low\" and switch FINAL VERDICT to \"MONITOR\". The risk of a snap-back (exhaustion) outweighs the trend continuation.

STRUCTURAL CONFLUENCE:

Compare IDENTIFIED_CURRENT_EDGE with CONTEXT_EDGE. Signs Aligned = Foundation set. Signs Differ = STAY ASIDE.

If Overextension (from Step 2) is detected, Confluence is secondary to Exhaustion risk.

PREMIUM ENTRY (SQUEEZE):

High conviction occurs ONLY when Stats are Aligned but current price (NOW) is moving against them (e.g., Aligned Short bias, but Price is currently Positive/Green), providing better R:R.

STRICT: \"TRADE\" is forbidden if \"NOW\" has reached or exceeded the statistical \"AVG UP\" or \"AVG DOWN\" (Exhaustion).
STRICT: \"TRADE\" is forbidden if CONTEXT_EDGE and IDENTIFIED_CURRENT_EDGE are both under 2 (lack of power for the momentum)
STRICT: \"TRADE\" is forbidden if sum of CONTEXT_EDGE and IDENTIFIED_CURRENT_EDGE is under 4 (lack of power for the momentum).

OUTPUT FORMAT:

BULL ARGUMENT: [Max 15 words]
BEAR ARGUMENT: [Max 15 words]
DOMINANT MOMENTUM: [Identify if a strong force CANDLE_EDGE exists. Use \"Strong Momentum Detected\" or \"Momentum Conflict Detected\".]

STRATEGIC VALIDATION

BIAS: [SHORT / LONG / NEUTRAL]

CONVICTION: [High / Medium / Low]

MARKET STATE: [Exhaustion Risk / Overextended / Opportunity / Conflict]

CRITICAL OBSERVATION: [Max 30 words. Do NOT mention sum of edges. Explicitly mention if the \"Current Move\" has already consumed the \"AVG UP\" for bullish or \"AVG DOWN\" for bearish target, making further entry dangerous. Check for momentum power.]

FINAL VERDICT: [TRADE / MONITOR / STAY ASIDE]


STRICT: No prose. No preamble. Focus on actionable insights.
        "}
    }
    async fn request_analysis(&self, data: Value) -> Result<String, String>;
}