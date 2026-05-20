use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use chrono_tz::America::New_York;
use dashmap::DashMap;
use reqwest::StatusCode;
use serde::Deserialize;

use crate::cache::RetroVisCache;
use crate::models::RawCandle;

#[derive(Debug, Clone, thiserror::Error)]
pub enum OandaError {
    #[error("symbolo non supportato per OANDA: {0}")]
    UnsupportedSymbol(String),
    #[error("richiesta OANDA fallita: {0}")]
    Http(String),
    #[error("risposta OANDA non valida: {0}")]
    Parse(String),
}

/// Default OANDA v20 practice host.
const DEFAULT_REST_URL: &str = "https://api-fxpractice.oanda.com";

#[derive(Clone)]
pub struct OandaClient {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
    response_cache: Arc<DashMap<String, Vec<RawCandle<f32>>>>,
}

impl OandaClient {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| DEFAULT_REST_URL.to_string()),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("reqwest client"),
            response_cache: Arc::new(DashMap::new()),
        }
    }

    pub fn instrument_for_retro_symbol(symbol: &str) -> Result<&'static str, OandaError> {
        match symbol {
            "NQ" => Ok("NAS100_USD"),
            "GC" => Ok("XAU_USD"),
            "YM" => Ok("US30_USD"),
            _ => Err(OandaError::UnsupportedSymbol(symbol.to_string())),
        }
    }

    fn oanda_granularity(tf: &str) -> &'static str {
        match tf {
            "M1" => "M1",
            "M5" => "M5",
            "M15" => "M15",
            "M30" => "M30",
            "H1" => "H1",
            _ => "M5",
        }
    }

    /// Fetch up to `count` candles ending strictly before `to_exclusive` (OANDA `to` semantics).
    pub async fn fetch_candles_before(
        &self,
        retro_symbol: &str,
        timeframe: &str,
        to_exclusive: DateTime<Utc>,
        count: u32,
    ) -> Result<Vec<RawCandle<f32>>, OandaError> {
        let instrument = Self::instrument_for_retro_symbol(retro_symbol)?;
        let gran = Self::oanda_granularity(timeframe);
        let to_str = to_exclusive.to_rfc3339_opts(SecondsFormat::Secs, true);
        let cache_key = format!("{}|{}|{}|{}", instrument, gran, to_str, count);
        if let Some(hit) = self.response_cache.get(&cache_key) {
            return Ok(hit.clone());
        }

        let base = format!(
            "{}/v3/instruments/{}/candles",
            self.base_url.trim_end_matches('/'),
            instrument
        );
        let mut url = reqwest::Url::parse(&base).map_err(|e| OandaError::Http(e.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("price", "M");
            pairs.append_pair("granularity", gran);
            pairs.append_pair("count", &count.to_string());
            pairs.append_pair("to", &to_str);
        }

        let mut last_err = None;
        for delay_ms in [0u64, 500, 1000, 3000] {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            let response = self
                .http
                .get(url.clone())
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await;

            let response = match response {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(OandaError::Http(e.to_string()));
                    continue;
                }
            };

            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            if status == StatusCode::TOO_MANY_REQUESTS
                || status.is_server_error()
                || status == StatusCode::REQUEST_TIMEOUT
            {
                last_err = Some(OandaError::Http(format!("HTTP {}: {}", status, body)));
                continue;
            }

            if !status.is_success() {
                return Err(OandaError::Http(format!("HTTP {}: {}", status, body)));
            }

            let parsed: OandaCandlesBody = serde_json::from_str(&body)
                .map_err(|e| OandaError::Parse(e.to_string()))?;

            let mut out = Vec::with_capacity(parsed.candles.len());
            for c in parsed.candles {
                if !c.complete {
                    continue;
                }
                let utc: DateTime<Utc> = DateTime::parse_from_rfc3339(&c.time)
                    .map_err(|e| OandaError::Parse(e.to_string()))?
                    .with_timezone(&Utc);
                let ny = utc.with_timezone(&New_York);
                let date = ny.format("%Y-%m-%d").to_string();
                let time = ny.format("%H:%M:%S").to_string();
                let o = parse_px(&c.mid.o)?;
                let h = parse_px(&c.mid.h)?;
                let l = parse_px(&c.mid.l)?;
                let cl = parse_px(&c.mid.c)?;
                out.push(RawCandle {
                    date,
                    time,
                    open: o,
                    high: h,
                    low: l,
                    close: cl,
                    _volume: c.volume.max(0) as f32,
                });
            }

            out.sort_by(|a, b| {
                let ta = format!("{} {}", a.date, a.time);
                let tb = format!("{} {}", b.date, b.time);
                ta.cmp(&tb)
            });

            self.response_cache.insert(cache_key, out.clone());
            return Ok(out);
        }

        Err(last_err.unwrap_or_else(|| OandaError::Http("retry esauriti".into())))
    }

    /// Pull enough history for session open + context (multiple pages if needed).
    pub async fn fetch_candles_for_full_search(
        &self,
        retro_symbol: &str,
        timeframe: &str,
        reference_ts: i64,
        min_bars: usize,
    ) -> Result<Vec<RawCandle<f32>>, OandaError> {
        let tf_sec = RetroVisCache::timeframe_to_seconds(timeframe).max(1) as i64;
        let mut to_exclusive = DateTime::from_timestamp(reference_ts, 0)
            .unwrap_or_else(Utc::now)
            + chrono::Duration::seconds(tf_sec);

        let mut merged: Vec<RawCandle<f32>> = Vec::new();
        let page: u32 = 500;
        let mut safety = 0;

        while merged.len() < min_bars && safety < 8 {
            safety += 1;
            let batch = self
                .fetch_candles_before(retro_symbol, timeframe, to_exclusive, page)
                .await?;

            if batch.is_empty() {
                break;
            }

            let earliest = candle_start_utc_ny_row(&batch[0])?;
            to_exclusive = earliest;

            for c in batch {
                if !merged.iter().any(|x| x.date == c.date && x.time == c.time) {
                    merged.push(c);
                }
            }

            merged.sort_by(|a, b| {
                let ta = format!("{} {}", a.date, a.time);
                let tb = format!("{} {}", b.date, b.time);
                ta.cmp(&tb)
            });
        }

        Ok(merged)
    }
}

pub fn candle_start_utc_ny_row(c: &RawCandle<f32>) -> Result<DateTime<Utc>, OandaError> {
    let d = chrono::NaiveDate::parse_from_str(&c.date, "%Y-%m-%d")
        .map_err(|e| OandaError::Parse(e.to_string()))?;
    let t = chrono::NaiveTime::parse_from_str(&c.time, "%H:%M:%S")
        .map_err(|e| OandaError::Parse(e.to_string()))?;
    let naive_dt = d.and_time(t);
    let loc = match New_York.from_local_datetime(&naive_dt) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => {
            return Err(OandaError::Parse("invalida ora locale NY".into()));
        }
    };
    Ok(loc.with_timezone(&Utc))
}

fn parse_px(s: &str) -> Result<f32, OandaError> {
    s.parse::<f32>()
        .map_err(|e| OandaError::Parse(format!("prezzo: {e}")))
}

#[derive(Deserialize)]
struct OandaCandlesBody {
    candles: Vec<OandaCandle>,
}

#[derive(Deserialize)]
struct OandaCandle {
    complete: bool,
    time: String,
    volume: i64,
    mid: OandaMid,
}

#[derive(Deserialize)]
struct OandaMid {
    o: String,
    h: String,
    l: String,
    c: String,
}
