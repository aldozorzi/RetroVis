// src/parser.rs
use std::error::Error;                    // Standard error trait
use std::fs::File;                         // File system access
use csv::ReaderBuilder;                    // Specialized CSV parser with custom settings
use chrono::{NaiveTime};        // Time and date handling
use crate::models::{                       // Importing our defined structures
    RawCandle, 
    NormalizedCandle, 
    DayResult, 
    HistoricalPattern
};
pub fn load_and_process(
    path: &str, 
    pattern_size: usize // Flexible: can be 6, 12, or whatever
) -> Result<Vec<HistoricalPattern<f32>>, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut reader = ReaderBuilder::new().has_headers(false).delimiter(b';').from_reader(file);

    let mut patterns = Vec::new();
    let mut day_candles = Vec::new();
    let mut baseline = 0.0;
    let mut max_high = 0.0;
    let mut min_low = f32::MAX;
    let mut is_active = false;

    let open_t = NaiveTime::from_hms_opt(9, 30, 0).unwrap();
    let close_t = NaiveTime::from_hms_opt(16, 0, 0).unwrap();

    for result in reader.deserialize() {
        let r: RawCandle<f32> = result?;
        let current_t = NaiveTime::parse_from_str(&r.time, "%H:%M:%S")?;

        if current_t == open_t {
            baseline = r.open;
            max_high = r.high;
            min_low = r.low;
            day_candles.clear();
            is_active = true;
        }

        if is_active {
            // Tracking absolute extremes for the session
            if r.high > max_high { max_high = r.high; }
            if r.low < min_low { min_low = r.low; }

            // Collect and normalize until pattern_size is reached
            if day_candles.len() < pattern_size {
                day_candles.push(NormalizedCandle::from_raw(&r, baseline));
            }

            if current_t == close_t {
                patterns.push(HistoricalPattern::<f32> {
                    candles: day_candles.clone(),
                    result: DayResult {
                        date: r.date.clone(),
                        // These are now fully invariant (%)
                        eod_ret_pct: ((r.close - baseline as f32) / baseline as f32) * 100.0,
                        mfe_pct: ((max_high - baseline as f32) / baseline as f32) * 100.0,
                        mae_pct: ((min_low - baseline as f32) / baseline as f32) * 100.0,
                        open_0930: baseline,
                    },
                });
                is_active = false;
            }
        }
    }
    Ok(patterns)
}