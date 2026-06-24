use serde_json::Value;

#[derive(Debug, Clone)]
pub struct CleanedMetrics {
    pub avg_up: f64,
    pub avg_down: f64,
}

pub struct DeterminationEngine;

impl DeterminationEngine {
    fn calculate_cleaned_metrics(matches: &Value) -> CleanedMetrics {
        let mut up_values = Vec::new();
        let mut down_values = Vec::new();

        if let Some(arr) = matches.as_array() {
            for m in arr {
                // Nel tuo JSON le chiavi storiche sono minuscole: "up" e "down"
                if let Some(up) = m["up"].as_f64() { up_values.push(up); }
                if let Some(down) = m["down"].as_f64() { down_values.push(down); }
            }
        }

        let clean_up = Self::filter_outliers(up_values);
        let clean_down = Self::filter_outliers(down_values);

        CleanedMetrics {
            avg_up: Self::mean(&clean_up),
            avg_down: Self::mean(&clean_down).abs(),
        }
    }

    fn filter_outliers(values: Vec<f64>) -> Vec<f64> {
        if values.len() < 3 { return values; }
        let mean = Self::mean(&values);
        
        let variance = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / values.len() as f64;
        let std_dev = variance.sqrt();

        if std_dev == 0.0 { return values; }

        values.into_iter()
            .filter(|&x| (x - mean).abs() <= 1.5 * std_dev)
            .collect()
    }

    fn mean(values: &[f64]) -> f64 {
        if values.is_empty() { return 0.0; }
        values.iter().sum::<f64>() / values.len() as f64
    }

    pub fn evaluate(session_data: &Value) -> (String, String, String, String) {
        let mut candle_edge = 0.0;
        let mut current_move = 0.0;
        let mut session_open = 0.0;
        let mut current_price_action = 0.0;
        let mut candle_matches = &Value::Null;

        if let Some(evolutions) = session_data["evolution_history"].as_array() {
            if let Some(latest) = evolutions.last() {
                let edge_raw = latest["edge_ratio"].as_f64().unwrap_or(0.0);
                let avg_up = latest["avg_up"].as_f64().unwrap_or(0.0);
                let avg_down = latest["avg_down"].as_f64().unwrap_or(0.0).abs();
                
                candle_edge = if avg_up >= avg_down { edge_raw.abs() } else { -edge_raw.abs() };
                
                current_price_action = session_data["current_price_action"].as_f64().unwrap_or(0.0);
                session_open = session_data["session_open"].as_f64().unwrap_or(0.0);
                current_move = current_price_action - session_open;
                
                candle_matches = &latest["matches"];
            }
        }

        let context = &session_data["context"];
        let c_edge = context["edge_ratio"].as_f64().unwrap_or(0.0);
        let c_up = context["avg_up"].as_f64().unwrap_or(0.0);
        let c_down = context["avg_down"].as_f64().unwrap_or(0.0).abs();
        let context_edge = if c_up >= c_down { c_edge.abs() } else { -c_edge.abs() };
        
        let context_matches = &context["matches"];

        // Consegna il conteggio totale degli outlier rilevati in entrambi i dataset
        let mut total_outliers = 0;

        // Pulizia CANDLE
        let mut candle_raw_down = Vec::new();
        if let Some(arr) = candle_matches.as_array() {
            for m in arr { if let Some(d) = m["down"].as_f64() { candle_raw_down.push(d); } }
        }
        let candle_metrics = Self::calculate_cleaned_metrics(candle_matches);
        let candle_clean_count = Self::filter_outliers(candle_raw_down.clone()).len();
        if candle_clean_count < candle_raw_down.len() {
            total_outliers += candle_raw_down.len() - candle_clean_count;
        }

        // Pulizia CONTEXT
        let mut context_raw_down = Vec::new();
        if let Some(arr) = context_matches.as_array() {
            for m in arr { if let Some(d) = m["down"].as_f64() { context_raw_down.push(d); } }
        }
        let context_metrics = Self::calculate_cleaned_metrics(context_matches);
        let context_clean_count = Self::filter_outliers(context_raw_down.clone()).len();
        if context_clean_count < context_raw_down.len() {
            total_outliers += context_raw_down.len() - context_clean_count;
        }

        let outlier_prefix = if total_outliers > 0 {
            format!("{} outlier(s) across datasets. ", total_outliers)
        } else {
            "".to_string()
        };

        // 1. Sign Alignment Check
        if candle_edge.signum() != context_edge.signum() {
            return (
                "STAY ASIDE".to_string(),
                "NEUTRAL".to_string(),
                "Conflict".to_string(),
                format!("{}Directional divergence between model contexts.", outlier_prefix)
            );
        }

        let bias = if candle_edge > 0.0 { "LONG" } else if candle_edge < 0.0 { "SHORT" } else { "NEUTRAL" }.to_string();

        // 2. Threshold Filters (Eseguito solo se sono concordi)
        if candle_edge.abs() < 2.0 || context_edge.abs() < 2.0 || (candle_edge.abs() + context_edge.abs()) < 4.0 {
            return (
                if candle_edge.abs() + context_edge.abs() < 3.5 { "STAY ASIDE".to_string() } else { "MONITOR".to_string() },
                bias,
                "Aligned / Low Edge".to_string(), 
                format!("{}Models are aligned but operational edge thresholds are not met.", outlier_prefix)
            );
        }

        // 3. Compression / Symmetrical Noise Check (Evaluated on both cleaned datasets)
        if candle_metrics.avg_down != 0.0 {
            let candle_ratio = candle_metrics.avg_up / candle_metrics.avg_down.abs();
            if candle_ratio >= 0.7 && candle_ratio <= 1.3 {
                return (
                    "STAY ASIDE".to_string(),
                    bias,
                    "Conflict".to_string(),
                    format!("{}Tight candle market compression lacking statistical asymmetry.", outlier_prefix)
                );
            }
        }

        if context_metrics.avg_down != 0.0 {
            let context_ratio = context_metrics.avg_up / context_metrics.avg_down.abs();
            if context_ratio >= 0.7 && context_ratio <= 1.3 {
                return (
                    "STAY ASIDE".to_string(),
                    bias,
                    "Conflict".to_string(),
                    format!("{}Tight context market compression lacking statistical asymmetry.", outlier_prefix)
                );
            }
        }

        // 4. Exhaustion Check against Cleaned Averages
        if bias == "LONG" {
            //println!("Session Open: {}, Current Price Action: {}", session_open, current_price_action);
            let target = session_open + candle_metrics.avg_up;
            let stop_loss = session_open - candle_metrics.avg_down;
            // println!("Target: {}, Stop Loss: {}", target, stop_loss);
            // Calcola il rischio e il rendimento potenziale dal prezzo corrente
            let potential_reward = target - current_price_action;
            let potential_risk = current_price_action - stop_loss;

            // Verifica se siamo sotto 1:1 RR o se il target è già stato raggiunto
            if current_move >= candle_metrics.avg_up {
                return (
                    "MONITOR".to_string(),
                    bias,
                    "Exhaustion Risk".to_string(),
                    format!("{}Upside distribution target achieved", outlier_prefix)
                );
            }
            println!("Potential Reward: {}, Potential Risk: {}", potential_reward, potential_risk);
            if potential_reward < potential_risk {
                return (
                    "MONITOR".to_string(),
                    bias,
                    "Exhaustion Risk".to_string(),
                    format!("{}Risk Reward ratio unfavorable.", outlier_prefix)
                );
            }
        } else if bias == "SHORT" {
            let target = session_open - candle_metrics.avg_down;
            let stop_loss = session_open + candle_metrics.avg_up;

            // Calcola il rischio e il rendimento potenziale dal prezzo corrente
            let potential_reward = current_price_action - target;
            let potential_risk = stop_loss - current_price_action;

            // Verifica se siamo sotto 1:1 RR o se il target è già stato raggiunto
            if current_move <= candle_metrics.avg_down {
                return (
                    "MONITOR".to_string(),
                    bias,
                    "Exhaustion Risk".to_string(),
                    format!("{}Downside distribution target achieved", outlier_prefix)
                );
            }

            if potential_reward < potential_risk {
                return (
                    "MONITOR".to_string(),
                    bias,
                    "Exhaustion Risk".to_string(),
                    format!("{}Risk Reward ratio unfavorable.", outlier_prefix)
                );
            }
        }

        (
            "TRADE".to_string(),
            bias,
            "Opportunity".to_string(),
            format!("{}Statistical alignment confirmed with favorable risk-reward.", outlier_prefix)
        )
    }
}