use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Timelike, Utc, Weekday};
use chrono_tz::America::New_York;
use chrono_tz::Europe::London;

use crate::models::RawCandle;
use crate::oanda::candle_start_utc_ny_row;

#[derive(Debug, thiserror::Error)]
pub enum FullSearchPrepError {
    #[error("timestamp non valido")]
    BadTimestamp,
    #[error("candele insufficienti da OANDA")]
    InsufficientData,
    #[error("barra di riferimento non trovata nel range")]
    RefBarNotFound,
    #[error("barra di apertura sessione non trovata")]
    OpenBarNotFound,
    #[error("contesto precedente non disponibile")]
    PrevSessionNotFound,
    #[error("parse ora candela: {0}")]
    CandleTime(String),
}

/// AUTO come in Quantower `RetroVis.cs` (orari locali Londra / NY, DST via IANA).
pub fn resolve_auto_session_id(utc: DateTime<Utc>) -> Option<&'static str> {
    let london = utc.with_timezone(&London);
    let ny = utc.with_timezone(&New_York);
    let lt = london.hour() * 100 + london.minute();
    let nt = ny.hour() * 100 + ny.minute();
    if lt >= 800 && lt < 1700 && nt < 930 {
        Some("LONDON_OPEN")
    } else if nt >= 930 && nt < 1600 {
        Some("NY_OPEN")
    } else {
        None
    }
}

fn prev_trading_day_naive(d: NaiveDate) -> NaiveDate {
    let mut p = d.pred_opt().unwrap_or(d);
    while matches!(p.weekday(), Weekday::Sat | Weekday::Sun) {
        p = p.pred_opt().unwrap_or(p);
    }
    p
}

fn ny_date_of_bar_start(utc: DateTime<Utc>) -> NaiveDate {
    utc.with_timezone(&New_York).date_naive()
}

fn utc_from_ny_hm(date: NaiveDate, h: u32, m: u32) -> Option<DateTime<Utc>> {
    let naive = date.and_time(NaiveTime::from_hms_opt(h, m, 0)?);
    match New_York.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => {
            Some(dt.with_timezone(&Utc))
        }
        chrono::LocalResult::None => None,
    }
}

pub fn ny_local_epoch_to_utc(ny_local_ts: i64) -> Option<DateTime<Utc>> {

    let naive = DateTime::from_timestamp(ny_local_ts, 0)?.naive_utc();
    match New_York.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        chrono::LocalResult::Ambiguous(dt, _) => Some(dt.with_timezone(&Utc)),
        chrono::LocalResult::None => None,
    }
}

/// Indice della candela che contiene `reference_ts` (UTC), oppure l'ultima con inizio <= ts.
pub fn find_reference_index(
    candles: &[RawCandle<f32>],
    reference_ts: i64,
    tf_sec: i64,
) -> Result<usize, FullSearchPrepError> {
    let ts = DateTime::from_timestamp(reference_ts, 0).ok_or(FullSearchPrepError::BadTimestamp)?;
    let mut fallback: Option<usize> = None;
    for (i, c) in candles.iter().enumerate() {
        let start = candle_start_utc_ny_row(c).map_err(|e| FullSearchPrepError::CandleTime(e.to_string()))?;
        let end = start + Duration::seconds(tf_sec);
        if start <= ts && ts < end {
            return Ok(i);
        }
        if start <= ts {
            fallback = Some(i);
        }
    }
    fallback.ok_or(FullSearchPrepError::RefBarNotFound)
}

/// Cerca l'apertura sessione andando indietro al massimo `max_lookback` candele da `ref_idx`.
pub fn find_session_open_index_backward(
    candles: &[RawCandle<f32>],
    ref_idx: usize,
    session_hint: &str,
    max_lookback: usize,
) -> Result<usize, FullSearchPrepError> {
    let lo = ref_idx.saturating_sub(max_lookback);
    for i in (lo..=ref_idx).rev() {
        let start = candle_start_utc_ny_row(&candles[i]).map_err(|e| FullSearchPrepError::CandleTime(e.to_string()))?;
        if bar_matches_session_open(start, session_hint) {
            return Ok(i);
        }
    }
    Err(FullSearchPrepError::OpenBarNotFound)
}

fn bar_matches_session_open(bar_start_utc: DateTime<Utc>, session_hint: &str) -> bool {
    match session_hint {
        "NY_OPEN" => {
            let ny = bar_start_utc.with_timezone(&New_York);
            ny.hour() == 9 && ny.minute() == 30
        }
        "LONDON_OPEN" => {
            let ld = bar_start_utc.with_timezone(&London);
            ld.hour() == 8 && ld.minute() == 0
        }
        _ => false,
    }
}

/// Sessione effettiva dalla candela di open (come `RetroVis.cs` dopo il match).
pub fn effective_session_from_open_bar(open_bar_start_utc: DateTime<Utc>) -> &'static str {
    let ny = open_bar_start_utc.with_timezone(&New_York);
    if ny.hour() == 9 && ny.minute() == 30 {
        "NY_OPEN"
    } else {
        "LONDON_OPEN"
    }
}

pub struct ContextInputs {
    pub session_open: f32,
    pub prev_close: f32,
    pub prev_high: f32,
    pub prev_low: f32,
    pub intraday_high: f32,
    pub intraday_low: f32,
    /// Chiave cache contesto: data calendario NY del giorno di apertura sessione.
    pub context_cache_date: String,
}

/// Replica `FetchContextMatchesAsync` usando serie temporale ordinata (vecchia → nuova).
pub fn compute_context_inputs(
    candles: &[RawCandle<f32>],
    open_idx: usize,
    effective_session: &str,
) -> Result<ContextInputs, FullSearchPrepError> {
    let session_open = candles[open_idx].open;
    let open_start = candle_start_utc_ny_row(&candles[open_idx])
        .map_err(|e| FullSearchPrepError::CandleTime(e.to_string()))?;
    let open_ny_date = ny_date_of_bar_start(open_start);
    let context_cache_date = open_ny_date.format("%Y-%m-%d").to_string();

    // Intraday pre-open: stessa data NY dell'open, al massimo 72 barre indietro (indici decrescenti).
    let mut intraday_high = session_open;
    let mut intraday_low = session_open;
    let mut steps = 0usize;
    let mut j = open_idx;
    while steps < 72 && j > 0 {
        j -= 1;
        steps += 1;
        let t = candle_start_utc_ny_row(&candles[j]).map_err(|e| FullSearchPrepError::CandleTime(e.to_string()))?;
        if ny_date_of_bar_start(t) != open_ny_date {
            break;
        }
        intraday_high = intraday_high.max(candles[j].high);
        intraday_low = intraday_low.min(candles[j].low);
    }

    // Sessione precedente: ancore 17:00 NY come in `FetchContextMatchesAsync`.
    let prev_end_date = prev_trading_day_naive(open_ny_date);
    let prev_session_end = utc_from_ny_hm(prev_end_date, 17, 0).ok_or(FullSearchPrepError::PrevSessionNotFound)?;
    let prev_start_date = prev_trading_day_naive(prev_end_date);
    let prev_session_start = utc_from_ny_hm(prev_start_date, 17, 0).ok_or(FullSearchPrepError::PrevSessionNotFound)?;

    let _ = effective_session;

    let mut prev_close = 0.0f32;
    let mut prev_high = f32::NEG_INFINITY;
    let mut prev_low = f32::INFINITY;
    let mut found_prev = false;

    let mut k = open_idx;
    let mut guard = 0usize;
    while k > 0 && guard < 1200 {
        k -= 1;
        guard += 1;
        let bar_start = candle_start_utc_ny_row(&candles[k]).map_err(|e| FullSearchPrepError::CandleTime(e.to_string()))?;

        if bar_start >= prev_session_end {
            continue;
        }
        if bar_start < prev_session_start {
            break;
        }
        if !found_prev {
            prev_close = candles[k].close;
            found_prev = true;
        }
        prev_high = prev_high.max(candles[k].high);
        prev_low = prev_low.min(candles[k].low);
    }

    if !found_prev {
        return Err(FullSearchPrepError::PrevSessionNotFound);
    }

    Ok(ContextInputs {
        session_open,
        prev_close,
        prev_high,
        prev_low,
        intraday_high,
        intraday_low,
        context_cache_date,
    })
}

/// Candele pattern sessione: da open fino a ref **esclusa** (come lista inviata da Quantower).
pub fn pattern_candles_exclusive_ref(
    candles: &[RawCandle<f32>],
    open_idx: usize,
    ref_idx: usize,
) -> Result<Vec<RawCandle<f32>>, FullSearchPrepError> {
    if ref_idx <= open_idx {
        return Err(FullSearchPrepError::InsufficientData);
    }
    Ok(candles[open_idx..ref_idx].to_vec())
}
