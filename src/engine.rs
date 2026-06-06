use crate::models::{CandleStore, NormalizedCandle, RawCandle};
use chrono::{Timelike, TimeZone};
use chrono_tz::America::New_York;
use std::str::FromStr;

pub struct SearchEngine {
    pub dataset: CandleStore,
}

impl SearchEngine {
    pub fn new(dataset: CandleStore) -> Self {
        Self { dataset }
    }

    fn get_ny_seconds_for_session(session: &crate::models::SessionConfig, current_date: chrono::NaiveDate) -> u32 {

        let session_tz = chrono_tz::Tz::from_str(&session.timezone)
            .unwrap_or(chrono_tz::Europe::London);

        // Use the actual processing date to dynamic calculate DST shifts
        let session_datetime = current_date.and_time(session.start_time);

        let localized_session = match session_tz.from_local_datetime(&session_datetime).single() {
            Some(dt) => dt,
            None => return 0, 
        };

        let datetime_in_ny = localized_session.with_timezone(&New_York);

        let hour   = datetime_in_ny.hour()   as u32;
        let minute = datetime_in_ny.minute() as u32;
        let second = datetime_in_ny.second() as u32;

        (hour * 3600) + (minute * 60) + second
    }

    fn calculate_distance(current: &[NormalizedCandle<f32>], historical: &[NormalizedCandle<f32>]) -> f32 {
        let mut distance = 0.0;
        for (c, h) in current.iter().zip(historical.iter()) {
            distance += (c.body_pct      - h.body_pct).powi(2);
            distance += (c.wick_up_pct   - h.wick_up_pct).powi(2);
            distance += (c.wick_down_pct - h.wick_down_pct).powi(2);
            distance += (c.range_pct     - h.range_pct).powi(2);
        }
        distance.sqrt()
    }

    pub fn find_matches(
        &self,
        current_pattern: &[NormalizedCandle<f32>],
        top_k: usize,
        session: &crate::models::SessionConfig,
        timeframe: u32,
    ) -> Vec<crate::models::MatchOutcome> {
        let len = current_pattern.len();
        let mut outcomes = Vec::new();

        //Extract the actual calendar date from the first candle
        let current_dt = New_York.timestamp_opt(current_pattern[0].timestamp as i64, 0).unwrap();
        let current_date = current_dt.date_naive();

        let session_offset_secs = Self::get_ny_seconds_for_session(session, current_date);

        for i in 0..self.dataset.len() {
            let (_, raw_day_candles) = match self.dataset.get(i) { Some(d) => d, None => continue };
            if raw_day_candles.is_empty() { continue; }

            // Genera il timeframe al volo se diverso da 5
            let owned_candles;
            let candles: &[RawCandle<f32>] = if timeframe == 5 {
                raw_day_candles
            } else {
                owned_candles = crate::parser::resample_slice(raw_day_candles, timeframe);
                &owned_candles
            };

            let day_start_timestamp    = candles[0].timestamp;
            let target_start_timestamp = day_start_timestamp + session_offset_secs;

            let start_idx = match candles.binary_search_by_key(&target_start_timestamp, |c| c.timestamp) {
                Ok(idx) => idx,
                Err(idx) => {
                    if idx < candles.len() {
                        idx
                    } else {
                        continue;
                    }
                }
            };
            if candles.len() < start_idx + len { continue; }

            let baseline = candles[start_idx].open;
            let historical_pattern: Vec<NormalizedCandle<f32>> = candles[start_idx..start_idx + len]
                .iter()
                .map(|r| NormalizedCandle::from_raw(r, baseline))
                .collect();

            let dist = Self::calculate_distance(current_pattern, &historical_pattern);

            let eoh_timestamp = target_start_timestamp + 3600;
            let eoh_pct = candles[start_idx..]
                .iter()
                .take_while(|c| c.timestamp <= eoh_timestamp)
                .last()
                .map(|c| ((c.close - baseline) / baseline) * 100.0);

            let end_idx = if session.id == "LONDON_OPEN" {
                let london_end = day_start_timestamp + 33900;
                candles.iter().position(|c| c.timestamp == london_end).unwrap_or(candles.len() - 1)
            } else {
                candles.len() - 1
            };
            let actual_end = if end_idx > start_idx { end_idx } else { candles.len() - 1 };

            let mut max_high = baseline;
            let mut min_low  = baseline;
            let last_close   = candles[actual_end].close;
            for c in &candles[start_idx..=actual_end] {
                if c.high > max_high { max_high = c.high; }
                if c.low  < min_low  { min_low  = c.low;  }
            }

            // --- Calcolo movimento candela successiva (Punto X + 1) ---
            let next_idx = start_idx + len;
            let (next_up_pct, next_down_pct) = if next_idx < candles.len() {
                let next_c = &candles[next_idx];
                
                if baseline > 0.0 {
                    (
                        ((next_c.high - next_c.open) / baseline) * 100.0,
                        ((next_c.open - next_c.low) / baseline) * 100.0,
                    )
                } else {
                    (0.0, 0.0)
                }
            } else {
                (0.0, 0.0)
            };
            // -----------------------------------------------------------------------

            outcomes.push(crate::models::MatchOutcome {
                timestamp:   candles[start_idx].timestamp,
                distance:    dist,
                eod_ret_pct: ((last_close - baseline) / baseline) * 100.0,
                mfe_pct:     ((max_high   - baseline) / baseline) * 100.0,
                mae_pct:     ((min_low    - baseline) / baseline) * 100.0,
                eoh_pct:     eoh_pct.unwrap_or(0.0),
                next_candle_up_pct: Some(next_up_pct),
                next_candle_down_pct: Some(next_down_pct),
            });
        }

        outcomes.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
        outcomes.into_iter().take(top_k).collect()
    }

    pub fn get_day_candles(&self, date: u32, timeframe_min: u32, include_prev: bool ) -> Vec<crate::models::ChartDataResponse> {
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

        // Resampling giorno precedente
        let owned_prev;
        let prev = if timeframe_min == 5 || raw_prev.is_empty() {
            raw_prev
        } else {
            owned_prev = crate::parser::resample_slice(raw_prev, timeframe_min);
            &owned_prev
        };

        // Resampling giorno corrente
        let owned_curr;
        let curr = if timeframe_min == 5 {
            raw_curr
        } else {
            owned_curr = crate::parser::resample_slice(raw_curr, timeframe_min);
            &owned_curr
        };

        // Unione e mappatura
        prev.iter().chain(curr.iter())
            .map(|c| crate::models::ChartDataResponse {
                timestamp: c.timestamp,
                open: c.open, high: c.high, low: c.low, close: c.close,
            })
            .collect()
    }

    pub fn find_context_matches(
    &self,
    current_context: &crate::models::Context,
    top_k: usize,
    session: &crate::models::SessionConfig,
    timestamp: u32,
) -> Vec<crate::models::MatchOutcome> {
    let mut outcomes = Vec::new();

    let current_dt = New_York.timestamp_opt(timestamp as i64, 0).unwrap();
    let current_date = current_dt.date_naive();

    let session_offset_secs = Self::get_ny_seconds_for_session(session, current_date);

    for i in 1..self.dataset.len() {
        let (_, candles)      = match self.dataset.get(i)     { Some(d) => d, None => continue };
        let (_, prev_candles) = match self.dataset.get(i - 1) { Some(d) => d, None => continue };
        if candles.is_empty() { continue; }

        let day_start_timestamp = candles[0].timestamp;
        let target_start_timestamp = day_start_timestamp + session_offset_secs;

        let start_idx = match candles.binary_search_by_key(&target_start_timestamp, |c| c.timestamp) {
            Ok(idx) => idx,
            Err(idx) => {
                if idx < candles.len() {
                    idx
                } else {
                    continue;
                }
            }
        };

        let prev_close = match prev_candles.last() { Some(c) => c.close, None => continue };
        let prev_high  = prev_candles.iter().map(|c| c.high).fold(f32::NEG_INFINITY, f32::max);
        let prev_low   = prev_candles.iter().map(|c| c.low).fold(f32::INFINITY,     f32::min);

        let (intraday_high, intraday_low) = if start_idx == 0 {
            let open = candles[0].open;
            (open, open)
        } else {
            let h = candles[0..start_idx].iter().map(|c| c.high).fold(f32::NEG_INFINITY, f32::max);
            let l = candles[0..start_idx].iter().map(|c| c.low).fold(f32::INFINITY,     f32::min);
            (h, l)
        };

        let session_open = candles[start_idx].open;
        let hist_context = crate::models::Context::new(
            session_open, prev_close, prev_high, prev_low, intraday_high, intraday_low,
        );

        let dist          = current_context.distance(&hist_context);
        let baseline      = session_open;
        let eoh_timestamp = target_start_timestamp + 3600;

        let eoh_pct = candles[start_idx..]
            .iter()
            .take_while(|c| c.timestamp <= eoh_timestamp)
            .last()
            .map(|c| ((c.close - baseline) / baseline) * 100.0);

        let end_idx = if session.id == "LONDON_OPEN" {
            let london_end = day_start_timestamp + 33900;
            candles.iter().position(|c| c.timestamp == london_end).unwrap_or(candles.len() - 1)
        } else {
            candles.len() - 1
        };
        let actual_end = if end_idx > start_idx { end_idx } else { candles.len() - 1 };

        let mut max_high = baseline;
        let mut min_low  = baseline;
        let last_close   = candles[actual_end].close;
        for c in &candles[start_idx..=actual_end] {
            if c.high > max_high { max_high = c.high; }
            if c.low  < min_low  { min_low  = c.low;  }
        }

        outcomes.push(crate::models::MatchOutcome {
            timestamp:   candles[start_idx].timestamp,
            distance:    dist,
            eod_ret_pct: ((last_close - baseline) / baseline) * 100.0,
            mfe_pct:     ((max_high   - baseline) / baseline) * 100.0,
            mae_pct:     ((min_low    - baseline) / baseline) * 100.0,
            eoh_pct:     eoh_pct.unwrap_or(0.0),
            next_candle_up_pct: None, // Non calcolato in questo contesto
            next_candle_down_pct: None, // Non calcolato in questo contesto
        });
    }

    outcomes.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
    outcomes.into_iter().take(top_k).collect()
}
}