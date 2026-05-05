use crate::models::{HistoricalDay, NormalizedCandle};
use chrono::{NaiveDate, NaiveTime};

pub struct SearchEngine<T> {
    pub dataset: Vec<HistoricalDay<T>>,
}

impl SearchEngine<f32> {
    pub fn new(dataset: Vec<HistoricalDay<f32>>) -> Self {
        Self { dataset }
    }

    fn get_ny_time_for_session(date_str: &str, session: &crate::models::SessionConfig) -> Option<NaiveTime> {
        use chrono::TimeZone;
        use chrono_tz::Tz;

        let target_date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .or_else(|_| NaiveDate::parse_from_str(date_str, "%d/%m/%Y"))
            .ok()?; 

        let session_tz: Tz = session.timezone.parse().ok()?;
        let ny_tz: Tz = "America/New_York".parse().unwrap();

        let local_naive_dt = target_date.and_time(session.start_time);

        let local_dt = match session_tz.from_local_datetime(&local_naive_dt) {
            chrono::LocalResult::Single(dt) => dt,
            chrono::LocalResult::Ambiguous(dt_earliest, _) => dt_earliest,
            chrono::LocalResult::None => {
                session_tz
                    .from_local_datetime(&(local_naive_dt + chrono::Duration::hours(1)))
                    .single()?
            }
        };

        Some(local_dt.with_timezone(&ny_tz).time())
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

        for day in &self.dataset {
            
            let Some(start_t_ny) = Self::get_ny_time_for_session(&day.date, session) else {
                continue;
            };

            if let Some(start_idx) = day.candles.iter().position(|c| {
                // Nota: usiamo il formato corretto per il match dell'orario
                c.time == start_t_ny.format("%H:%M:%S").to_string()
            }) {
                if day.candles.len() >= start_idx + len {
                    let baseline = day.candles[start_idx].open;
                    
                    let historical_pattern: Vec<crate::models::NormalizedCandle<f32>> = day.candles[start_idx..start_idx + len]
                        .iter()
                        .map(|r| crate::models::NormalizedCandle::from_raw(r, baseline))
                        .collect();

                    let dist = Self::calculate_distance(current_pattern, &historical_pattern);
                    
                    let eoh_time_ny = start_t_ny + chrono::Duration::hours(1);
                    let eoh_time_str = eoh_time_ny.format("%H:%M:%S").to_string();
                    let eoh_pct = day.candles[start_idx..]
                        .iter()
                        .filter(|c| c.time <= eoh_time_str)
                        .last()
                        .map(|c| ((c.close - baseline) / baseline) * 100.0);

                    let end_idx = if session.id == "LONDON_OPEN" {
                        day.candles.iter()
                            .position(|c| c.time == "09:25:00")
                            .unwrap_or(day.candles.len() - 1)
                    } else {
                        day.candles.len() - 1
                    };
                    let actual_end = if end_idx > start_idx { end_idx } else { day.candles.len() - 1 };

                    // Calcolo MFE/MAE/Return sull'intera giornata rimanente
                    let mut max_high = baseline;
                    let mut min_low = baseline;
                    let last_close = day.candles[actual_end].close;

                    for c in &day.candles[start_idx..=actual_end] {
                        if c.high > max_high { max_high = c.high; }
                        if c.low < min_low { min_low = c.low; }
                    }

                    outcomes.push(crate::models::MatchOutcome {
                        date: day.date.clone(),
                        time: day.candles[start_idx].time.clone(),
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

    pub fn get_day_candles(&self, date: &str, include_prev: bool) -> Vec<crate::models::ChartDataResponse> {

        let pos = self.dataset.iter().position(|d| d.date == date);

        let Some(pos) = pos else {
            return Vec::new();
        };

        let prev_candles: &[crate::models::RawCandle<f32>] = if include_prev && pos > 0 {
            &self.dataset[pos - 1].candles
        } else {
            &[]
        };

        let curr_candles = &self.dataset[pos].candles;

        prev_candles.iter()
            .chain(curr_candles.iter())
            .map(|c| crate::models::ChartDataResponse {
                time: format!("{} {}", c.date, c.time),
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

        for (i, day) in self.dataset.iter().enumerate() {
            if i == 0 { continue; }

            let Some(start_t_ny) = Self::get_ny_time_for_session(&day.date, session) else {
                continue;
            };

            let Some(start_idx) = day.candles.iter().position(|c| {
                c.time == start_t_ny.format("%H:%M:%S").to_string()
            }) else {
                continue;
            };

            // prev day data
            let prev_day = &self.dataset[i - 1];
            let prev_close = match prev_day.candles.last() {
                Some(c) => c.close,
                None => continue,
            };
            let prev_high = prev_day.candles.iter().map(|c| c.high).fold(f32::NEG_INFINITY, f32::max);
            let prev_low  = prev_day.candles.iter().map(|c| c.low).fold(f32::INFINITY, f32::min);

            // Max/min current day before session start
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

            let eoh_time_ny = start_t_ny + chrono::Duration::hours(1);
            let eoh_time_str = eoh_time_ny.format("%H:%M:%S").to_string();
            let baseline = session_open;
            let eoh_pct = day.candles[start_idx..]
                .iter()
                .filter(|c| c.time <= eoh_time_str)
                .last()
                .map(|c| ((c.close - baseline) / baseline) * 100.0);

            let end_idx = if session.id == "LONDON_OPEN" {
                day.candles.iter()
                    .position(|c| c.time == "09:25:00")
                    .unwrap_or(day.candles.len() - 1)
            } else {
                day.candles.len() - 1
            };
            let actual_end = if end_idx > start_idx { end_idx } else { day.candles.len() - 1 };

            let baseline = session_open;
            let mut max_high = baseline;
            let mut min_low  = baseline;
            let last_close   = day.candles[actual_end].close;

            for c in &day.candles[start_idx..=actual_end] {
                if c.high > max_high { max_high = c.high; }
                if c.low  < min_low  { min_low  = c.low;  }
            }

            outcomes.push(crate::models::MatchOutcome {
                date: day.date.clone(),
                time: day.candles[start_idx].time.clone(),
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