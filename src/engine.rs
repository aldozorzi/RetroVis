use crate::models::{CandleStore, NormalizedCandle, RawCandle};
use chrono::{Timelike, TimeZone};
use chrono_tz::America::New_York;
use std::str::FromStr;
use std::borrow::Cow;

// CSV market data files are stored at 5-minute native resolution.
pub const NATIVE_TIMEFRAME_MIN: u32 = 5;

// Window used to sample "end of hour" price action shortly after a session opens.
const END_OF_HOUR_WINDOW_SECS: u32 = 3_600;

// Duration of the LONDON_OPEN session window, measured from the start
// of that day's data (not from the session's opening time itself).
const LONDON_SESSION_WINDOW_SECS: u32 = 33_900; // 9h 25m

pub struct SearchEngine {
    pub dataset: CandleStore,
}

struct SessionStart {
    start_idx: usize,
    day_start_timestamp: u32,
    target_start_timestamp: u32,
}

impl SearchEngine {
    pub fn new(dataset: CandleStore) -> Self {
        Self { dataset }
    }

    fn get_ny_seconds_for_session(session: &crate::models::SessionConfig, current_date: chrono::NaiveDate) -> u32 {

        let session_tz = chrono_tz::Tz::from_str(&session.timezone)
            .unwrap_or(chrono_tz::Europe::London);

        // Use the actual processing date to dynamically calculate DST shifts
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

        let current_dt = New_York.timestamp_opt(current_pattern[0].timestamp as i64, 0).unwrap();
        let current_date = current_dt.date_naive();
        let session_offset_secs = Self::get_ny_seconds_for_session(session, current_date);

        for i in 0..self.dataset.len() {
            let (_, raw_day_candles) = match self.dataset.get(i) { Some(d) => d, None => continue };
            if raw_day_candles.is_empty() { continue; }

            let candles = Self::maybe_resample(raw_day_candles, timeframe);

            let session_start = match Self::find_session_start(&candles, session_offset_secs) {
                Some(s) => s,
                None => continue,
            };
            let SessionStart { start_idx, day_start_timestamp, target_start_timestamp } = session_start;

            if candles.len() < start_idx + len { continue; }

            let baseline = candles[start_idx].open;
            let historical_pattern: Vec<NormalizedCandle<f32>> = candles[start_idx..start_idx + len]
                .iter()
                .map(|r| NormalizedCandle::from_raw(r, baseline))
                .collect();
            let dist = Self::calculate_distance(current_pattern, &historical_pattern);

            // Price action of the candle immediately after the matched pattern (point X + 1).
            let next_idx = start_idx + len;
            let (next_up_pct, next_down_pct) = if next_idx < candles.len() && baseline > 0.0 {
                let next_c = &candles[next_idx];
                (
                    Self::pct_diff(next_c.high, next_c.open, baseline),
                    Self::pct_diff(next_c.open, next_c.low, baseline),
                )
            } else {
                (0.0, 0.0)
            };

            outcomes.push(Self::build_outcome(
                &candles, session, start_idx, day_start_timestamp, target_start_timestamp,
                baseline, dist, Some(next_up_pct), Some(next_down_pct),
            ));
        }

        outcomes.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        outcomes.into_iter().take(top_k).collect()
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

            let session_start = match Self::find_session_start(&candles, session_offset_secs) {
                Some(s) => s,
                None => continue,
            };
            let SessionStart { start_idx, day_start_timestamp, target_start_timestamp } = session_start;

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

            let dist     = current_context.distance(&hist_context);
            let baseline = session_open;

            outcomes.push(Self::build_outcome(
                &candles, session, start_idx, day_start_timestamp, target_start_timestamp,
                baseline, dist, None, None,
            ));
        }

        outcomes.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        outcomes.into_iter().take(top_k).collect()
    }

    fn find_session_start(candles: &[RawCandle<f32>], session_offset_secs: u32) -> Option<SessionStart> {
        let day_start_timestamp = candles[0].timestamp;
        let target_start_timestamp = day_start_timestamp + session_offset_secs;
        let start_idx = match candles.binary_search_by_key(&target_start_timestamp, |c| c.timestamp) {
            Ok(idx) => idx,
            Err(idx) if idx < candles.len() => idx,
            Err(_) => return None,
        };
        Some(SessionStart { start_idx, day_start_timestamp, target_start_timestamp })
    }

    fn build_outcome(
        candles: &[RawCandle<f32>],
        session: &crate::models::SessionConfig,
        start_idx: usize,
        day_start_timestamp: u32,
        target_start_timestamp: u32,
        baseline: f32,
        distance: f32,
        next_candle_up_pct: Option<f32>,
        next_candle_down_pct: Option<f32>,
    ) -> crate::models::MatchOutcome {
        let eoh_pct = Self::eoh_pct(candles, start_idx, target_start_timestamp, baseline);
        let actual_end = Self::find_session_end_idx(candles, session, day_start_timestamp, start_idx);
        let (max_high, min_low, last_close) = Self::session_extremes(candles, start_idx, actual_end, baseline);

        crate::models::MatchOutcome {
            timestamp: candles[start_idx].timestamp,
            distance,
            eod_ret_pct: Self::pct_diff(last_close, baseline, baseline),
            mfe_pct: Self::pct_diff(max_high, baseline, baseline),
            mae_pct: Self::pct_diff(min_low, baseline, baseline),
            eoh_pct,
            next_candle_up_pct,
            next_candle_down_pct,
        }
    }

    fn find_session_end_idx(
        candles: &[RawCandle<f32>],
        session: &crate::models::SessionConfig,
        day_start_timestamp: u32,
        start_idx: usize,
    ) -> usize {
        let end_idx = if session.id == "LONDON_OPEN" {
            let london_end = day_start_timestamp + LONDON_SESSION_WINDOW_SECS;
            candles.iter().position(|c| c.timestamp == london_end).unwrap_or(candles.len() - 1)
        } else {
            candles.len() - 1
        };
        if end_idx > start_idx { end_idx } else { candles.len() - 1 }
    }

    fn session_extremes(
        candles: &[RawCandle<f32>],
        start_idx: usize,
        actual_end: usize,
        baseline: f32,
    ) -> (f32, f32, f32) {
        let mut max_high = baseline;
        let mut min_low  = baseline;
        for c in &candles[start_idx..=actual_end] {
            if c.high > max_high { max_high = c.high; }
            if c.low  < min_low  { min_low  = c.low;  }
        }
        (max_high, min_low, candles[actual_end].close)
    }

    fn eoh_pct(candles: &[RawCandle<f32>], start_idx: usize, target_start_timestamp: u32, baseline: f32) -> f32 {
        let eoh_timestamp = target_start_timestamp + END_OF_HOUR_WINDOW_SECS;
        candles[start_idx..]
            .iter()
            .take_while(|c| c.timestamp <= eoh_timestamp)
            .last()
            .map(|c| Self::pct_diff(c.close, baseline, baseline))
            .unwrap_or(0.0)
    }

    #[inline]
    fn pct_diff(a: f32, b: f32, baseline: f32) -> f32 {
        ((a - b) / baseline) * 100.0
    }


    // CSV files are stored at 5-minute native resolution — resampling only
    // kicks in when the caller asks for a coarser timeframe.
    pub(crate) fn maybe_resample<'a>(raw: &'a [RawCandle<f32>], timeframe_min: u32) -> Cow<'a, [RawCandle<f32>]> {
        if timeframe_min == NATIVE_TIMEFRAME_MIN {
            Cow::Borrowed(raw)
        } else {
            Cow::Owned(crate::parser::resample_slice(raw, timeframe_min))
        }
    }
}