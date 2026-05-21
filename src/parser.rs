use std::error::Error;
use std::fs::File;
use csv::ReaderBuilder;
use crate::models::{RawCandle, HistoricalDay};
use memmap2::Mmap;

pub fn load_and_process(path: &str, symbol: &str) -> Result<Vec<HistoricalDay<f32>>, Box<dyn Error>> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let mut reader = ReaderBuilder::new()
        .has_headers(false) 
        .delimiter(b';')    
         .from_reader(&mmap[..]);

    let mut all_days = Vec::with_capacity(500); // pre allocation to avoid continuous resizing
    let mut current_day_candles = Vec::with_capacity(1000);
    let mut current_date = String::new();

    let mut record = csv::ByteRecord::new();
    let mut r: RawCandle<f32>;

    while reader.read_byte_record(&mut record)? {
        r = record.deserialize(None)?;

        if !current_date.is_empty() && r.date != current_date {
            all_days.push(HistoricalDay {
                date: current_date.clone(),
                candles: std::mem::take(&mut current_day_candles), // tale all and clear for next day
            });
        }

        current_date = r.date.clone();
        current_day_candles.push(r);
    }

    if !current_day_candles.is_empty() {
        all_days.push(HistoricalDay {
            date: current_date,
            candles: current_day_candles,
        });
    }
    
    println!("{} - Caricate {} giornate storiche.", symbol, all_days.len());
    Ok(all_days)
}

/// Aggrega candele da `source_tf_min` a `target_tf_min`.
/// `target_tf_min` deve essere multiplo di `source_tf_min`.
pub fn resample(
    days: &[HistoricalDay<f32>],
    source_tf_min: u32,
    target_tf_min: u32,
) -> Vec<HistoricalDay<f32>> {
    assert_eq!(target_tf_min % source_tf_min, 0, "Il target deve essere multiplo del source");

    days.into_iter().map(|day| {
        let resampled = resample_candles(&day.candles, target_tf_min);
        HistoricalDay { date: day.date.clone(), candles: resampled }
    }).collect()
}

fn resample_candles(candles: &[RawCandle<f32>], target_tf_min: u32) -> Vec<RawCandle<f32>> {
    candles
        .chunk_by(|a, b| bar_group(a, target_tf_min) == bar_group(b, target_tf_min))  // raggruppa per slot di orario
        .filter_map(|group| {
            // Raggruppa solo gruppi "completi" allineati ai boundary
            // (puoi rimuovere il check se vuoi includere anche l'ultima candela parziale)
            let first = group.first()?;
            let last  = group.last()?;
            Some(RawCandle {
                date:  first.date.clone(),
                time:  first.time.clone(),
                open:  first.open,
                high:  group.iter().map(|c| c.high).fold(f32::NEG_INFINITY, f32::max),
                low:   group.iter().map(|c| c.low).fold(f32::INFINITY, f32::min),
                close: last.close,
                _volume: group.iter().map(|c| c._volume).sum(),
            })
        })
        .collect()
}

fn bar_group(c: &RawCandle<f32>, target_tf_min: u32) -> u32 {
    let parts: Vec<u32> = c.time.splitn(3, ':')
        .map(|s| s.parse().unwrap_or(0))
        .collect();
    let total_minutes = parts[0] * 60 + parts[1];
    total_minutes / target_tf_min
}