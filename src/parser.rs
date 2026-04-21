use std::error::Error;
use std::fs::File;
use csv::ReaderBuilder;
use crate::models::{RawCandle, HistoricalDay};

/// Carica il file CSV e raggruppa le candele per giornata.
/// Non esegue filtri orari nè normalizzazioni: queste operazioni
/// sono ora delegate all'Engine in modo dinamico.
pub fn load_and_process(path: &str) -> Result<Vec<HistoricalDay<f32>>, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut reader = ReaderBuilder::new()
        .has_headers(false) // I tuoi file non hanno header
        .delimiter(b';')    // Usiamo il punto e virgola come nel tuo CSV
        .from_reader(file);

    let mut all_days = Vec::new();
    let mut current_day_candles = Vec::new();
    let mut current_date = String::new();

    for result in reader.deserialize() {
        let r: RawCandle<f32> = result?;

        // Se la data cambia, salviamo la giornata accumulata e iniziamo la nuova
        if !current_date.is_empty() && r.date != current_date {
            all_days.push(HistoricalDay {
                date: current_date.clone(),
                candles: current_day_candles.clone(),
            });
            current_day_candles.clear();
        }

        current_date = r.date.clone();
        current_day_candles.push(r);
    }

    // Aggiungiamo l'ultima giornata processata
    if !current_day_candles.is_empty() {
        all_days.push(HistoricalDay {
            date: current_date,
            candles: current_day_candles,
        });
    }

    println!("Caricate {} giornate storiche.", all_days.len());
    Ok(all_days)
}