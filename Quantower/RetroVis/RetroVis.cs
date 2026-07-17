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

    private AggregateData _candlesSummary = new();
    private NextCandleData _nextCandle = new();
    private List<MatchResponse> _candlesResults = [];
    private bool _isPanelVisible = true;
    private readonly Rectangle _toggleRect = new(5, 80, 120, 25);
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
    private readonly Rectangle _backtestBtnRect = new(130, 80, 130, 25);

    private string _currentAnalysisText;

    private int _analysisIndex;

    // Blinking circle state
    private bool _alertEdgePositive;
    private static readonly TimeSpan BlinkDuration = TimeSpan.FromSeconds(3);
    private static readonly TimeSpan BlinkInterval = TimeSpan.FromMilliseconds(300);
    #endregion

    public RetroVis() : base()
    {
        this.Name = "RetroVis";
        this.SeparateWindow = false;
    }

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
        int baseCandleIndex = this._analysisIndex + 1;
        if (this.MaxCandles > 0 && (this._openBarIndex - baseCandleIndex) >= this.MaxCandles)
            baseCandleIndex = this._openBarIndex - this.MaxCandles + 1;
        baseCandleIndex = Math.Clamp(baseCandleIndex, 0, Math.Max(0, this.Count - 1));

        double nextCandleClose = this.Close(baseCandleIndex);
        return (nextCandleClose + this._nextCandle.AvgUp, nextCandleClose - this._nextCandle.AvgDown);
    }

    /// <summary>
    /// Same session-level calculation used for drawing the solid lines.
    /// </summary>
    private (double Up, double Down) GetSessionLevels()
        {
            int baseCandleIndex = this._analysisIndex + 1;
            if (this.MaxCandles > 0 && (this._openBarIndex - baseCandleIndex) >= this.MaxCandles)
                baseCandleIndex = this._openBarIndex - this.MaxCandles + 1;
                baseCandleIndex = Math.Clamp(baseCandleIndex, 0, Math.Max(0, this.Count - 1));

            double _currentPriceBase = this.Close(baseCandleIndex);
            return (_currentPriceBase + this._candlesSummary.AvgUp, _currentPriceBase - this._candlesSummary.AvgDown);
    }

    /// <summary>
    /// Picks which level pair feeds the order TP/sizing, per OrderTpSource.
    /// </summary>
    private (double Up, double Down) GetOrderTpLevels() =>
        this.OrderTpSource == "Session" ? this.GetSessionLevels() : this.GetNextCandleLevels();

    private Rectangle GetLongButtonRect() => new(this._toggleRect.X, this._toggleRect.Y + 35 + this._lastPanelHeight + 10, 160, 30);
    private Rectangle GetShortButtonRect() => new(this._toggleRect.X + 170, this._toggleRect.Y + 35 + this._lastPanelHeight + 10, 160, 30);

    private void DrawResultsPanelOnChart(Graphics gr)
    {
        var sessionUp = this._candlesSummary.AvgUp > this._candlesSummary.AvgDown;
        var candleUp = this._nextCandle.AvgUp > this._nextCandle.AvgDown;
        if (this._candlesSummary.Confidence >= 70f && this._nextCandle.Confidence >= 70f && sessionUp == candleUp)
        {
            var chartRect = this.CurrentChart.MainWindow.ClientRectangle;
            this._alertEdgePositive = this._candlesSummary.AvgUp > Math.Abs(this._candlesSummary.AvgDown);
            PanelRenderer.DrawAlertCircle(gr, chartRect, this._alertEdgePositive);
        }

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
        var result = this._panelRenderer.DrawResultsPanel(gr, origin, 330, this._lastPanelHeight, state);

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
