pub fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() { return 0.0; }
    values.sort_by(|a, b| a.total_cmp(b));

    values[values.len() / 2]
}

fn edge_ratio(up: f32, down: f32) -> f32 {
    let long_edge  = up   / (if down == 0.0 { 1.0 } else { down });
    let short_edge = down / (if up   == 0.0 { 1.0 } else { up   });
    long_edge.max(short_edge)
}

fn bias_pct<I: IntoIterator<Item = (f32, f32)>>(pairs: I) -> f32 {
    let mut bullish = 0usize;
    let mut bearish = 0usize;
    for (up, down) in pairs {
        if up > down { bullish += 1; }
        else if down > up { bearish += 1; }
    }
    let total = (bullish + bearish) as f32;
    if total == 0.0 { return 0.0; }
    if bullish >= bearish {
        bullish as f32 / total * 100.0
    } else {
        -(bearish as f32 / total * 100.0)
    }
}

fn weighted_median(values: &[f32], weights: &[f32]) -> f32 {
    if values.is_empty() { return 0.0; }

    let mut indexed: Vec<(f32, f32)> = values.iter().zip(weights.iter()).map(|(&v, &w)| (v, w)).collect();
    indexed.sort_by(|a, b| a.0.total_cmp(&b.0));

    let total_weight: f32 = indexed.iter().map(|(_, w)| w).sum();
    let half_weight = total_weight / 2.0;

    let mut cumulative = 0.0;
    for (val, weight) in indexed.iter() {
        cumulative += weight;
        if cumulative >= half_weight {
            return *val;
        }
    }

    indexed.last().unwrap().0
}

fn weighted_mad(values: &[f32], weights: &[f32], center: f32) -> f32 {
    if values.is_empty() { return 0.0; }
    let abs_devs: Vec<f32> = values.iter().map(|&v| (v - center).abs()).collect();
    weighted_median(&abs_devs, weights)
}

/// Median-based projection for a pair of opposing magnitudes (e.g. favorable vs
/// adverse excursion, or next-candle upside vs downside wick), plus how reliable
/// the dominant side's estimate is. Same shape of problem for session MFE/MAE
/// and for next-candle up/down — only the input data differs.
pub struct Projection {
    pub avg_up: f32,
    pub avg_down: f32,
    pub edge_ratio: f32,
    pub bias_pct: f32,
    /// 0.0 to 1.0 — combina quanti match concordano sulla direzione (bias_pct)
    /// con quanto è precisa la stima del lato dominante (MAD pesata). Scende se
    /// i match disaccordano O se il lato dominante è sparso — basta una delle due.
    pub confidence: f32,
}

pub fn weighted_projection(ups: &[f32], downs: &[f32], weights: &[f32]) -> Projection {
    let avg_up = weighted_median(ups, weights);
    let avg_down = weighted_median(downs, weights);
    let ratio = edge_ratio(avg_up, avg_down);

    let raw_bias_pct = bias_pct(ups.iter().zip(downs.iter()).map(|(&u, &d)| (u, d)));

    let (dominant_values, dominant_median) = if avg_up >= avg_down {
        (ups, avg_up)
    } else {
        (downs, avg_down)
    };
    let dominant_mad = weighted_mad(dominant_values, weights, dominant_median);
    let n = ups.len() as f32;

    let magnitude_confidence = if n < 3.0 || dominant_median == 0.0 {
        0.0
    } else {
        dominant_median.abs() / (dominant_median.abs() + dominant_mad / n.sqrt())
    };

    let confidence = (raw_bias_pct.abs() / 100.0) * magnitude_confidence;

    Projection { avg_up, avg_down, edge_ratio: ratio, bias_pct: raw_bias_pct, confidence }
}