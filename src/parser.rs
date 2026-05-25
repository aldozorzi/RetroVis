use std::error::Error;
use std::fs::File;
use crate::models::{RawCandle, HistoricalDay};
use memmap2::Mmap;

pub fn load_and_process(path: &str, symbol: &str) -> Result<Vec<HistoricalDay<f32>>, Box<dyn Error>> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    let mut all_days = Vec::with_capacity(5000); 
    let mut current_day_candles = Vec::with_capacity(300);
    let mut current_date_num = 0u32;

    let mut start = 0;
    let buffer = &mmap[..];

    for i in 0..buffer.len() {
        if buffer[i] == b'\n' {
            let line = &buffer[start..i];
            start = i + 1;

            if line.is_empty() { continue; }

            // Split manuale sui byte del punto e virgola
            let mut fields = line.split(|&b| b == b';');

            let date_bytes = match fields.next() { Some(f) => f, None => continue };
            let time_bytes = match fields.next() { Some(f) => f, None => continue };
            let open_bytes = match fields.next() { Some(f) => f, None => continue };
            let high_bytes = match fields.next() { Some(f) => f, None => continue };
            let low_bytes = match fields.next() { Some(f) => f, None => continue };
            let close_bytes = match fields.next() { Some(f) => f, None => continue };
            let volume_bytes = match fields.next() { Some(f) => f, None => continue };

            let date_num = match parse_date_to_u32(date_bytes) { Some(d) => d, None => continue };
            let open = match parse_f32(open_bytes) { Some(v) => v, None => continue };
            let high = match parse_f32(high_bytes) { Some(v) => v, None => continue };
            let low = match parse_f32(low_bytes) { Some(v) => v, None => continue };
            let close = match parse_f32(close_bytes) { Some(v) => v, None => continue };
            let volume = match parse_f32(volume_bytes) { Some(v) => v, None => continue };

            let timestamp = calculate_timestamp(date_bytes, time_bytes);

            let r = RawCandle {
                timestamp,
                open,
                high,
                low,
                close,
                _volume: volume,
            };

            if current_date_num != 0 && date_num != current_date_num {
                all_days.push(HistoricalDay {
                    date: current_date_num,
                    candles: std::mem::take(&mut current_day_candles),
                });
            }

            current_date_num = date_num;
            current_day_candles.push(r);
        }
    }

    if !current_day_candles.is_empty() {
        all_days.push(HistoricalDay {
            date: current_date_num,
            candles: current_day_candles,
        });
    }

    println!("{} - Caricate {} giornate storiche.", symbol, all_days.len());
    Ok(all_days)
}

/// Aggrega candele da `source_tf_min` a `target_tf_min`.
/// `target_tf_min` deve essere multiplo di `source_tf_min`.
pub fn resample(source_days: &[HistoricalDay<f32>], _from_min: u32, to_min: u32) -> Vec<HistoricalDay<f32>> {
    let mut resampled_days = Vec::with_capacity(source_days.len());
    let interval_secs = (to_min * 60) as i64;

    for day in source_days {
        if day.candles.is_empty() { continue; }

        let mut resampled_candles = Vec::with_capacity(day.candles.len() / 3);
        let mut current_bucket: Option<RawCandle<f32>> = None;

        for candle in &day.candles {
            // Arrotonda il timestamp all'inizio dell'intervallo da 15 minuti
            let bucket_timestamp = (candle.timestamp / interval_secs) * interval_secs;

            match &mut current_bucket {
                Some(b) if b.timestamp == bucket_timestamp => {
                    // Aggiorna la candela da 15 minuti corrente
                    if candle.high > b.high { b.high = candle.high; }
                    if candle.low < b.low { b.low = candle.low; }
                    b.close = candle.close;
                    b._volume += candle._volume;
                }
                _ => {
                    // Se c'è un bucket precedente completo, lo salviamo
                    if let Some(b) = current_bucket.take() {
                        resampled_candles.push(b);
                    }
                    // Inizializza un nuovo bucket da 15 minuti
                    current_bucket = Some(RawCandle {
                        timestamp: bucket_timestamp,
                        open: candle.open,
                        high: candle.high,
                        low: candle.low,
                        close: candle.close,
                        _volume: candle._volume,
                    });
                }
            }
        }

        // Push dell'ultimo bucket del giorno, se presente
        if let Some(b) = current_bucket {
            resampled_candles.push(b);
        }

        resampled_days.push(HistoricalDay {
            date: day.date,
            candles: resampled_candles,
        });
    }

    resampled_days
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

fn calculate_timestamp(date_bytes: &[u8], time_bytes: &[u8]) -> i64 {
    if date_bytes.len() < 10 || time_bytes.len() < 8 { return 0; }

    // Estrazione numerica della data (YYYY-MM-DD) convertendo in i64
    let year = ((date_bytes[0] - b'0') as i64 * 1000) + ((date_bytes[1] - b'0') as i64 * 100) + ((date_bytes[2] - b'0') as i64 * 10) + (date_bytes[3] - b'0') as i64;
    let month = ((date_bytes[5] - b'0') as i64 * 10) + (date_bytes[6] - b'0') as i64;
    let day = ((date_bytes[8] - b'0') as i64 * 10) + (date_bytes[9] - b'0') as i64;

    // Estrazione numerica dell'orario (HH:MM:SS) convertendo in i64
    let hour = ((time_bytes[0] - b'0') as i64 * 10) + (time_bytes[1] - b'0') as i64;
    let minute = ((time_bytes[3] - b'0') as i64 * 10) + (time_bytes[4] - b'0') as i64;
    let second = ((time_bytes[6] - b'0') as i64 * 10) + (time_bytes[7] - b'0') as i64;

    // Formula per i giorni cumulativi
    let mut y = year;
    if month <= 2 { y -= 1; }
    let era = y / 400;
    let y_in_era = y % 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = y_in_era * 365 + y_in_era / 4 - y_in_era / 100 + doy;
    let days_since_zero = era * 146097 + doe as i64;
    
    // Sottraiamo i giorni tra l'anno 0 e il 1970 (719468 giorni)
    let days_since_1970 = days_since_zero - 719468;

    // Conversione in secondi
    (days_since_1970 * 86400) + (hour * 3600) + (minute * 60) + second
}