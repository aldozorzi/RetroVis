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