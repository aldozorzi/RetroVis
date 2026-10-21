# RetroVis

RetroVis is a local research stack for session-based pattern matching on futures
data. It combines a small Rust HTTP server with a Quantower indicator:

- the **Rust server** loads historical 5-minute candles into memory and performs
  vector similarity searches against the current session;
- the **Quantower indicator** reads bars from the chart, sends them to the local
  server, and renders the matching statistics, levels, alerts, and optional
  order helpers directly on the chart.

Everything runs locally. There is no cloud backend and no telemetry.

## Architecture

```text
Quantower chart
      |
      |  bars, session, OHLC context
      v
RetroVis indicator (C# / .NET)
      |
      |  HTTP + JSON
      v
RetroVis server (Rust / Axum)  ->  in-memory candle store + query cache
      |
      +--> POST /search
      +--> POST /context-search
      +--> GET  /day-details
      +--> GET  /visualize
```

The server binds to `127.0.0.1:9999` by default.

## Data and memory footprint

At startup the server parses three files:

```text
data/nq-M5.csv
data/gc-M5.csv
data/ym-M5.csv
```

Together they contain **4,048,311 M5 rows**. Each row is a semicolon-separated
record with six fields used by the engine:

```text
YYYY-MM-DD;HH:MM:SS;open;high;low;close;volume
```

The trailing volume column is ignored. The CSVs are read through a memory-mapped
file and stored as compact `RawCandle<f32>` values (20 bytes each), so the
complete candle store occupies **less than 80 MB of memory (about 77 MiB)**
despite the roughly 200 MB of CSV input.

Each instrument also keeps a day index mapping `YYYYMMDD` to a range in the
candle vector. Coarser timeframes (M10, M15, M30, H1) are produced by resampling
the native M5 data on demand.

## Sessions

The engine aligns every historical day to the session open using the timezone of
that specific historical date, so US/UK daylight-saving transitions are handled
correctly.

| Session | Local time | Timezone |
| --- | --- | --- |
| `LONDON_OPEN` | 08:00 | `Europe/London` |
| `NY_OPEN` | 09:30 | `America/New_York` |
| `CPI` | 08:30, fixed 1-hour window | `America/New_York` |
| `PPI` | 08:30, fixed 1-hour window | `America/New_York` |
| `NFP` | 08:30, fixed 1-hour window | `America/New_York` |
| `FOMC` | 14:00, fixed 1-hour window | `America/New_York` |

News sessions only use dates listed in `data/CPI.txt`, `data/PPI.txt`,
`data/NFP.txt`, and `data/FOMC.txt` (one `YYYY-MM-DD` per line).

## Pattern search

For a pattern query, the server:

1. selects the historical session for each trading day;
2. normalizes each candle relative to the session open into body, upper-wick,
   lower-wick, and range percentages;
3. computes the Euclidean distance between the current pattern and every
   historical window of the same length;
4. keeps the `top_k` closest matches;
5. aggregates MFE/MAE, end-of-session, end-of-hour, and next-candle statistics.

The context search uses a different feature vector: opening gap, position inside
the previous day's range, and distances from the previous day's high and the
intraday high/low.

Results are cached per symbol, timeframe, and timestamp. The cache is evicted at
the start of each UTC day.

## HTTP API

### `POST /search`

Request (`QueryRequest`):

```json
{
  "symbol": "NQ",
  "session_id": "NY_OPEN",
  "timeframe": "M5",
  "timestamp": 1784862000,
  "session_open": 28544.25,
  "top_k": 5,
  "candles": [
    { "timestamp": 1784861700, "open": 28540.0, "high": 28560.0, "low": 28535.0, "close": 28555.0 }
  ]
}
```

### `POST /context-search`

Request (`ContextQueryRequest`):

```json
{
  "symbol": "NQ",
  "session_id": "NY_OPEN",
  "timestamp": 1784862000,
  "session_open": 28544.25,
  "prev_close": 28520.0,
  "prev_high": 28600.0,
  "prev_low": 28450.0,
  "intraday_high": 28590.0,
  "intraday_low": 28510.0,
  "top_k": 5
}
```

Both search endpoints return the same shape:

```json
{
  "summary": {
    "avg_up": 0.0,
    "avg_down": 0.0,
    "avg_eos": 0.0,
    "avg_eoh": 0.0,
    "edge_ratio": 0.0,
    "bias_pct": 0,
    "confidence": 0
  },
  "matches": [
    {
      "timestamp": 0,
      "similarity": 0.0,
      "eos_pts": 0.0,
      "up_pts": 0.0,
      "down_pts": 0.0,
      "eoh_pts": 0.0,
      "next_candle_up": 0.0,
      "next_candle_down": 0.0
    }
  ],
  "next_candle": {
    "avg_up": 0.0,
    "avg_down": 0.0,
    "edge_ratio": 0.0,
    "bias_pct": 0,
    "confidence": 0
  }
}
```

### `GET /day-details`

Returns the candles for a date (and optionally the previous day) so they can be
plotted:

```text
/day-details?symbol=NQ&date=2026-07-17&timeframe=M5
```

### `GET /visualize`

Serves a lightweight-charts debug page for a date:

```text
/visualize?symbol=NQ&date=2026-07-17&timeframe=M5
```

The page loads the chart library from the unpkg CDN.

## Quantower indicator

The C# indicator lives in `Quantower/RetroVis/` and targets .NET 10 on Windows.

Main parts:

| Path | Purpose |
| --- | --- |
| `RetroVis.cs` | indicator entry point, parameters, update loop, painting |
| `Services/RetroVisApiClient.cs` | HTTP client for `/search` and `/context-search` |
| `Services/SessionBarReader.cs` | session-aware reading of chart bars and resampling |
| `Rendering/PanelRenderer.cs` | results panel, levels, buttons |
| `Rendering/MultiTimeframePanelRenderer.cs` | higher-timeframe panel |
| `Services/CloseAlertService.cs` | alert before candle close |
| `Services/PositionSizer.cs` | position size and SL/TP calculation |
| `Services/OrderService.cs` | market order helper |
| `Services/AutoOrderService.cs` | optional automatic order helper |

The indicator can:

- resolve `AUTO` sessions from London/New York local time;
- request candle-pattern and context matches for the active session;
- draw session levels, next-candle levels, and match markers on the chart;
- show a results panel and a multi-timeframe panel;
- play a close alert and optionally place orders through Quantower.

Auto orders are disabled by default (`EnableAutoOrders = false`). The order
helpers are experimental and should only be used on a demo account.

### Build

Adjust the `OutputPath` and the `TradingPlatform.BusinessLayer` `HintPath` in
`Quantower/RetroVis/RetroVis.csproj` to match your Quantower installation, then
open `RetroVis.slnx` in Visual Studio or run:

```text
dotnet build Quantower/RetroVis/RetroVis.csproj
```

Copy the compiled indicator into your Quantower `Settings/Scripts/Indicators`
folder, add it to a chart, and keep `Webserver URL` pointed at
`http://127.0.0.1:9999`.

## Server build and run

Requirements: stable Rust with edition 2024 support.

```text
cargo run --release
```

On Windows, `dist.bat` builds the release binary and copies `retro-vis.exe` to
the repository root.

Run the test suite with:

```text
cargo test
```

## Repository layout

```text
src/                    Rust server
  main.rs               startup, data loading, routes
  parser.rs             mmap CSV parser and resampling
  engine.rs             session alignment and similarity search
  service.rs            query orchestration and aggregation
  stats.rs              weighted medians and projection statistics
  cache.rs              per-day result cache
  handlers.rs           HTTP handlers
  visualize.rs          lightweight-charts debug page
  models/               request, response, and domain types
data/                   historical M5 CSVs and news event dates
Quantower/RetroVis/     C# Quantower indicator
```

## Configuration and secrets

Local operation needs no API keys, and the current code does not read any
environment variables. No `.env` file is required. Any local `.env` is ignored
by Git and must never be committed.

## Disclaimer

RetroVis is research and educational software, not financial advice. Historical
pattern statistics do not guarantee future results. Test thoroughly, preferably
on a demo account, before enabling any order-related feature.
