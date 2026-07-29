using System;
using System.Collections.Generic;
using System.Drawing;
using System.Runtime.Versioning;
using System.Threading.Tasks;
using TradingPlatform.BusinessLayer;
using TradingPlatform.BusinessLayer.Chart;
using Ldazrz.RetroVis.Models;
using Ldazrz.RetroVis.Services;
using Ldazrz.RetroVis.Rendering;

namespace Ldazrz.RetroVis;

[SupportedOSPlatform("windows")]
public class RetroVis : Indicator
{
    #region Parameters
    [InputParameter("Account", 10)]
    public Account SelectedAccount;

    [InputParameter("Session Type", 20, variants: ["AUTO", "AUTO", "NY_OPEN", "NY_OPEN", "LONDON_OPEN", "LONDON_OPEN", "CPI", "CPI", "PPI", "PPI", "NFP", "NFP", "FOMC", "FOMC"])]
    public string SessionId = "AUTO";

    [InputParameter("Min Candles for Pattern", 30, 1, 15, 1)]
    public int MinCandles = 1;

    [InputParameter("Max Candles for Pattern", 40, 1, 20, 1)]
    public int MaxCandles = 6;

    [InputParameter("Webserver URL", 50)]
    public string ServerUrl = "http://127.0.0.1:9999";

    [InputParameter("Draw Session Levels", 60)]
    public bool DrawSessionLevels { get; set; } = true;

    [InputParameter("Draw Next Candle Levels", 70)]
    public bool DrawNextCandleLevels { get; set; } = true;

    [InputParameter("Enable Candle Close Alert", 80)]
    public bool EnableCloseAlert { get; set; } = true;

    [InputParameter("Alert Seconds Before Close", 90, 1, 60, 1)]
    public int AlertSecondsBeforeClose = 10;

    [InputParameter("Alert Sound File (.wav)", 100)]
    public string AlertSoundPath = @"C:\Risorse\suoni\ding.wav";

    [InputParameter("Risk Amount ($)", 110)]
    public double RiskAmount = 100;

    [InputParameter("Enable Auto Orders", 115)]
    public bool EnableAutoOrders { get; set; } = false;

    [InputParameter("Show Context Matches", 130)]
    public bool ShowContextMatches { get; set; } = false;


    //[InputParameter("Order TP Source", 12, variants: ["Next Candle Levels", "NextCandle", "Session Levels", "Session"])]
    public string OrderTpSource = "NextCandle";
    #endregion

    #region Internal Fields
    private RetroVisApiClient _apiClient;
    private SessionBarReader _barReader;
    private readonly PanelRenderer _panelRenderer = new();
    private readonly MultiTimeframePanelRenderer _multiTfRenderer = new();

    private AggregateData _candlesSummary = new();
    private NextCandleData _nextCandle = new();
    private List<MatchResponse> _candlesResults = [];
    private bool _isPanelVisible = true;
    private readonly Rectangle _toggleRect = new(5, 50, 120, 25);
    private int _openBarIndex = -1;
    private double _currentPriceBase = 0;
    private DateTime _lastProcessedBarTime;
    private int _lastProcessedBarIndex = -1;  // Track the last processed bar index to detect genuine new candles

    private int _currentCandleCount = 0;
    private string _ticker = "";
    private string _effectiveSession;

    private CloseAlertService _alertService;
    private ChartInputHandler _inputHandler;
    private readonly OrderService _orderService = new();
    private AutoOrderService _autoOrderService;
    private bool _autoOrderPlaced = false;

    private static TimeZoneInfo _nyTz;
    private static TimeZoneInfo _londonTz;

    private double _currentDelta = 0;

    private List<MatchClickArea> _matchAreas = [];
    private AggregateData _contextSummary = new();
    private List<MatchResponse> _contextResults = [];
    private string _lastContextSessionDate = "";
    private List<MatchClickArea> _contextAreas = [];
    private int _lastPanelHeight = 0;

    private bool _isBacktestMode = false;
    private readonly Rectangle _backtestBtnRect = new(130, 50, 130, 25);

    private string _currentAnalysisText;

    private int _analysisIndex;

    // Multi-timeframe panel fields
    private static readonly Dictionary<string, string[]> HigherTimeframes = new()
    {
        ["M5"]  = ["M10", "M15"],
        ["M10"] = ["M15", "M30"],
        ["M15"] = ["M30"]
    };

    private string _currentTimeframe;
    private string[] _higherTfs = [];
    private string _selectedTimeframe;
    private readonly Dictionary<string, HigherTfState> _higherTfStates = [];
    private List<TfClickArea> _multiTfClickAreas = [];
    #endregion

    public RetroVis() : base()
    {
        this.Name = "RetroVis";
        this.SeparateWindow = false;
    }

    private string CurrentTimeframe =>
        this._currentTimeframe ??= this.GetFormattedTimeframe();

    /// <summary>
    /// Carries the search results and tracking state for one higher timeframe.
    /// </summary>
        private sealed record HigherTfState(
        AggregateData Summary,
        NextCandleData NextCandle,
        float LastClose,
        long LastProcessedCandleIndex,
        bool IsLoading
    );

    protected override void OnInit()
    {
        this._apiClient = new RetroVisApiClient(this.ServerUrl, TimeSpan.FromSeconds(60));

        try
        {
            _nyTz = TimeZoneInfo.FindSystemTimeZoneById("Eastern Standard Time");
            _londonTz = TimeZoneInfo.FindSystemTimeZoneById("GMT Standard Time");
        }
        catch (Exception ex)
        {
            RetroVisLog.Error("CRITICAL: Timezone non trovata! " + ex.Message);
        }

        this._barReader = new SessionBarReader(this.HistoricalData, RetroVis._nyTz, RetroVis._londonTz);
        this._alertService = new CloseAlertService(this.HistoricalData);
        this._inputHandler = new ChartInputHandler(this.HistoricalData);
        this._autoOrderService = new AutoOrderService(this._orderService, this.AlertSoundPath);

        this._lastProcessedBarTime = DateTime.MinValue;
        this._lastProcessedBarIndex = -1;  // Initialize bar index tracking
        this._alertService.LoadSound(this.AlertSoundPath);

        if (this.CurrentChart != null)
        {
            this.CurrentChart.MouseUp -= this.OnChartMouseUp;
            this.CurrentChart.MouseUp += this.OnChartMouseUp;
            this.CurrentChart.MouseDown -= this.OnChartMouseDown;
            this.CurrentChart.MouseDown += this.OnChartMouseDown;
        }

        this.InitTicker();

        this._currentTimeframe = this.GetFormattedTimeframe();
        this._selectedTimeframe = this._currentTimeframe;
        this._higherTfs = HigherTimeframes.TryGetValue(this._currentTimeframe, out var higher) ? higher : [];

    }

    protected void ResetLists()
    {
        this._matchAreas.Clear();
        this._contextAreas.Clear();
    }

    protected void Reset()
    {
        // Keep _lastProcessedBarIndex to track genuine new candles
        this._lastProcessedBarTime = default;
        this._candlesResults.Clear();
        this._contextResults.Clear();
        this.ResetLists();
        this._currentCandleCount = 0;
        this._lastContextSessionDate = "";
        this._currentAnalysisText = "Loading new Analysis...";

        // Preserve higher-TF data across new candles — the data remains
        // valid until the next higher-TF bucket boundary triggers a fresh
        // fetch. Only clear click areas so stale rectangles don't persist.
        this._multiTfClickAreas.Clear();
    }

    protected override void OnSettingsUpdated()
    {
        try
        {
            base.OnSettingsUpdated();
            this._alertService?.LoadSound(this.AlertSoundPath);
            CloseAlertService.Reset();
            this.Reset();
            this._lastProcessedBarIndex = -1;  // Reset bar index tracking when settings change
        }
        catch (Exception ex)
        {
            RetroVisLog.Error($"OnSettingsUpdated error: {ex.Message}");
            this._lastProcessedBarIndex = -1;  // Ensure bar index is reset even if there's an error
        }
    }

    private HistoryAggregationTime Aggregation => (HistoryAggregationTime)this.HistoricalData.Aggregation;

    protected override void OnUpdate(UpdateArgs args)
    {
        try
        {
            this._alertService.CheckAndPlay(this.EnableCloseAlert, this._isBacktestMode, this.AlertSecondsBeforeClose);

            if (this._isBacktestMode || !this._isPanelVisible) return;
            this._currentDelta = this.Close(0) - this._currentPriceBase;

            DateTime currentBarTime = this.Time();
            int currentBarIndex = this.Count - 1;  // Current bar index

            // Check if this is a genuine new candle (new index) or just a chart reload/change
            // RetroVisLog.Info($"OnUpdate: CurrentBarIndex={currentBarIndex}, LastProcessedBarIndex={this._lastProcessedBarIndex}, CurrentBarTime={currentBarTime}, LastProcessedBarTime={this._lastProcessedBarTime}");
            bool isNewCandle = currentBarIndex != this._lastProcessedBarIndex;
            
            // Always update time tracking
            if (currentBarTime != this._lastProcessedBarTime)
            {
                this._lastProcessedBarTime = currentBarTime;
            }
            
            // Only reset and process if this is a new candle
            if (isNewCandle)
            {
                this.Reset();
                this._lastProcessedBarIndex = currentBarIndex;
                
                if (currentBarTime.ToUniversalTime() < DateTime.UtcNow.AddMinutes(-this.Aggregation.Period.Duration.TotalMinutes))
                    return;
                    
                this.ProcessAnalysis(0);
            }

            // Check higher-TF bucket boundaries on every tick using wall-clock time.
            // This detects M15 completion at e.g. 10:15 even on a 10-min chart
            // where the base bar (10:10-10:20) hasn't changed yet.
            this.CheckHigherTimeframes(currentBarTime);
            
            // Check for auto order conditions ONLY on new candle formation, not on chart changes
            if (this.EnableAutoOrders && !this._isBacktestMode && isNewCandle && this._candlesResults.Count > 0 && this._nextCandle.Confidence > 0)
            {
                this._autoOrderPlaced = this._autoOrderService.TryPlaceAutoOrder(
                    this.Symbol,
                    this.SelectedAccount,
                    this.RiskAmount,
                    this._candlesSummary,
                    this._nextCandle,
                    this.GetNextCandleLevels());
            }
        }
        catch (Exception ex)
        {
            RetroVisLog.Error($"OnUpdate error: {ex.Message}");
        }
    }

    private async Task FetchMatchesAsync(List<RawCandle> candles)
    {
        var lastCandle = candles[^1];
        var request = new QueryRequest
        {
            Symbol = this._ticker,
            SessionId = this._effectiveSession,
            Candles = candles,
            TopK = 5,
            Timestamp = (uint)lastCandle.Timestamp,
            Timeframe = this.GetFormattedTimeframe(),
            SessionOpen = (float)this._currentPriceBase
        };

        var fullData = await this._apiClient.FetchMatchesAsync(request);
        if (fullData == null) return;

        this._candlesResults = fullData.Matches;
        this._candlesSummary = fullData.Summary;
        this._nextCandle = fullData.NextCandle;

        
    }

    private ContextQueryRequest BuildContextQueryRequest()
    {
        const int closeHour = 17;
        const int closeMinute = 0;

        float sessionOpen = (float)this.Open(this._openBarIndex);
        var intraday = this._barReader.ExtractIntradayRange(this._openBarIndex, this.Count);
        var prevDay = this._barReader.ExtractPrevDayStats(this._openBarIndex, this.Count, closeHour, closeMinute);

        uint openTimestamp = (uint)new DateTimeOffset(this.Time(this._openBarIndex)).ToUnixTimeSeconds();

        return new ContextQueryRequest
        {
            Symbol = this._ticker,
            SessionId = this._effectiveSession,
            SessionOpen = sessionOpen,
            PrevClose = prevDay.PrevClose,
            PrevHigh = prevDay.PrevHigh,
            PrevLow = prevDay.PrevLow,
            IntradayHigh = intraday.High,
            IntradayLow = intraday.Low,
            TopK = 5,
            Timestamp = openTimestamp
        };
    }

    private async Task FetchContextMatchesAsync()
    {
        var request = this.BuildContextQueryRequest();
        var fullData = await this._apiClient.FetchContextMatchesAsync(request);
        if (fullData == null) return;

        this._contextResults = fullData.Matches;
        this._contextSummary = fullData.Summary;
    }

    /// <summary>
    /// Called on every new base candle. Checks whether any higher timeframe
    /// had a candle change (using timestamp floor division) and triggers
    /// a search for each changed timeframe.
    /// </summary>
    private void CheckHigherTimeframes(DateTime currentBarTime)
    {
        if (this._higherTfs.Length == 0 || this._openBarIndex == -1 || this._effectiveSession == null)
            return;

        uint currentTimestamp = (uint)DateTime.UtcNow.Subtract(new DateTime(1970, 1, 1)).TotalSeconds;

        foreach (string tf in this._higherTfs)
        {
            int tfMinutes = ParseTfMinutes(tf);
            if (tfMinutes <= 0) continue;

            long tfSeconds = tfMinutes * 60L;
            long tfIndex = currentTimestamp / tfSeconds;

            bool exists = this._higherTfStates.TryGetValue(tf, out var state);
            if (!exists || tfIndex != state.LastProcessedCandleIndex)
            {
                // Candle boundary crossed — trigger search
                float prevClose = state?.LastClose ?? 0f;
                this._higherTfStates[tf] = new HigherTfState(
                    new AggregateData(), new NextCandleData(), prevClose,
                    tfIndex, IsLoading: true);
                _ = this.FetchHigherTfMatchesAsync(tf);
            }
        }
    }

    /// <summary>
    /// Returns the duration in minutes for a formatted timeframe string (e.g. "M10" -> 10).
    /// </summary>
    private static int ParseTfMinutes(string tf)
    {
        if (tf.Length > 1 && tf[0] == 'M' && int.TryParse(tf[1..], out int minutes))
            return minutes;
        return 0;
    }

    /// <summary>
    /// Fetches search results for a higher timeframe. Pre-aggregates base candles
    /// into the target timeframe before sending to the server.
    /// </summary>
    private async Task FetchHigherTfMatchesAsync(string targetTf)
    {
        try
        {
            int targetMinutes = ParseTfMinutes(targetTf);
            if (targetMinutes <= 0) return;

            // Extract the most recent closed candles for aggregation.
            // We want the newest MaxCandles candles (not from session open)
            // so the aggregated higher-TF buckets cover the latest data.
            int firstIndex = Math.Min(this._openBarIndex, this._analysisIndex + this.MaxCandles);
            var baseCandles = this._barReader.ExtractCandles(firstIndex, this._analysisIndex, this.MaxCandles);
            if (baseCandles.Count == 0) return;

            // Include the current (open) bar so the last higher-TF bucket
            // is complete at boundary times (e.g. M15 at 10:15 on a M10
            // chart). The first tick after the boundary snapshots the
            // current bar's OHLC — the close is the price at that instant,
            // which is the correct bucket close.  After this single fetch
            // completes, the data stays stable until the next boundary.
            if (!this._isBacktestMode && this.Count > 0)
            {
                TimeZoneInfo nyTz = TimeZoneInfo.FindSystemTimeZoneById("Eastern Standard Time");
                DateTime barUtc = this.Time(0).ToUniversalTime();
                DateTime barNy = TimeZoneInfo.ConvertTimeFromUtc(barUtc, nyTz);
                uint tsNy = (uint)new DateTimeOffset(barNy, TimeSpan.Zero).ToUnixTimeSeconds();

                baseCandles.Add(new RawCandle
                {
                    Timestamp = tsNy,
                    Open = (float)this.Open(0),
                    High = (float)this.High(0),
                    Low = (float)this.Low(0),
                    Close = (float)this.Close(0),
                    Volume = Math.Max((float)this.Volume(0), 0f)
                });
            }

            // Pre-aggregate into target timeframe
            var aggregatedCandles = SessionBarReader.AggregateCandles(baseCandles, targetMinutes);

            // Determine last-bucket completeness using wall-clock time.
            // With the current (open) bar appended, the old end-time check
            // incorrectly PASSES at non-boundary times because the open
            // bar's nominal end time always spans the bucket.
            //
            // Instead we check whether the target-TF bucket end has truly
            // passed in real wall-clock time, using the same NY-as-UTC
            // convention as BarTimeNy so the comparison is aligned.
            if (aggregatedCandles.Count > 0)
            {
                uint targetTfSeconds = (uint)targetMinutes * 60;
                uint lastBucketEnd = aggregatedCandles[^1].Timestamp + targetTfSeconds;

                bool drop = false;
                if (this._isBacktestMode)
                {
                    // Backtest: no current-bar was appended, all candles
                    // are closed → classic end-time check.
                    uint baseTfSeconds = (uint)ParseTfMinutes(this._currentTimeframe) * 60;
                    uint lastBaseCandleEnd = baseCandles[^1].Timestamp + baseTfSeconds;
                    drop = lastBaseCandleEnd < lastBucketEnd;
                }
                else
                {
                    // Real-time: frame is complete only when wall-clock
                    // has passed the bucket's end.  Use the same NY-as-UTC
                    // conversion as BarTimeNy for alignment.
                    TimeZoneInfo nyTz = TimeZoneInfo.FindSystemTimeZoneById("Eastern Standard Time");
                    DateTime nowUtc = DateTime.UtcNow;
                    DateTime nowNy = TimeZoneInfo.ConvertTimeFromUtc(nowUtc, nyTz);
                    uint shiftedNow = (uint)new DateTimeOffset(nowNy, TimeSpan.Zero).ToUnixTimeSeconds();
                    drop = shiftedNow < lastBucketEnd;
                }

                if (drop)
                    aggregatedCandles.RemoveAt(aggregatedCandles.Count - 1);
            }

            // MinCandles is not applied here: higher-TF aggregation only
            // produces complete buckets (the incomplete-bucket check above
            // already drops partial ones), so any count is valid to send.

            if (aggregatedCandles.Count == 0) return;

            float sessionOpen = (float)this.Open(this._openBarIndex);
            uint timestamp = aggregatedCandles[^1].Timestamp;

            var request = new QueryRequest
            {
                Symbol = this._ticker,
                SessionId = this._effectiveSession,
                Candles = aggregatedCandles,
                TopK = 5,
                Timestamp = timestamp,
                Timeframe = targetTf,
                SessionOpen = sessionOpen
            };

            var fullData = await this._apiClient.FetchMatchesAsync(request);
            if (fullData == null) return;

            float lastAggClose = aggregatedCandles.Count > 0
                ? aggregatedCandles[^1].Close
                : 0f;
            this._higherTfStates[targetTf] = new HigherTfState(
                fullData.Summary ?? new AggregateData(),
                fullData.NextCandle ?? new NextCandleData(),
                lastAggClose,
                this._higherTfStates[targetTf].LastProcessedCandleIndex,
                IsLoading: false);
        }
        catch (Exception ex)
        {
            RetroVisLog.Error($"FetchHigherTfMatchesAsync ({targetTf}) error: {ex.Message}");
            // On error, preserve existing data including LastClose.
            if (this._higherTfStates.TryGetValue(targetTf, out var st))
                this._higherTfStates[targetTf] = st with { IsLoading = false };
        }
    }

    /// <summary>
    /// Retrieves AggregateData and NextCandleData for any timeframe
    /// (current or higher). Used by level-drawing methods.
    /// </summary>
    private (AggregateData Summary, NextCandleData NextCandle) GetDataForTimeframe(string tf)
    {
        if (tf == this._currentTimeframe || !this._higherTfStates.TryGetValue(tf, out var state))
            return (this._candlesSummary, this._nextCandle);
        return (state.Summary, state.NextCandle);
    }

    /// <summary>
    /// Builds the ordered list of display data for the multi-timeframe panel.
    /// Current timeframe first, then higher TFs.
    /// </summary>
    private List<HigherTfDisplayData> BuildPanelData()
    {
        var list = new List<HigherTfDisplayData>();

        // Current timeframe always included
        list.Add(new HigherTfDisplayData(
            this._currentTimeframe,
            this._candlesSummary,
            this._nextCandle));

        // Add higher timeframes that have completed their fetch
        foreach (string tf in this._higherTfs)
        {
            if (this._higherTfStates.TryGetValue(tf, out var state) && !state.IsLoading)
            {
                list.Add(new HigherTfDisplayData(
                    tf,
                    state.Summary,
                    state.NextCandle));
            }
        }

        return list;
    }

    public override void OnPaintChart(PaintChartEventArgs args)
    {
        try
        {
            Graphics gr = args.Graphics;
            this._panelRenderer.DrawToggleButtons(gr, this._toggleRect, this._backtestBtnRect, this._isPanelVisible, this._isBacktestMode);

            if (this._isPanelVisible)
            {
                this.DrawLevelsOnChart(gr);
                this.DrawResultsPanelOnChart(gr);

                // Draw multi-timeframe panel (replaces old single alert circle)
                var chartRect = this.CurrentChart.MainWindow.ClientRectangle;
                var panelData = this.BuildPanelData();
                var multiTfResult = this._multiTfRenderer.DrawMultiTimeframePanel(
                    gr, chartRect, panelData, this._selectedTimeframe);
                this._multiTfClickAreas = [.. multiTfResult.ClickAreas];

                if (this.SelectedAccount != null)
                    this._panelRenderer.DrawOrderButtons(gr, this.GetLongButtonRect(), this.GetShortButtonRect());
            }
        }
        catch (Exception ex)
        {
            RetroVisLog.Error($"OnPaintChart error: {ex.Message}");
        }
    }

    private void DrawLevelsOnChart(Graphics gr)
    {
        if (!this.DrawSessionLevels && !this.DrawNextCandleLevels) return;
        if (this._currentCandleCount < this.MinCandles) return;
        if (this.Count <= 0) return;

        var mainWindow = this.CurrentChart.MainWindow;
        int drawBarIndex = Math.Clamp(this._openBarIndex, 0, this.Count - 1);
        DateTime sessionStartTime = this.Time(drawBarIndex);
        var (sessionUp, sessionDown) = this.GetSessionLevels();
        var (nextCandleUp, nextCandleDown) = this.GetNextCandleLevels();

        PanelRenderer.DrawLevels(
            gr, mainWindow,
            sessionStartTime,
            sessionUp: sessionUp,
            sessionDown: sessionDown,
            nextCandleUp: nextCandleUp,
            nextCandleDown: nextCandleDown,
            this.DrawSessionLevels, this.DrawNextCandleLevels);
    }

    /// <summary>
    /// Same next-candle level calculation used both for drawing the dashed
    /// lines and for determining the TP target when placing an order, so
    /// the two always agree on what "the TP level" means.
    /// </summary>
    private (double Up, double Down) GetNextCandleLevels()
    {
        return GetNextCandleLevelsFor(this._selectedTimeframe);
    }

    private (double Up, double Down) GetNextCandleLevelsFor(string tf)
    {
        // For the current timeframe, use the closed-bar price as the anchor.
        // For higher timeframes, use the last aggregated bucket's close.
        if (tf == this._currentTimeframe || !this._higherTfStates.TryGetValue(tf, out var state))
        {
            int baseCandleIndex = this._analysisIndex + 1;
            if (this.MaxCandles > 0 && (this._openBarIndex - baseCandleIndex) >= this.MaxCandles)
                baseCandleIndex = this._openBarIndex - this.MaxCandles + 1;
            baseCandleIndex = Math.Clamp(baseCandleIndex, 0, Math.Max(0, this.Count - 1));

            var (_, nextCandle) = this.GetDataForTimeframe(tf);
            double nextCandleClose = this.Close(baseCandleIndex);
            return (nextCandleClose + nextCandle.AvgUp, nextCandleClose - nextCandle.AvgDown);
        }
        else
        {
            // Higher timeframe: anchor on the aggregated bucket's own close.
            double close = state.LastClose;
            return (close + state.NextCandle.AvgUp, close - state.NextCandle.AvgDown);
        }
    }

    /// <summary>
    /// Same session-level calculation used for drawing the solid lines.
    /// Uses the currently selected timeframe's aggregated data.
    /// </summary>
    private (double Up, double Down) GetSessionLevels()
    {
        return GetSessionLevelsFor(this._selectedTimeframe);
    }

    private (double Up, double Down) GetSessionLevelsFor(string tf)
    {
        // For the current timeframe, use the closed-bar price as the anchor.
        // For higher timeframes, use the last aggregated bucket's close.
        if (tf == this._currentTimeframe || !this._higherTfStates.TryGetValue(tf, out var state))
        {
            int baseCandleIndex = this._analysisIndex + 1;
            if (this.MaxCandles > 0 && (this._openBarIndex - baseCandleIndex) >= this.MaxCandles)
                baseCandleIndex = this._openBarIndex - this.MaxCandles + 1;
            baseCandleIndex = Math.Clamp(baseCandleIndex, 0, Math.Max(0, this.Count - 1));

            double priceBase = this.Close(baseCandleIndex);
            var (summary, _) = this.GetDataForTimeframe(tf);
            return (priceBase + summary.AvgUp, priceBase - summary.AvgDown);
        }
        else
        {
            // Higher timeframe: anchor on the aggregated bucket's own close.
            double close = state.LastClose;
            return (close + state.Summary.AvgUp, close - state.Summary.AvgDown);
        }
    }

    /// <summary>
    /// Picks which level pair feeds the order TP/sizing, per OrderTpSource.
    /// </summary>
    private (double Up, double Down) GetOrderTpLevels() =>
        this.OrderTpSource == "Session" ? this.GetSessionLevels() : this.GetNextCandleLevels();

    private Rectangle GetLongButtonRect() => new(this._toggleRect.X, this._toggleRect.Y + 35 + this._lastPanelHeight + 10, 135, 30);
    private Rectangle GetShortButtonRect() => new(this._toggleRect.X + 145, this._toggleRect.Y + 35 + this._lastPanelHeight + 10, 135, 30);

    private void DrawResultsPanelOnChart(Graphics gr)
    {
        // Calculate position sizes for display
        double currentPrice = this.Symbol.Last;
        var (levelUp, levelDown) = this.GetOrderTpLevels();
        
        double longQty = 0, shortQty = 0;
        if (this.Symbol != null && this._currentCandleCount >= this.MinCandles)
        {
            var longResult = PositionSizer.CalculateSize(
                this.Symbol, currentPrice, levelUp, this.RiskAmount);
            longQty = longResult.IsValid ? longResult.Quantity : 0;

            var shortResult = PositionSizer.CalculateSize(
                this.Symbol, currentPrice, levelDown, this.RiskAmount);
            shortQty = shortResult.IsValid ? shortResult.Quantity : 0;
        }

        var state = new PanelState(
            EffectiveSession: this._effectiveSession,
            CurrentCandleCount: this._currentCandleCount,
            MinCandles: this.MinCandles,
            CurrentDelta: this._currentDelta,
            CandlesSummary: this._candlesSummary,
            NextCandle: this._nextCandle,
            CandlesResults: this._candlesResults,
            ContextSummary: this._contextSummary,
            ContextResults: this._contextResults,
            IsLoadingNewAnalysis: this._currentAnalysisText == "Loading new Analysis...",
            PositionSizeLong: longQty,
            PositionSizeShort: shortQty,
            HasValidPositionSize: longQty > 0 || shortQty > 0,
            ShowContextMatches: this.ShowContextMatches
        );

        Point origin = new(this._toggleRect.X, this._toggleRect.Y + 35);
        var result = this._panelRenderer.DrawResultsPanel(gr, origin, 280, this._lastPanelHeight, state);

        this._lastPanelHeight = result.PanelHeight;
        this._matchAreas = [.. result.MatchAreas];
        this._contextAreas = [.. result.ContextAreas];
    }

    private void OnChartMouseDown(object sender, ChartMouseNativeEventArgs e)
    {
        this._inputHandler.OnMouseDown(e);
    }

    private void OnChartMouseUp(object sender, ChartMouseNativeEventArgs e)
    {
        // Check multi-timeframe panel circle clicks
        foreach (var area in this._multiTfClickAreas)
        {
            if (area.Rect.Contains(e.Location))
            {
                this._selectedTimeframe = area.Timeframe;
                return;
            }
        }

        Rectangle longRect = this.SelectedAccount != null ? this.GetLongButtonRect() : Rectangle.Empty;
        Rectangle shortRect = this.SelectedAccount != null ? this.GetShortButtonRect() : Rectangle.Empty;

        var result = this._inputHandler.OnMouseUp(
            e, this._toggleRect, this._backtestBtnRect, longRect, shortRect,
            this._isPanelVisible, this._isBacktestMode,
            this._matchAreas, this._contextAreas, this._ticker, this.GetFormattedTimeframe());

        switch (result.Action)
        {
            case InputAction.TogglePanel:
                this._isPanelVisible = !this._isPanelVisible;
                break;
            case InputAction.ToggleBacktest:
                this._isBacktestMode = !this._isBacktestMode;
                if (!this._isBacktestMode)
                {
                    this._lastProcessedBarTime = default;
                    this.RunManualAnalysis(0);
                }
                break;
            case InputAction.RunManualAnalysis:
                this.RunManualAnalysis(result.CandleIndex);
                break;
            case InputAction.PlaceLong:
                this.PlaceOrder(Side.Buy);
                break;
            case InputAction.PlaceShort:
                this.PlaceOrder(Side.Sell);
                break;
        }
    }

    private void PlaceOrder(Side side)
    {
        var (levelUp, levelDown) = this.GetOrderTpLevels();
        double entryPrice = this.Symbol.Last;
        double tpPrice = side == Side.Buy ? levelUp : levelDown;

        this._orderService.PlaceMarketOrder(side, this.Symbol, this.SelectedAccount, entryPrice, tpPrice, this.RiskAmount);
    }

    protected void RunManualAnalysis(int candleIndex)
    {
        this.Reset();
        this.ProcessAnalysis(candleIndex);
        // Also fetch higher-TF data for the manually selected candle
        if (this._openBarIndex != -1)
        {
            DateTime candleTime = this.Time(this._analysisIndex);
            this.CheckHigherTimeframes(candleTime);
        }
    }

    private void ProcessAnalysis(int refIndex)
    {
        this._analysisIndex = refIndex;

        string sessionToUse = this.SessionId;
        if (this.SessionId == "AUTO")
        {
            sessionToUse = this._barReader.ResolveAutoSession(refIndex);
            if (sessionToUse == null)
            {
                this._effectiveSession = null;
                return;
            }
        }

        this._openBarIndex = this._barReader.FindSessionOpenBarIndex(refIndex, sessionToUse, this.Count);
        if (this._openBarIndex == -1) return;
        if (this._openBarIndex >= this.MaxCandles && this._candlesResults.Count > 0) return;

        this._effectiveSession = sessionToUse;

        string sessionDateKey = $"{sessionToUse}_{this.Time(this._openBarIndex):yyyy-MM-dd}";
        if (sessionDateKey != this._lastContextSessionDate)
        {
            this._lastContextSessionDate = sessionDateKey;
            _ = this.FetchContextMatchesAsync();
        }

        if ((this._openBarIndex - refIndex) < this.MinCandles) return;

        var candlesToInquire = this._barReader.ExtractCandles(this._openBarIndex, refIndex, this.MaxCandles);

        this._currentPriceBase = (float)this.Open(this._openBarIndex);
        this._currentDelta = (float)this.Close(refIndex) - this._currentPriceBase;
        this._currentCandleCount = candlesToInquire.Count;
        _ = this.FetchMatchesAsync(candlesToInquire);
    }

    private string GetFormattedTimeframe()
    {
        string tf = this.Aggregation.Period.Format();

        if (tf.EndsWith('m')) return "M" + tf.Replace("m", "");
        if (tf.EndsWith('h')) return "H" + tf.Replace("h", "");
        return tf;
    }

    public override void Dispose()
    {
        this.CurrentChart.MouseUp -= this.OnChartMouseUp;
        this.CurrentChart.MouseDown -= this.OnChartMouseDown;
        this._alertService?.Dispose();
        this._apiClient?.Dispose();
        this._autoOrderService?.Dispose();
        this._multiTfRenderer?.Dispose();
        this._higherTfStates.Clear();
        GC.SuppressFinalize(this);
    }

    protected void InitTicker()
    {
        string symbol = this.Symbol.Description.ToLower();
        if (symbol.Contains("nasdaq")) this._ticker = "NQ";
        else if (symbol.Contains("s&p")) this._ticker = "ES";
        else if (symbol.Contains("gold")) this._ticker = "GC";
        else if (symbol.Contains("dow")) this._ticker = "YM";
    }
}
