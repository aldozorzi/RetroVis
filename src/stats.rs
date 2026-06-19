pub fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() { return 0.0; }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

pub fn edge_ratio(up: f32, down: f32) -> f32 {
    let long_edge  = up   / (if down == 0.0 { 1.0 } else { down });
    let short_edge = down / (if up   == 0.0 { 1.0 } else { up   });
    long_edge.max(short_edge)
}

pub fn bias_pct<I: IntoIterator<Item = (f32, f32)>>(pairs: I) -> f32 {
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

pub fn weighted_median(values: &[f32], weights: &[f32]) -> f32 {
    if values.is_empty() { return 0.0; }

    let mut indexed: Vec<(f32, f32)> = values.iter().zip(weights.iter()).map(|(&v, &w)| (v, w)).collect();
    indexed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

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

pub fn weighted_mad(values: &[f32], weights: &[f32], center: f32) -> f32 {
    if values.is_empty() { return 0.0; }
    let abs_devs: Vec<f32> = values.iter().map(|&v| (v - center).abs()).collect();
    weighted_median(&abs_devs, weights)
}