// src/engine.rs
use crate::models::{HistoricalPattern, NormalizedCandle, MatchOutcome};

pub struct SearchEngine<T> {
    pub dataset: Vec<HistoricalPattern<T>>,
}

impl SearchEngine<f32> {
    pub fn new(dataset: Vec<HistoricalPattern<f32>>) -> Self {
        Self { dataset }
    }

    // Calculate distance beetween two candles
    fn calculate_distance(current: &[NormalizedCandle<f32>], historical: &[NormalizedCandle<f32>]) -> f32 {
        let mut distance = 0.0;
        
        // compare candles one by one
        for (c, h) in current.iter().zip(historical.iter()) {
            // Sommiamo i quadrati delle differenze di ogni componente
            distance += (c.body_pct - h.body_pct).powi(2);
            distance += (c.wick_up_pct - h.wick_up_pct).powi(2);
            distance += (c.wick_down_pct - h.wick_down_pct).powi(2);
            distance += (c.range_pct - h.range_pct).powi(2);
        }
        
        distance.sqrt() // Squared root to get euclidean distance
    }

    // Find best top-k matches in the past
    pub fn find_matches<'a>(
        &'a self, 
        current_pattern: &[NormalizedCandle<f32>], 
        top_k: usize
    ) -> Vec<MatchOutcome<'a>> {
        let len = current_pattern.len();
        let mut outcomes = Vec::new();

        for entry in &self.dataset {
            // Compare only if historical day has candles enough
            if entry.candles.len() >= len {
                let dist = Self::calculate_distance(current_pattern, &entry.candles[..len]);
                outcomes.push(MatchOutcome {
                    pattern: entry,
                    distance: dist,
                });
            }
        }

        // Sort by distance
        outcomes.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());

        // Take first K
        outcomes.into_iter().take(top_k).collect()
    }
}