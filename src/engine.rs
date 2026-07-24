use crate::models::{CandleStore, NormalizedCandle, RawCandle};
use chrono::{Timelike, TimeZone, NaiveDate};
use chrono_tz::America::New_York;
use std::str::FromStr;
use std::borrow::Cow;
use std::collections::HashSet;

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

    fn decode_naive_ny_date(timestamp: u32) -> NaiveDate {
        chrono::DateTime::from_timestamp(timestamp as i64, 0)
        .unwrap()
        .naive_utc()
        .date()
    }

    pub fn find_matches(
        &self,
        current_pattern: &[NormalizedCandle<f32>],
        top_k: usize,
        session: &crate::models::SessionConfig,
        timeframe: u32,
        valid_dates: Option<&HashSet<u32>>,
    ) -> Vec<crate::models::MatchOutcome> {
        let len = current_pattern.len();
        let mut outcomes = Vec::new();

        // --- DEBUG: attiva solo per la query incriminata ---
        let debug_this_call = current_pattern[0].timestamp as u32 == 1784257200;
        let debug_target_date: u32 = 20151209; // <-- metti qui la data del match sospetto (YYYYMMDD)
        // -----------------------------------------------------

        let current_date = Self::decode_naive_ny_date(current_pattern[0].timestamp);
        let session_offset_secs = Self::get_ny_seconds_for_session(session, current_date);


        for i in 0..self.dataset.len() {
            let (day_date_num, raw_day_candles) = match self.dataset.get(i) { Some(d) => d, None => continue };
            if let Some(dates) = valid_dates {
                if !dates.contains(&day_date_num) { continue; }
            }
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

            // --- DEBUG: stampa solo per il giorno sospetto, dentro la query incriminata ---
            if debug_this_call && day_date_num == debug_target_date {
                let day_start_ny = chrono::DateTime::from_timestamp(day_start_timestamp as i64, 0).unwrap().naive_utc();
                println!("=== START NEW CANDLE ===");
                println!("day_start_timestamp NY = {} (ts={})", day_start_ny.format("%Y-%m-%d %H:%M:%S"), day_start_timestamp);
                println!("session_offset_secs = {} ({}h{}m)", session_offset_secs, session_offset_secs / 3600, (session_offset_secs % 3600) / 60);
                println!("target_start_timestamp NY = {}", chrono::DateTime::from_timestamp(target_start_timestamp as i64, 0).unwrap().naive_utc().format("%Y-%m-%d %H:%M:%S"));
                println!("=== DEBUG giorno {} start_idx={} baseline={} ===", day_date_num, start_idx, baseline);
                for (i, (cur, raw_hist)) in current_pattern.iter()
                    .zip(candles[start_idx..start_idx + len].iter())
                    .enumerate()
                {
                    let hist = NormalizedCandle::from_raw(raw_hist, baseline);

                    // decodifica il timestamp come NY wall-clock (nessuno shift di fuso)
                    let ny_dt = chrono::DateTime::from_timestamp(raw_hist.timestamp as i64, 0)
                        .unwrap()
                        .naive_utc();

                    println!(
                        "candela {} @ {} NY (ts={}): storico OHLC o={:.2} h={:.2} l={:.2} c={:.2} | cur(body={:.4} wu={:.4} wd={:.4} rng={:.4}) | hist(body={:.4} wu={:.4} wd={:.4} rng={:.4})",
                        i, ny_dt.format("%Y-%m-%d %H:%M:%S"), raw_hist.timestamp,
                        raw_hist.open, raw_hist.high, raw_hist.low, raw_hist.close,
                        cur.body_pct, cur.wick_up_pct, cur.wick_down_pct, cur.range_pct,
                        hist.body_pct, hist.wick_up_pct, hist.wick_down_pct, hist.range_pct
                    );
                }
                println!("=== distanza totale = {} ===", dist);
            }
            // --------------------------------------------------------------------------

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
        valid_dates: Option<&HashSet<u32>>,
    ) -> Vec<crate::models::MatchOutcome> {
        let mut outcomes = Vec::new();

        let current_date = Self::decode_naive_ny_date(timestamp);
        let session_offset_secs = Self::get_ny_seconds_for_session(session, current_date);

        for i in 1..self.dataset.len() {
            let (date_num, candles)       = match self.dataset.get(i)     { Some(d) => d, None => continue };
            let (_, prev_candles) = match self.dataset.get(i - 1) { Some(d) => d, None => continue };
            // For news-event sessions, only search on dates present in the valid-dates file
            if let Some(dates) = valid_dates {
                if !dates.contains(&date_num) { continue; }
            }
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
        // Cerca la candela di mezzanotte NY nello slice, invece di assumere candles[0].
        let midnight_ts = (candles[0].timestamp / 86_400) * 86_400;

        let day_start_timestamp = match candles.binary_search_by_key(&midnight_ts, |c| c.timestamp) {
            Ok(_) => midnight_ts,
            Err(_) => return None, // mezzanotte non presente in questo slice -> giorno da scartare
        };

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
        let end_idx = if let Some(duration) = session.duration_secs {
            // Fixed-duration session (e.g. news events): lasts `duration` seconds from session start
            let end_ts = candles[start_idx].timestamp + duration;
            candles.iter()
                .position(|c| c.timestamp >= end_ts)
                .unwrap_or(candles.len() - 1)
        } else if session.id == "LONDON_OPEN" {
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


#[cfg(test)]
mod session_alignment_tests {
    use super::*;
    use crate::models::RawCandle;

    /// Costruisce un timestamp "naive-as-NY" per una data/ora, con lo stesso
    /// schema di encoding usato da parser.rs::calculate_timestamp.
    fn naive_ts(y: i32, m: u32, d: u32, h: u32, min: u32) -> u32 {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
            .and_utc()
            .timestamp() as u32
    }

    fn dummy_candle(ts: u32) -> RawCandle<f32> {
        RawCandle { timestamp: ts, open: 100.0, high: 101.0, low: 99.0, close: 100.5 }
    }

    // --- decode_naive_ny_date ---

    #[test]
    fn decode_naive_ny_date_no_timezone_shift() {
        // Le 03:00 NY del 17/07/2026 devono restare 03:00 del 17/07/2026,
        // MAI shiftate da una conversione di fuso (era il bug C#/New_York.timestamp_opt).
        let ts = naive_ts(2026, 7, 17, 3, 0);
        let date = SearchEngine::decode_naive_ny_date(ts);
        assert_eq!(date, chrono::NaiveDate::from_ymd_opt(2026, 7, 17).unwrap());
    }

    // --- find_session_start: giorno normale, mezzanotte presente ---

    #[test]
    fn find_session_start_normal_day_finds_midnight_correctly() {
        let candles: Vec<RawCandle<f32>> = (0..12) // 00:00 .. 05:30, ogni 30 min
            .map(|i| dummy_candle(naive_ts(2015, 12, 9, 0, 0) + i * 1800))
            .collect();

        let session_offset_secs = 3 * 3600; // Londra: 03:00 NY
        let result = SearchEngine::find_session_start(&candles, session_offset_secs);

        assert!(result.is_some());
        let s = result.unwrap();
        assert_eq!(s.day_start_timestamp, naive_ts(2015, 12, 9, 0, 0));
        assert_eq!(s.target_start_timestamp, naive_ts(2015, 12, 9, 3, 0));
        assert_eq!(candles[s.start_idx].timestamp, naive_ts(2015, 12, 9, 3, 0));
    }

    // --- find_session_start: riapertura domenicale Globex (niente mezzanotte) ---

    #[test]
    fn find_session_start_sunday_reopen_returns_none() {
        // Domenica: il mercato riapre alle 18:00 NY, la mezzanotte non esiste nello slice.
        // Regressione diretta del bug: prima veniva usato candles[0] (18:00) come falsa
        // "mezzanotte", producendo un target_start_timestamp di 21:00 (18:00+3h) invece
        // di scartare il giorno.
        let candles: Vec<RawCandle<f32>> = (0..12) // 18:00 .. 23:30
            .map(|i| dummy_candle(naive_ts(2022, 5, 22, 18, 0) + i * 1800))
            .collect();

        let session_offset_secs = 3 * 3600;
        let result = SearchEngine::find_session_start(&candles, session_offset_secs);

        assert!(result.is_none(), "un giorno senza mezzanotte deve essere scartato, non produrre un falso match");
    }

    // --- find_session_start: mezzanotte presente ma target oltre la fine dello slice ---

    #[test]
    fn find_session_start_target_beyond_slice_returns_none() {
        let candles: Vec<RawCandle<f32>> = (0..4) // 00:00 .. 01:30 soltanto
            .map(|i| dummy_candle(naive_ts(2015, 12, 9, 0, 0) + i * 1800))
            .collect();

        let session_offset_secs = 3 * 3600; // target 03:00, fuori dallo slice disponibile
        let result = SearchEngine::find_session_start(&candles, session_offset_secs);

        assert!(result.is_none());
    }
}

#[cfg(test)]
mod find_matches_integration_tests {
    use super::*;
    use crate::models::{CandleStore, RawCandle, NormalizedCandle, SessionConfig};

    fn naive_ts(y: i32, m: u32, d: u32, h: u32, min: u32) -> u32 {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
            .and_utc()
            .timestamp() as u32
    }

    fn candle(ts: u32, o: f32, h: f32, l: f32, c: f32) -> RawCandle<f32> {
        RawCandle { timestamp: ts, open: o, high: h, low: l, close: c }
    }

    fn london_session() -> SessionConfig {
        SessionConfig {
            id: "LONDON_OPEN".to_string(),
            start_time: chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap(), // 8am London
            timezone: "Europe/London".to_string(),
            duration_secs: None,
        }
    }

    /// Dataset con due giorni:
    /// - una domenica (riapertura Globex 18:00, NIENTE mezzanotte) -> deve essere scartata
    /// - un giorno feriale normale (mezzanotte presente, apertura Londra alle 03:00 NY)
    ///   con un pattern quasi identico a quello cercato -> deve essere il match trovato
    fn build_test_dataset() -> CandleStore {
        let mut candles = Vec::new();
        let mut days = Vec::new();

        // Giorno 1: domenica 2022-05-22, apertura Globex 18:00, nessuna mezzanotte nello slice.
        let sunday_start = candles.len();
        for i in 0..12 {
            candles.push(candle(
                naive_ts(2022, 5, 22, 18, 0) + i * 1800,
                100.0, 101.0, 99.0, 100.0,
            ));
        }
        days.push((20220522u32, sunday_start..candles.len()));

        // Giorno 2: feriale, 2015-12-09, mezzanotte presente, pattern noto alle 03:00-04:00.
        let weekday_start = candles.len();
        for i in 0..12 { // 00:00 .. 05:30
            candles.push(candle(
                naive_ts(2015, 12, 9, 0, 0) + i * 1800,
                200.0, 200.0, 200.0, 200.0,
            ));
        }
        // Sovrascrivi le 3 candele della sessione Londra (03:00, 03:30, 04:00) con
        // valori che matchano quasi esattamente il pattern corrente cercato sotto.
        candles[weekday_start + 6] = candle(naive_ts(2015, 12, 9, 3, 0), 100.0, 101.0, 99.5, 100.5);
        candles[weekday_start + 7] = candle(naive_ts(2015, 12, 9, 3, 30), 100.5, 100.8, 98.0, 98.5);
        candles[weekday_start + 8] = candle(naive_ts(2015, 12, 9, 4, 0), 98.5, 98.7, 97.0, 97.5);
        days.push((20151209u32, weekday_start..candles.len()));

        CandleStore { candles, days }
    }

    #[test]
    fn find_matches_skips_sunday_and_aligns_to_correct_session() {
        let dataset = build_test_dataset();
        let engine = SearchEngine::new(dataset);
        let session = london_session();

        // Pattern corrente: query "di oggi", 03:00/03:30/04:00 NY, quasi identico
        // alle candele storiche piazzate nel giorno feriale.
        let raw_current = vec![
            candle(naive_ts(2026, 7, 17, 3, 0), 100.0, 101.0, 99.5, 100.5),
            candle(naive_ts(2026, 7, 17, 3, 30), 100.5, 100.8, 98.0, 98.5),
            candle(naive_ts(2026, 7, 17, 4, 0), 98.5, 98.7, 97.0, 97.5),
        ];
        let baseline = 100.0;
        let current_pattern: Vec<NormalizedCandle<f32>> = raw_current
            .iter()
            .map(|r| NormalizedCandle::from_raw(r, baseline))
            .collect();

        let results = engine.find_matches(&current_pattern, 5, &session, 30, None);

        assert!(!results.is_empty(), "deve trovare almeno un match valido");
        // Il match trovato deve provenire dal giorno feriale (start_idx=6, ts 03:00 del 2015-12-09),
        // MAI dalla domenica (che non ha mezzanotte e va scartata).
        assert_eq!(results[0].timestamp, naive_ts(2015, 12, 9, 3, 0));
        // La distanza dev'essere piccola, visto che il pattern storico è quasi identico.
        assert!(results[0].distance < 1.0, "distanza troppo alta: {}", results[0].distance);
    }
}

#[cfg(test)]
mod session_offset_tests {
    use super::*;
    use crate::models::SessionConfig;

    fn london_8am() -> SessionConfig {
        SessionConfig {
            id: "LONDON_OPEN".to_string(),
            start_time: chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            timezone: "Europe/London".to_string(),
            duration_secs: None,
        }
    }

    #[test]
    fn london_open_in_full_summer_dst_is_3am_ny() {
        // Luglio: sia UK (BST, UTC+1) sia US (EDT, UTC-4) sono in DST.
        // 8:00 London = 7:00 UTC = 3:00 NY.
        let date = chrono::NaiveDate::from_ymd_opt(2026, 7, 17).unwrap();
        let offset = SearchEngine::get_ny_seconds_for_session(&london_8am(), date);
        assert_eq!(offset, 3 * 3600);
    }

    #[test]
    fn london_open_in_full_winter_is_3am_ny() {
        // Gennaio: nessuno dei due è in DST (UK=GMT UTC+0, US=EST UTC-5).
        // 8:00 London = 8:00 UTC = 3:00 NY. Stesso offset del caso estivo,
        // perché il differenziale London-NY resta 5h tutto l'anno TRANNE
        // nelle settimane di mismatch (vedi test sotto).
        let date = chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let offset = SearchEngine::get_ny_seconds_for_session(&london_8am(), date);
        assert_eq!(offset, 3 * 3600);
    }

    #[test]
    fn london_open_during_us_uk_dst_mismatch_week_shifts_by_one_hour() {
        // Nel 2026, gli USA passano a DST l'8 marzo, il Regno Unito il 29 marzo.
        // Nella finestra 8-29 marzo: US già in EDT (UTC-4), UK ancora in GMT (UTC+0).
        // 8:00 London (GMT) = 8:00 UTC = 4:00 NY (non più 3:00!).
        // Questo è esattamente il tipo di caso che il codice attuale gestisce male
        // se session_offset_secs viene calcolato una sola volta e riusato per
        // giorni storici che cadono in stagioni diverse.
        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let offset = SearchEngine::get_ny_seconds_for_session(&london_8am(), date);
        assert_eq!(offset, 4 * 3600, "durante il mismatch DST UK/US l'offset deve spostarsi di 1h");
    }
}