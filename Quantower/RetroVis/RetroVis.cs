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

    [InputParameter("Session Type", 20, variants: ["AUTO", "AUTO", "NY_OPEN", "NY_OPEN", "LONDON_OPEN", "LONDON_OPEN"])]
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

    

    [InputParameter("Order TP Source", 12, variants: ["Next Candle Levels", "NextCandle", "Session Levels", "Session"])]
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

    private int _currentCandleCount = 0;
    private string _ticker = "";
    private string _effectiveSession;

    private CloseAlertService _alertService;
    private ChartInputHandler _inputHandler;
    private readonly OrderService _orderService = new();

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
    private bool _isBlinking;
    private DateTime _blinkStartTime;
    private bool _blinkEdgePositive;
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
        _apiClient = new RetroVisApiClient(this.ServerUrl, TimeSpan.FromSeconds(60));

        try
        {
            _nyTz = TimeZoneInfo.FindSystemTimeZoneById("Eastern Standard Time");
            _londonTz = TimeZoneInfo.FindSystemTimeZoneById("GMT Standard Time");
        }
        catch (Exception ex)
        {
            RetroVisLog.Error("CRITICAL: Timezone non trovata! " + ex.Message);
        }

        _barReader = new SessionBarReader(this.HistoricalData, _nyTz, _londonTz);
        _alertService = new CloseAlertService(this.HistoricalData);
        _inputHandler = new ChartInputHandler(this.HistoricalData);

        _lastProcessedBarTime = DateTime.MinValue;
        _alertService.LoadSound(this.AlertSoundPath);

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
        _matchAreas.Clear();
        _contextAreas.Clear();
    }

    protected void Reset()
    {
        _lastProcessedBarTime = default;
        _candlesResults.Clear();
        _contextResults.Clear();
        this.ResetLists();
        _currentCandleCount = 0;
        _lastContextSessionDate = "";
        _currentAnalysisText = "Loading new Analysis...";
    }

    protected override void OnSettingsUpdated()
    {
        try
        {
            base.OnSettingsUpdated();
            _alertService?.LoadSound(this.AlertSoundPath);
            this.Reset();
        }
        catch (Exception ex)
        {
            RetroVisLog.Error($"OnSettingsUpdated error: {ex.Message}");
        }
    }

    private HistoryAggregationTime Aggregation => (HistoryAggregationTime)this.HistoricalData.Aggregation;

    protected override void OnUpdate(UpdateArgs args)
    {
        try
        {
            _alertService.CheckAndPlay(this.EnableCloseAlert, _isBacktestMode, this.AlertSecondsBeforeClose);

            if (_isBacktestMode || !_isPanelVisible) return;
            _currentDelta = this.Close(0) - _currentPriceBase;

            DateTime currentBarTime = this.Time();
            if (currentBarTime == _lastProcessedBarTime) return;

            this.Reset();

            if (currentBarTime.ToUniversalTime() < DateTime.UtcNow.AddMinutes(-this.Aggregation.Period.Duration.TotalMinutes))
                return;

            _lastProcessedBarTime = currentBarTime;
            this.ProcessAnalysis(0);
        }
        catch (Exception ex)
        {
            RetroVisLog.Error($"OnUpdate error: {ex.Message}");
        }
    }

    private async Task FetchMatchesAsync(List<RawCandle> candles)
    {
        var lastCandle = candles[candles.Count - 1];
        var request = new QueryRequest
        {
            Symbol = _ticker,
            SessionId = _effectiveSession,
            Candles = candles,
            TopK = 5,
            Timestamp = (uint)lastCandle.Timestamp,
            Timeframe = this.GetFormattedTimeframe(),
            SessionOpen = (float)_currentPriceBase
        };

        var fullData = await _apiClient.FetchMatchesAsync(request);
        if (fullData == null) return;

        _candlesResults = fullData.Matches;
        _candlesSummary = fullData.Summary;
        _nextCandle = fullData.NextCandle;

        // Trigger blinking circle when both confidences >= 70%
        if (_candlesSummary.Confidence >= 70f && _nextCandle.Confidence >= 70f)
        {
            _isBlinking = true;
            _blinkStartTime = DateTime.UtcNow;
            _blinkEdgePositive = _candlesSummary.AvgUp > Math.Abs(_candlesSummary.AvgDown);
        }
    }

    private ContextQueryRequest BuildContextQueryRequest()
    {
        const int closeHour = 17;
        const int closeMinute = 0;

        float sessionOpen = (float)this.Open(_openBarIndex);
        var intraday = _barReader.ExtractIntradayRange(_openBarIndex, this.Count);
        var prevDay = _barReader.ExtractPrevDayStats(_openBarIndex, this.Count, closeHour, closeMinute);

        uint openTimestamp = (uint)new DateTimeOffset(this.Time(_openBarIndex)).ToUnixTimeSeconds();

        return new ContextQueryRequest
        {
            Symbol = _ticker,
            SessionId = _effectiveSession,
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
        var fullData = await _apiClient.FetchContextMatchesAsync(request);
        if (fullData == null) return;

        _contextResults = fullData.Matches;
        _contextSummary = fullData.Summary;
    }

    public override void OnPaintChart(PaintChartEventArgs args)
    {
        try
        {
            Graphics gr = args.Graphics;
            _panelRenderer.DrawToggleButtons(gr, _toggleRect, _backtestBtnRect, _isPanelVisible, _isBacktestMode);

            if (_isPanelVisible)
            {
                this.DrawLevelsOnChart(gr);
                this.DrawResultsPanelOnChart(gr);
                if (this.SelectedAccount != null)
                    _panelRenderer.DrawOrderButtons(gr, this.GetLongButtonRect(), this.GetShortButtonRect());
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
        if (_currentCandleCount < this.MinCandles) return;
        if (this.Count <= 0) return;

        var mainWindow = this.CurrentChart.MainWindow;
        int drawBarIndex = Math.Clamp(_openBarIndex, 0, this.Count - 1);
        DateTime sessionStartTime = this.Time(drawBarIndex);

        var (nextCandleUp, nextCandleDown) = this.GetNextCandleLevels();

        _panelRenderer.DrawLevels(
            gr, mainWindow,
            sessionStartTime, sessionStartTime,
            sessionUp: _currentPriceBase + _candlesSummary.AvgUp,
            sessionDown: _currentPriceBase - _candlesSummary.AvgDown,
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
        int baseCandleIndex = _analysisIndex + 1;
        if (this.MaxCandles > 0 && (_openBarIndex - baseCandleIndex) >= this.MaxCandles)
            baseCandleIndex = _openBarIndex - this.MaxCandles + 1;
        baseCandleIndex = Math.Clamp(baseCandleIndex, 0, Math.Max(0, this.Count - 1));

        double nextCandleClose = this.Close(baseCandleIndex);
        return (nextCandleClose + _nextCandle.AvgUp, nextCandleClose - _nextCandle.AvgDown);
    }

    /// <summary>
    /// Same session-level calculation used for drawing the solid lines.
    /// </summary>
    private (double Up, double Down) GetSessionLevels() =>
        (_currentPriceBase + _candlesSummary.AvgUp, _currentPriceBase - _candlesSummary.AvgDown);

    /// <summary>
    /// Picks which level pair feeds the order TP/sizing, per OrderTpSource.
    /// </summary>
    private (double Up, double Down) GetOrderTpLevels() =>
        this.OrderTpSource == "Session" ? this.GetSessionLevels() : this.GetNextCandleLevels();

    private Rectangle GetLongButtonRect() => new(_toggleRect.X, _toggleRect.Y + 35 + _lastPanelHeight + 10, 160, 30);
    private Rectangle GetShortButtonRect() => new(_toggleRect.X + 170, _toggleRect.Y + 35 + _lastPanelHeight + 10, 160, 30);

    private void DrawResultsPanelOnChart(Graphics gr)
    {
        // Draw blinking circle in top-left corner of chart
        if (_isBlinking)
        {
            var elapsed = DateTime.UtcNow - _blinkStartTime;
            if (elapsed < BlinkDuration)
            {
                bool visible = (int)(elapsed.TotalMilliseconds / BlinkInterval.TotalMilliseconds) % 2 == 0;
                if (visible)
                {
                    var chartRect = this.CurrentChart.MainWindow.ClientRectangle;
                    _panelRenderer.DrawBlinkingCircle(gr, chartRect, _blinkEdgePositive);
                }
            }
            else
            {
                _isBlinking = false;
            }
        }

        var state = new PanelState(
            EffectiveSession: _effectiveSession,
            CurrentCandleCount: _currentCandleCount,
            MinCandles: this.MinCandles,
            CurrentDelta: _currentDelta,
            CandlesSummary: _candlesSummary,
            NextCandle: _nextCandle,
            CandlesResults: _candlesResults,
            ContextSummary: _contextSummary,
            ContextResults: _contextResults,
            IsLoadingNewAnalysis: _currentAnalysisText == "Loading new Analysis...");

        Point origin = new(_toggleRect.X, _toggleRect.Y + 35);
        var result = _panelRenderer.DrawResultsPanel(gr, origin, 330, _lastPanelHeight, state);

        _lastPanelHeight = result.PanelHeight;
        _matchAreas = new List<MatchClickArea>(result.MatchAreas);
        _contextAreas = new List<MatchClickArea>(result.ContextAreas);
    }

    private void OnChartMouseDown(object sender, ChartMouseNativeEventArgs e)
    {
        _inputHandler.OnMouseDown(e);
    }

    private void OnChartMouseUp(object sender, ChartMouseNativeEventArgs e)
    {
        Rectangle longRect = this.SelectedAccount != null ? this.GetLongButtonRect() : Rectangle.Empty;
        Rectangle shortRect = this.SelectedAccount != null ? this.GetShortButtonRect() : Rectangle.Empty;

        var result = _inputHandler.OnMouseUp(
            e, _toggleRect, _backtestBtnRect, longRect, shortRect,
            _isPanelVisible, _isBacktestMode,
            _matchAreas, _contextAreas, _ticker, this.GetFormattedTimeframe());

        switch (result.Action)
        {
            case InputAction.TogglePanel:
                _isPanelVisible = !_isPanelVisible;
                break;
            case InputAction.ToggleBacktest:
                _isBacktestMode = !_isBacktestMode;
                if (!_isBacktestMode) _lastProcessedBarTime = default;
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

        _orderService.PlaceMarketOrder(side, this.Symbol, this.SelectedAccount, entryPrice, tpPrice, this.RiskAmount);
    }

    protected void RunManualAnalysis(int candleIndex)
    {
        this.Reset();
        this.ProcessAnalysis(candleIndex);
    }

    private void ProcessAnalysis(int refIndex)
    {
        _analysisIndex = refIndex;

        string sessionToUse = this.SessionId;
        if (this.SessionId == "AUTO")
        {
            sessionToUse = _barReader.ResolveAutoSession(refIndex);
            if (sessionToUse == null)
            {
                _effectiveSession = null;
                return;
            }
        }

        _openBarIndex = _barReader.FindSessionOpenBarIndex(refIndex, sessionToUse, this.Count);
        if (_openBarIndex == -1) return;
        if (_openBarIndex >= this.MaxCandles && _candlesResults.Count > 0) return;

        _effectiveSession = sessionToUse;

        string sessionDateKey = $"{sessionToUse}_{this.Time(_openBarIndex):yyyy-MM-dd}";
        if (sessionDateKey != _lastContextSessionDate)
        {
            _lastContextSessionDate = sessionDateKey;
            _ = this.FetchContextMatchesAsync();
        }

        if ((_openBarIndex - refIndex) < this.MinCandles) return;

        var candlesToInquire = _barReader.ExtractCandles(_openBarIndex, refIndex, this.MaxCandles);

        _currentPriceBase = (float)this.Open(_openBarIndex);
        _currentDelta = (float)this.Close(refIndex) - _currentPriceBase;
        _currentCandleCount = candlesToInquire.Count;
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
        _alertService?.Dispose();
        _apiClient?.Dispose();
        GC.SuppressFinalize(this);
    }

    protected void InitTicker()
    {
        string symbol = this.Symbol.Description.ToLower();
        if (symbol.Contains("nasdaq")) _ticker = "NQ";
        else if (symbol.Contains("s&p")) _ticker = "ES";
        else if (symbol.Contains("gold")) _ticker = "GC";
        else if (symbol.Contains("dow")) _ticker = "YM";
    }
}
