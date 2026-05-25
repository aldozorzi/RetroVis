use crate::models::{HistoricalDay, NormalizedCandle};
use chrono::Timelike;

pub struct SearchEngine<T> {
    pub dataset: Vec<HistoricalDay<T>>,
}

impl SearchEngine<f32> {
    pub fn new(dataset: Vec<HistoricalDay<f32>>) -> Self {
        Self { dataset }
    }

    // Restituisce i secondi dall'inizio della giornata (00:00:00) per la sessione, senza conversioni o stringhe
    fn get_ny_seconds_for_session(session: &crate::models::SessionConfig) -> i64 {
        let hour = session.start_time.hour() as i64;
        let minute = session.start_time.minute() as i64;
        let second = session.start_time.second() as i64;
        (hour * 3600) + (minute * 60) + second
    }

    fn calculate_distance(current: &[NormalizedCandle<f32>], historical: &[NormalizedCandle<f32>]) -> f32 {
        let mut distance = 0.0;
        for (c, h) in current.iter().zip(historical.iter()) {
            distance += (c.body_pct - h.body_pct).powi(2);
            distance += (c.wick_up_pct - h.wick_up_pct).powi(2);
            distance += (c.wick_down_pct - h.wick_down_pct).powi(2);
            distance += (c.range_pct - h.range_pct).powi(2);
        }
        distance.sqrt()
    }

    pub fn find_matches(
        &self, 
        current_pattern: &[crate::models::NormalizedCandle<f32>], 
        top_k: usize,
        session: &crate::models::SessionConfig
    ) -> Vec<crate::models::MatchOutcome> {
        let len = current_pattern.len();
        let mut outcomes = Vec::new();
        let session_offset_secs = Self::get_ny_seconds_for_session(session);

        for day in &self.dataset {
            if day.candles.is_empty() { continue; }
            
            // Troviamo la mezzanotte del giorno corrente usando il timestamp della prima candela
            let day_start_timestamp = (day.candles[0].timestamp / 86400) * 86400;
            let target_start_timestamp = day_start_timestamp + session_offset_secs;

            // Ricerca binaria sul timestamp u64 anziché sulla stringa dell'ora
            if let Ok(start_idx) = day.candles.binary_search_by_key(&target_start_timestamp, |c| c.timestamp) {
                if day.candles.len() >= start_idx + len {
                    let baseline = day.candles[start_idx].open;
                    
                    let historical_pattern: Vec<crate::models::NormalizedCandle<f32>> = day.candles[start_idx..start_idx + len]
                        .iter()
                        .map(|r| crate::models::NormalizedCandle::from_raw(r, baseline))
                        .collect();

                    let dist = Self::calculate_distance(current_pattern, &historical_pattern);
                    
                    // Fine della prima ora: +3600 secondi
                    let eoh_timestamp = target_start_timestamp + 3600;
                    let eoh_pct = day.candles[start_idx..]
                        .iter()
                        .take_while(|c| c.timestamp <= eoh_timestamp)
                        .last()
                        .map(|c| ((c.close - baseline) / baseline) * 100.0);

                    // Calcolo dei limiti orari numerici (09:25:00 = 33900 secondi dalla mezzanotte)
                    let end_idx = if session.id == "LONDON_OPEN" {
                        let london_end_timestamp = day_start_timestamp + 33900;
                        day.candles.iter()
                            .position(|c| c.timestamp == london_end_timestamp)
                            .unwrap_or(day.candles.len() - 1)
                    } else {
                        day.candles.len() - 1
                    };
                    let actual_end = if end_idx > start_idx { end_idx } else { day.candles.len() - 1 };

                    let mut max_high = baseline;
                    let mut min_low = baseline;
                    let last_close = day.candles[actual_end].close;

                    for c in &day.candles[start_idx..=actual_end] {
                        if c.high > max_high { max_high = c.high; }
                        if c.low < min_low { min_low = c.low; }
                    }

                    outcomes.push(crate::models::MatchOutcome {
                        timestamp: day.candles[start_idx].timestamp,
                        distance: dist,
                        eod_ret_pct: ((last_close - baseline) / baseline) * 100.0,
                        mfe_pct: ((max_high - baseline) / baseline) * 100.0,
                        mae_pct: ((min_low - baseline) / baseline) * 100.0,
                        eoh_pct: eoh_pct.unwrap_or(0.0),
                    });
                }
            }
        }

        outcomes.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
        outcomes.into_iter().take(top_k).collect()
    }

    pub fn get_day_candles(&self, date: u32, include_prev: bool) -> Vec<crate::models::ChartDataResponse> {
        let pos = self.dataset.iter().position(|d| d.date == date);
        let Some(pos) = pos else { return Vec::new(); };

        let prev_candles = if include_prev && pos > 0 {
            self.dataset[pos - 1].candles.as_slice()
        } else {
            &[]
        };

        let curr_candles = &self.dataset[pos].candles;

        prev_candles.iter()
            .chain(curr_candles.iter())
            .map(|c| crate::models::ChartDataResponse {
                timestamp: c.timestamp, // Espone direttamente il timestamp lineare alla UI/grafico
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
            })
            .collect()
    }

    pub fn find_context_matches(
        &self,
        current_context: &crate::models::Context,
        top_k: usize,
        session: &crate::models::SessionConfig,
    ) -> Vec<crate::models::MatchOutcome> {
        let mut outcomes = Vec::new();
        let session_offset_secs = Self::get_ny_seconds_for_session(session);

        for (i, day) in self.dataset.iter().enumerate() {
            if i == 0 || day.candles.is_empty() { continue; }

            let day_start_timestamp = (day.candles[0].timestamp / 86400) * 86400;
            let target_start_timestamp = day_start_timestamp + session_offset_secs;

            let Ok(start_idx) = day.candles.binary_search_by_key(&target_start_timestamp, |c| c.timestamp) else {
                continue;
            };

            let prev_day = &self.dataset[i - 1];
            let prev_close = match prev_day.candles.last() {
                Some(c) => c.close,
                None => continue,
            };
            let prev_high = prev_day.candles.iter().map(|c| c.high).fold(f32::NEG_INFINITY, f32::max);
            let prev_low  = prev_day.candles.iter().map(|c| c.low).fold(f32::INFINITY, f32::min);

            let (intraday_high, intraday_low) = if start_idx == 0 {
                let open = day.candles[0].open;
                (open, open)
            } else {
                let h = day.candles[0..start_idx].iter().map(|c| c.high).fold(f32::NEG_INFINITY, f32::max);
                let l = day.candles[0..start_idx].iter().map(|c| c.low).fold(f32::INFINITY, f32::min);
                (h, l)
            };

            let session_open = day.candles[start_idx].open;
            let hist_context = crate::models::Context::new(
                session_open,
                prev_close,
                prev_high,
                prev_low,
                intraday_high,
                intraday_low,
            );

            let dist = current_context.distance(&hist_context);
            let eoh_timestamp = target_start_timestamp + 3600;
            let baseline = session_open;
            
            let eoh_pct = day.candles[start_idx..]
                .iter()
                .take_while(|c| c.timestamp <= eoh_timestamp)
                .last()
                .map(|c| ((c.close - baseline) / baseline) * 100.0);

            let end_idx = if session.id == "LONDON_OPEN" {
                let london_end_timestamp = day_start_timestamp + 33900;
                day.candles.iter()
                    .position(|c| c.timestamp == london_end_timestamp)
                    .unwrap_or(day.candles.len() - 1)
            } else {
                day.candles.len() - 1
            };
            let actual_end = if end_idx > start_idx { end_idx } else { day.candles.len() - 1 };

            let mut max_high = baseline;
            let mut min_low  = baseline;
            let last_close   = day.candles[actual_end].close;

            for c in &day.candles[start_idx..=actual_end] {
                if c.high > max_high { max_high = c.high; }
                if c.low  < min_low  { min_low  = c.low;  }
            }

            outcomes.push(crate::models::MatchOutcome {
                timestamp: day.candles[start_idx].timestamp,
                distance: dist,
                eod_ret_pct: ((last_close - baseline) / baseline) * 100.0,
                mfe_pct: ((max_high - baseline) / baseline) * 100.0,
                mae_pct: ((min_low  - baseline) / baseline) * 100.0,
                eoh_pct: eoh_pct.unwrap_or(0.0),
            });
        }

        outcomes.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
        outcomes.into_iter().take(top_k).collect()
    }
}