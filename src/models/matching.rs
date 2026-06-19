use serde::{Deserialize, Serialize, Serializer};
use std::sync::Arc;
use crate::stats::{median, edge_ratio, bias_pct};   // usati dentro from_aggregates

pub struct MatchOutcome {
    pub timestamp: u32,
    pub distance: f32,
    pub eod_ret_pct: f32,
    pub mfe_pct: f32,
    pub mae_pct: f32,
    pub eoh_pct: f32,
    pub next_candle_up_pct: Option<f32>,
    pub next_candle_down_pct: Option<f32>,
}

#[derive(Debug, serde::Serialize)]
pub struct MatchesResponse {
    pub summary: AggregateData,
    pub matches: Vec<MatchResponse>,
    pub next_candle: NextCandleData,
}
impl From<CachedResult> for MatchesResponse {
    fn from(cached: CachedResult) -> Self {
        Self {
            summary: cached.summary,
            matches: (*cached.matches).clone(),
            next_candle: cached.next_candle,
        }
    }
}

// API-facing match result: same data as MatchOutcome, converted from
// percentages to price points using the current session's price level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MatchResponse {
    pub timestamp: u32,
    pub similarity: f32, // Similarity rescaled to 0-100 (higher = more similar).
    pub eos_pts: f32,
    pub up_pts: f32,
    pub down_pts: f32,
    pub eoh_pts: f32,
    pub next_candle_up: f32,  
    pub next_candle_down: f32,
}

// Aggregated statistics across all historical matches for a session.
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct AggregateData {
    pub avg_up: f32,
    pub avg_down: f32,
    pub avg_eos: f32,
    pub edge_ratio: f32,
    pub avg_eoh: f32,
    #[serde(serialize_with = "serialize_f32_as_int")]
    pub bias_pct: f32,
}

// Same statistics as AggregateData, but for the single candle immediately
// following the matched session open — a short-term directional signal.
#[derive(Debug, serde::Serialize, Clone, Copy, serde::Deserialize)]
pub struct NextCandleData {
    pub avg_up: f32,
    pub avg_down: f32,
    pub edge_ratio: f32,
    #[serde(serialize_with = "serialize_f32_as_int")]
    pub bias_pct: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedResult {
    pub summary: AggregateData,
    pub matches: Arc<Vec<MatchResponse>>,
    pub ai_analysis: Option<Arc<String>>,
    pub next_candle: NextCandleData,
}

impl CachedResult {
    pub fn from_aggregates(summary: AggregateData, matches: Vec<MatchResponse>) -> Self {
        let mut ups: Vec<f32>   = matches.iter().map(|m| m.next_candle_up).collect();
        let mut downs: Vec<f32> = matches.iter().map(|m| m.next_candle_down).collect();

        let avg_next_up   = median(&mut ups);
        let avg_next_down = median(&mut downs);
        let next_edge_ratio = edge_ratio(avg_next_up, avg_next_down);
        let next_bias_pct = bias_pct(matches.iter().map(|m| (m.next_candle_up, m.next_candle_down)));

        Self {
            summary,
            matches: Arc::new(matches),
            ai_analysis: None,
            next_candle: NextCandleData {
                avg_up: avg_next_up,
                avg_down: avg_next_down,
                edge_ratio: next_edge_ratio,
                bias_pct: next_bias_pct,
            },
        }
    }
}

fn serialize_f32_as_int<S>(value: &f32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    // Usa .round() se vuoi arrotondare, oppure rimuovilo se vuoi solo troncare
    serializer.serialize_i32(value.round() as i32)
}