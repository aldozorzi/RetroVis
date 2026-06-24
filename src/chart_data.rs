use crate::models::RawCandle;
use crate::engine::SearchEngine;

impl SearchEngine {
    pub fn get_day_candles(&self, date: u32, timeframe_min: u32, include_prev: bool) -> Vec<RawCandle<f32>> {
        let pos = match self.dataset.find_by_date(date) {
            Some(p) => p,
            None => return Vec::new()
        };

        let raw_prev: &[RawCandle<f32>] = if include_prev && pos > 0 {
            self.dataset.get(pos - 1).map(|(_, c)| c).unwrap_or(&[])
        } else {
            &[]
        };
        let (_, raw_curr) = match self.dataset.get(pos) {
            Some(d) => d,
            None => return Vec::new()
        };

        let prev = SearchEngine::maybe_resample(raw_prev, timeframe_min);
        let curr = SearchEngine::maybe_resample(raw_curr, timeframe_min);

        prev.iter().chain(curr.iter()).cloned().collect()
    }
}
