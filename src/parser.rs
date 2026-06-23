use std::error::Error;
use std::fs::File;
use memmap2::Mmap;
use crate::models::{RawCandle, CandleStore};


use crate::engine::NATIVE_TIMEFRAME_MIN;

pub fn load_and_process(path: &str, symbol: &str) -> Result<CandleStore, Box<dyn Error>> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    let mut candles: Vec<RawCandle<f32>> = Vec::with_capacity(800_000);
    let mut days: Vec<(u32, std::ops::Range<usize>)> = Vec::with_capacity(5000);
    let mut current_date_num = 0u32;
    let mut day_start = 0usize;
    let mut start = 0;
    let buffer = &mmap[..];

    for i in 0..buffer.len() {
        if buffer[i] == b'\n' {
            let mut line = &buffer[start..i];
            start = i + 1;
            if line.ends_with(b"\r") {
                line = &line[..line.len() - 1];
            }
            if line.is_empty() { continue; }

            let mut fields = line.split(|&b| b == b';');
            let date_bytes   = match fields.next() { Some(f) => f, None => continue };
            let time_bytes   = match fields.next() { Some(f) => f, None => continue };
            let open_bytes   = match fields.next() { Some(f) => f, None => continue };
            let high_bytes   = match fields.next() { Some(f) => f, None => continue };
            let low_bytes    = match fields.next() { Some(f) => f, None => continue };
            let close_bytes  = match fields.next() { Some(f) => f, None => continue };

            let date_num  = match parse_date_to_u32(date_bytes) { Some(d) => d, None => continue };
            let open      = match parse_f32(open_bytes)   { Some(v) => v, None => continue };
            let high      = match parse_f32(high_bytes)   { Some(v) => v, None => continue };
            let low       = match parse_f32(low_bytes)    { Some(v) => v, None => continue };
            let close     = match parse_f32(close_bytes)  { Some(v) => v, None => continue };
            let timestamp = calculate_timestamp(date_bytes, time_bytes);

            if current_date_num != 0 && date_num != current_date_num {
                days.push((current_date_num, day_start..candles.len()));
                day_start = candles.len();
            }
            current_date_num = date_num;
            candles.push(RawCandle { timestamp, open, high, low, close });
        }
    }
    if day_start < candles.len() {
        days.push((current_date_num, day_start..candles.len()));
    }
    candles.shrink_to_fit();
    days.shrink_to_fit();
    println!("{} - {} days, {} candles", symbol, days.len(), candles.len());
    Ok(CandleStore { candles, days })
}

pub fn resample_slice(source_candles: &[RawCandle<f32>], to_min: u32) -> Vec<RawCandle<f32>> {
    let interval_secs = (to_min * 60) as u32;
    let ratio = (to_min / NATIVE_TIMEFRAME_MIN).max(1);
    let mut resampled = Vec::with_capacity(source_candles.len() / ratio as usize + 1);
    let mut bucket: Option<RawCandle<f32>> = None;

    for candle in source_candles {
        let ts = (candle.timestamp / interval_secs) * interval_secs;
        match &mut bucket {
            Some(b) if b.timestamp == ts => {
                if candle.high > b.high { b.high = candle.high; }
                if candle.low < b.low { b.low = candle.low; }
                b.close = candle.close;
            }
            _ => {
                if let Some(b) = bucket.take() { resampled.push(b); }
                bucket = Some(RawCandle {
                    timestamp: ts,
                    open: candle.open,
                    high: candle.high,
                    low: candle.low,
                    close: candle.close,
                });
            }
        }
    }
    if let Some(b) = bucket { resampled.push(b); }
    resampled
}

fn parse_f32(bytes: &[u8]) -> Option<f32> {
    let s = std::str::from_utf8(bytes).ok()?;
    s.trim().parse::<f32>().ok()
}

fn parse_date_to_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 10 { return None; }
    let year = std::str::from_utf8(&bytes[0..4]).ok()?.parse::<u32>().ok()?;
    let month = std::str::from_utf8(&bytes[5..7]).ok()?.parse::<u32>().ok()?;
    let day = std::str::from_utf8(&bytes[8..10]).ok()?.parse::<u32>().ok()?;
    Some(year * 10000 + month * 100 + day)
}

/// Computes a Unix timestamp directly from raw CSV bytes, bypassing chrono's
/// string parsing. This runs once per row over ~800k rows during load, so
/// skipping allocation/parsing overhead here meaningfully speeds up startup.
/// Uses Howard Hinnant's days-from-civil algorithm for the date math.
fn calculate_timestamp(date_bytes: &[u8], time_bytes: &[u8]) -> u32 {
    if date_bytes.len() < 10 || time_bytes.len() < 8 { return 0; }

    // Parse date (YYYY-MM-DD) as integers, avoiding string allocation.
    let year = ((date_bytes[0] - b'0') as i64 * 1000) + ((date_bytes[1] - b'0') as i64 * 100) + ((date_bytes[2] - b'0') as i64 * 10) + (date_bytes[3] - b'0') as i64;
    let month = ((date_bytes[5] - b'0') as i64 * 10) + (date_bytes[6] - b'0') as i64;
    let day = ((date_bytes[8] - b'0') as i64 * 10) + (date_bytes[9] - b'0') as i64;

    // Parse time (HH:MM:SS) the same way.
    let hour = ((time_bytes[0] - b'0') as i64 * 10) + (time_bytes[1] - b'0') as i64;
    let minute = ((time_bytes[3] - b'0') as i64 * 10) + (time_bytes[4] - b'0') as i64;
    let second = ((time_bytes[6] - b'0') as i64 * 10) + (time_bytes[7] - b'0') as i64;

    // Days-since-epoch via the civil calendar algorithm (handles leap years without branching).
    let mut y = year;
    if month <= 2 { y -= 1; }
    let era = y / 400;
    let y_in_era = y % 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = y_in_era * 365 + y_in_era / 4 - y_in_era / 100 + doy;
    let days_since_zero = era * 146097 + doe as i64;

    // 719468 = days between year 0 and 1970-01-01 (the Unix epoch).
    let days_since_1970 = days_since_zero - 719468;

    // Convert to seconds.
    ((days_since_1970 * 86400) + (hour * 3600) + (minute * 60) + second) as u32
}