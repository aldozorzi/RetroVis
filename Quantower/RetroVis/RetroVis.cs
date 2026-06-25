using System;
using System.Collections.Generic;
using System.ComponentModel.DataAnnotations;
using System.Drawing;
using System.Drawing.Printing;
using System.IO;
using System.Media;
using System.Net.Http;
using System.Net.Http.Json;
using System.Reflection.Metadata;
using System.Runtime.Versioning;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using System.Threading.Tasks;
using TradingPlatform.BusinessLayer;
using TradingPlatform.BusinessLayer.Chart;
using static System.Runtime.InteropServices.JavaScript.JSType;

namespace Ldazrz.RetroVis;

[SupportedOSPlatform("windows")]


public class RetroVis : Indicator
{
    #region Parameters
    [InputParameter("Session Type", 0, variants: ["AUTO", "AUTO", "NY_OPEN", "NY_OPEN", "LONDON_OPEN", "LONDON_OPEN" ])]
    public string SessionId = "AUTO";

    [InputParameter("Min Candles for Pattern", 1, 1, 15, 1)]
    public int MinCandles = 1;

    [InputParameter("Max Candles for Pattern", 2, 1, 20, 1)]
    public int MaxCandles = 6;

    [InputParameter("Webserver URL", 3)]
    public string ServerUrl = "http://127.0.0.1:9999";

    [InputParameter("Draw Session Levels", 5)]
    public bool DrawSessionLevels { get; set; } = true;

    [InputParameter("Draw Next Candle Levels", 5)]
    public bool DrawNextCandleLevels { get; set; } = true;

    [InputParameter("Enable Candle Close Alert", 6)]
    public bool EnableCloseAlert { get; set; } = true;

    [InputParameter("Alert Seconds Before Close", 7, 1, 60, 1)]
    public int AlertSecondsBeforeClose = 10;

    [InputParameter("Alert Sound File (.wav)", 8)]
    public string AlertSoundPath = @"E:\Risorse\suoni\ding.wav";

    #endregion

    #region Internal Fields
    private readonly HttpClient httpClient = new();
    private AggregateData candlesSummary = new();
    private NextCandleData nextCandle = new();
    private List<MatchResponse> candlesResults = [];
    private bool isPanelVisible = true;
    private Rectangle toggleRect = new (5, 80, 120, 25);
    private int openBarIndex = -1;
    private double currentPriceBase = 0;
    private DateTime lastProcessedBarTime;
    
    private int currentCandleCount = 0;
    private string ticker = "";
    private string effectiveSession;

    private SoundPlayer alertSoundPlayer;
    private string loadedAlertSoundPath = "";
    private static DateTime lastAlertBarTime = DateTime.MinValue;

    private static TimeZoneInfo NyTz ;
    private static TimeZoneInfo LondonTz;

    double currentDelta = 0;

    private readonly List<RectangleF> matchClickAreas = [];
    private readonly List<string> matchDates = [];

    public string SearchUrl { get; set; }
    public string ContextUrl { get; set; }
    private AggregateData contextSummary = new ();
    private List<MatchResponse> contextResults = [];
    private string lastContextSessionDate = ""; // traccia se il contesto è già stato caricato per questa apertura

    private readonly List<RectangleF> contextClickAreas = [];
    private readonly List<string> contextDates = [];
    private int lastPanelHeight = 0;

    private bool isBacktestMode = false;
    private Rectangle backtestBtnRect = new (130, 80, 130, 25); // Accanto al primo bottone

    private string currentAnalysisText;
    
    private readonly Font dataFont = new ("Consolas", 9);
    private readonly Font btnfont = new ("Arial", 7, FontStyle.Bold);
    private readonly Font headerFont = new ("Consolas", 11, FontStyle.Bold);
    private readonly Font bodyFont = new ("Consolas", 10);
    private readonly SolidBrush panelBgBrush = new (Color.FromArgb(220, 10, 10, 10));
    private readonly SolidBrush matchBgBrush = new (Color.FromArgb(0, 180, 100, 30));
    private readonly Pen upPen = new (Color.LimeGreen, 2);
    private readonly Pen downPen = new (Color.Sienna, 2);

    private bool chartMouseDownReceived = false;

    private int analysisIndex;
    #endregion

    public RetroVis() : base()
    {
        this.Name = "RetroVis";
        this.SeparateWindow = false;
    }
    protected override void OnInit()
    {
        this.httpClient.Timeout = TimeSpan.FromSeconds(60);

        this.SearchUrl = $"{this.ServerUrl.TrimEnd('/')}/search";
        this.ContextUrl = $"{this.ServerUrl.TrimEnd('/')}/context-search";
        try
        {
            NyTz = TimeZoneInfo.FindSystemTimeZoneById("Eastern Standard Time");
            LondonTz = TimeZoneInfo.FindSystemTimeZoneById("GMT Standard Time");
        }
        catch (Exception ex)
        {
            Log("CRITICAL: Timezone non trovata! " + ex.Message);
        }

        this.lastProcessedBarTime = DateTime.MinValue;
        RetroVis.lastAlertBarTime = DateTime.MinValue;
        this.InitAlertSound();
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
        this.matchClickAreas.Clear();
        this.matchDates.Clear();
        this.contextClickAreas.Clear();
        this.contextDates.Clear();
    }

    protected void Reset()
    {
        this.lastProcessedBarTime = default;
        this.candlesResults.Clear();
        this.contextResults.Clear();
        this.ResetLists();
        this.currentCandleCount = 0;
        this.lastContextSessionDate = "";
        this.currentAnalysisText = "Loading new Analysis...";
    }

    protected override void OnSettingsUpdated()
    {
        base.OnSettingsUpdated();
        this.InitAlertSound();
        this.Reset();
    }

    private void InitAlertSound()
    {
        try
        {
            if (string.IsNullOrWhiteSpace(this.AlertSoundPath))
            {
                this.alertSoundPlayer = null;
                this.loadedAlertSoundPath = "";
                return;
            }
            if (this.AlertSoundPath == this.loadedAlertSoundPath && this.alertSoundPlayer != null)
                return; // già caricato, evita I/O ripetuto

            if (!File.Exists(this.AlertSoundPath))
            {
                Log("Alert sound file non trovato: " + this.AlertSoundPath);
                this.alertSoundPlayer = null;
                this.loadedAlertSoundPath = "";
                return;
            }

            this.alertSoundPlayer = new SoundPlayer(this.AlertSoundPath);
            this.alertSoundPlayer.Load(); // carica subito in memoria, evita ritardo al primo Play()
            this.loadedAlertSoundPath = this.AlertSoundPath;
        }
        catch (Exception ex)
        {
            Log("Errore caricamento alert sound: " + ex.Message);
            this.alertSoundPlayer = null;
            this.loadedAlertSoundPath = "";
        }
    }

    private void CheckCloseAlert()
    {
        if (!this.EnableCloseAlert || this.isBacktestMode) return;
        if (this.alertSoundPlayer == null) return;

        HistoryAggregationTime aggregation = (HistoryAggregationTime)this.HistoricalData.Aggregation;
        TimeSpan duration = aggregation.Period.Duration;

        // Solo intraday: escludi D1/W1/MN (>= 1 giorno)
        if (duration >= TimeSpan.FromDays(1)) return;
        if (duration <= TimeSpan.Zero) return;

        DateTime currentBarTime = this.Time();
        if (currentBarTime == RetroVis.lastAlertBarTime) return; // già suonato per questa barra

        DateTime barCloseTimeUtc = currentBarTime.ToUniversalTime() + duration;
        double secondsRemaining = (barCloseTimeUtc - DateTime.UtcNow).TotalSeconds;

        if (secondsRemaining > 0 && secondsRemaining <= this.AlertSecondsBeforeClose)
        {
            RetroVis.lastAlertBarTime = currentBarTime;
            try
            {
                this.alertSoundPlayer.Play(); // asincrono, non blocca il thread di update
            }
            catch (Exception ex)
            {
                Log("Errore riproduzione alert sound: " + ex.Message);
            }
        }
    }

    protected override void OnUpdate(UpdateArgs args)
    {
        this.CheckCloseAlert();

        if (this.isBacktestMode || !this.isPanelVisible) return;
        this.currentDelta = this.Close(0) - this.currentPriceBase;

        DateTime currentBarTime = this.Time();
        if (currentBarTime == this.lastProcessedBarTime)
        {
            this.currentDelta = this.Close(0) - this.currentPriceBase;
            return;
        }

        this.Reset();


        HistoryAggregationTime aggregation = (HistoryAggregationTime)this.HistoricalData.Aggregation;

        if (currentBarTime.ToUniversalTime() < DateTime.UtcNow.AddMinutes(-aggregation.Period.Duration.TotalMinutes)) // real time filter: only last bar
            return;


        this.lastProcessedBarTime = currentBarTime;
        this.ProcessAnalysis(0);

    }

    private async Task FetchMatchesAsync(List<RawCandle> candles)
    {
        try
        {
            var lastCandle = candles[candles.Count - 1];
            uint finalTimestamp = (uint)lastCandle.Timestamp;
            var request = new QueryRequest
            {
                Symbol = this.ticker,
                SessionId = this.effectiveSession,
                Candles = candles,
                TopK = 5,
                Timestamp = finalTimestamp,
                Timeframe = this.GetFormattedTimeframe(),
                SessionOpen = (float)this.currentPriceBase
            };
            
            string json = JsonSerializer.Serialize(request);
            // Log("request timestamp:" + finalTimestamp);
            //Log("request json:" + json);
            var content = new StringContent(json, Encoding.UTF8, "application/json");
            var response = await this.httpClient.PostAsync(this.SearchUrl, content);
            //Log(response.ToString());
            if (response.IsSuccessStatusCode)
            {
                string resJson = await response.Content.ReadAsStringAsync();
                //Log("resJson: " + resJson);
                var fullData = JsonSerializer.Deserialize<MatchesResponse>(resJson);
                
                if (fullData == null) return;
                // Log("Success");
                this.candlesResults = fullData.Matches;
                this.candlesSummary = fullData.Summary;

                this.nextCandle = fullData.NextCandle;
            }
        }
        catch (Exception ex) { Log("Error:"+ ex.Message); }
    }

    private ContextQueryRequest BuildContextQueryRequest()
    {
        int closeHour, closeMinute;

        if (this.effectiveSession == "NY_OPEN")
        {
            closeHour = 17;
            closeMinute = 0;
        }
        else if (this.effectiveSession == "LONDON_OPEN")
        {
            closeHour = 17;
            closeMinute = 0;
        }
        else
        {
            closeHour = 17;
            closeMinute = 0;
        }

        float sessionOpen = (float)this.Open(this.openBarIndex);

        float intradayHigh = sessionOpen;
        float intradayLow = sessionOpen;
        DateTime openBarDate = TimeZoneInfo.ConvertTimeFromUtc(this.Time(this.openBarIndex).ToUniversalTime(), NyTz).Date;

        for (int i = this.openBarIndex + 1; i < Math.Min(this.Count, this.openBarIndex + 72); i++)
        {
            DateTime barNy = TimeZoneInfo.ConvertTimeFromUtc(this.Time(i).ToUniversalTime(), NyTz);
            if (barNy.Date != openBarDate) break;
            if ((float)this.High(i) > intradayHigh) intradayHigh = (float)this.High(i);
            if ((float)this.Low(i) < intradayLow) intradayLow = (float)this.Low(i);
        }

        float prevClose = 0, prevHigh = float.MinValue, prevLow = float.MaxValue;
        bool foundPrevDay = false;

        DateTime currentSessionOpen = new (openBarDate.Year, openBarDate.Month, openBarDate.Day, closeHour, closeMinute, 0);

        DateTime prevSessionEnd = new (PrevTradingDay(currentSessionOpen).Year, PrevTradingDay(currentSessionOpen).Month, PrevTradingDay(currentSessionOpen).Day, closeHour, closeMinute, 0);
        DateTime prevSessionStart = new (PrevTradingDay(prevSessionEnd).Year, PrevTradingDay(prevSessionEnd).Month, PrevTradingDay(prevSessionEnd).Day, closeHour, closeMinute, 0);

        for (int i = this.openBarIndex + 1; i < Math.Min(this.Count, this.openBarIndex + 1000); i++)
        {
            DateTime barNy = TimeZoneInfo.ConvertTimeFromUtc(this.Time(i).ToUniversalTime(), NyTz);

            if (barNy >= prevSessionEnd) continue;   
            if (barNy < prevSessionStart) break;  

            if (!foundPrevDay)
            {
                prevClose = (float)this.Close(i);
                foundPrevDay = true;
            }
            if ((float)this.High(i) > prevHigh) prevHigh = (float)this.High(i);
            if ((float)this.Low(i) < prevLow) prevLow = (float)this.Low(i);
        }

        uint openTimestamp = (uint)new DateTimeOffset(this.Time(this.openBarIndex)).ToUnixTimeSeconds();

        var request = new ContextQueryRequest
        {
            Symbol = this.ticker,
            SessionId = this.effectiveSession,
            SessionOpen = sessionOpen,
            PrevClose = prevClose,
            PrevHigh = prevHigh,
            PrevLow = prevLow,
            IntradayHigh = intradayHigh,
            IntradayLow = intradayLow,
            TopK = 5,
            Timestamp = openTimestamp
        };
        return request;
    }

    private async Task FetchContextMatchesAsync()
    {
        try
        {
            ContextQueryRequest request = this.BuildContextQueryRequest();

            string json = JsonSerializer.Serialize(request);
            var content = new StringContent(json, Encoding.UTF8, "application/json");
            var response = await this.httpClient.PostAsync(this.ContextUrl, content);
            //Log(response.ToString());

            if (response.IsSuccessStatusCode)
            {
                string resJson = await response.Content.ReadAsStringAsync();
                //Log("Context resJson: " + resJson);
                var fullData = JsonSerializer.Deserialize<MatchesResponse>(resJson);
                if (fullData == null) return;
                this.contextResults = fullData.Matches;
                this.contextSummary = fullData.Summary;
            }
        }
        catch (Exception ex) { Log("ContextError: " + ex.Message); }
    }

    public override void OnPaintChart(PaintChartEventArgs args)
    {
        Graphics gr = args.Graphics;
        gr.FillRectangle(this.isPanelVisible ? Brushes.IndianRed : Brushes.SteelBlue, this.toggleRect);
        string label = this.isPanelVisible ? "HIDE PANEL" : "SHOW PANEL";
        
        float titleWidth = gr.MeasureString(label, this.btnfont).Width;
        gr.DrawString(label, this.btnfont, Brushes.White, this.toggleRect.X + (float)Math.Round((this.toggleRect.Width - titleWidth) / 2), this.toggleRect.Y + 7);

        gr.FillRectangle(this.isBacktestMode ? Brushes.Orange : Brushes.DimGray, this.backtestBtnRect);
        string btLabel = this.isBacktestMode ? "GO TO REAL TIME MODE" : " GO TO ANALYSIS MODE";
        float labelWidth = gr.MeasureString(btLabel, this.btnfont).Width;
        gr.DrawString(btLabel, this.btnfont, Brushes.White, this.backtestBtnRect.X + (float)Math.Round((this.backtestBtnRect.Width - labelWidth) / 2), this.backtestBtnRect.Y + 7);

        if (this.isPanelVisible)
        {
            this.DrawLevels(gr);
            this.DrawResultsPanel(gr);
            
        }
    }

    private void DrawLevels(Graphics gr)
    {
        if(!this.DrawSessionLevels && !this.DrawNextCandleLevels) return;

        if(this.currentCandleCount < this.MinCandles) return;
        if (this.DrawSessionLevels)
        {
            var mainWindow = this.CurrentChart.MainWindow;
            double up = this.currentPriceBase + this.candlesSummary.AvgUp;
            double down = this.currentPriceBase - this.candlesSummary.AvgDown;
            int upYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(up));
            int downYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(down));
            int drawBarIndex = Math.Max(0, this.openBarIndex);
            int startXCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartX(this.Time(drawBarIndex)));
            int rightX = mainWindow.ClientRectangle.Right;

            gr.DrawLine(this.upPen, startXCoord, upYCoord, rightX, upYCoord);
            gr.DrawLine(this.downPen, startXCoord, downYCoord, rightX, downYCoord);
        }
        if (this.DrawNextCandleLevels)
        {
            int baseCandleIndex = this.analysisIndex + 1;

            // If a limit is set, cap the index to a maximum distance from the open bar
            //Log("baseCandleIndex: " + baseCandleIndex + " - openBarIndex: " + openBarIndex + " - MaxCandles: " + MaxCandles);
            if (this.MaxCandles > 0 && (this.openBarIndex - baseCandleIndex) >= this.MaxCandles)
            {
                baseCandleIndex = this.openBarIndex - this.MaxCandles + 1;
            }

            int lastCandleIndex = baseCandleIndex;
            //Log("Last Candle Index: " + lastCandleIndex + " - Time:" + Time(lastCandleIndex) +"Close: " + Close(lastCandleIndex));
            var mainWindow = this.CurrentChart.MainWindow;
            double up = this.Close(lastCandleIndex) + this.nextCandle.AvgUp;
            double down = this.Close(lastCandleIndex) - this.nextCandle.AvgDown;
            int upYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(up));
            int downYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(down));
            int drawBarIndex = Math.Max(0, this.openBarIndex);
            int startXCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartX(this.Time(drawBarIndex)));
            int rightX = mainWindow.ClientRectangle.Right;
            using (Pen dashedUpPen = new (Color.Turquoise, 2f))
            {
                dashedUpPen.DashStyle = System.Drawing.Drawing2D.DashStyle.Dash;
                gr.DrawLine(dashedUpPen, startXCoord, upYCoord, rightX, upYCoord);
            }

            using Pen dashedDownPen = new(Color.LightGoldenrodYellow, 2f);
            dashedDownPen.DashStyle = System.Drawing.Drawing2D.DashStyle.Dash;
            gr.DrawLine(dashedDownPen, startXCoord, downYCoord, rightX, downYCoord);

        }
    }

    private void DrawResultsPanel(Graphics gr)
    {
        // ResetLists();
        Rectangle panelRect = new (this.toggleRect.X, this.toggleRect.Y + 35, 330, this.lastPanelHeight);
        gr.FillRectangle(this.panelBgBrush, panelRect);

        int tx = panelRect.X + 12, ty = panelRect.Y + 12;

        string deltaSign = this.currentDelta >= 0 ? "+" : "";
        string deltaText = $" NOW: {deltaSign}{this.currentDelta:F1}";
        Brush deltaBrush = this.currentDelta >= 0 ? Brushes.LimeGreen : Brushes.Red;

        

        string dynamicTitle;
        if(this.effectiveSession != null)
        {
            string candlesInfo = this.currentCandleCount < this.MinCandles ? $"Waiting for {(this.MinCandles > 1 ? this.MinCandles : "FIRST")} candle{(this.MinCandles > 1 ? "s" : "")}" : $"{this.currentCandleCount} candles";
            dynamicTitle = $"{this.effectiveSession} ({candlesInfo})";
            //currentAnalysisText = null;
        }
        else
        {
            dynamicTitle = "NO ACTIVE SESSION";
            this.lastPanelHeight = 100;
            this.currentAnalysisText = null;

        }
        gr.DrawString(dynamicTitle, this.headerFont, Brushes.Orange, tx, ty);
        float titleWidth = gr.MeasureString(dynamicTitle, this.headerFont).Width;
        if (this.candlesResults.Count > 0)
        {
            gr.DrawString(deltaText, this.headerFont, deltaBrush,30+ titleWidth, ty);
        }
        if (this.effectiveSession == null)
        {
            this.lastPanelHeight = ty - panelRect.Y + 30;
            return;
        }
        ty += 32;
        gr.DrawString("CANDLE MATCHES", this.headerFont, Brushes.Orange, tx, ty);
        ty += 18;
        

        if ( this.currentCandleCount < this.MinCandles)
        {
            this.currentAnalysisText = null;
            gr.DrawString($"WAITING FOR {(this.MinCandles > 1 ? this.MinCandles : "FIRST")} CLOSED CANDLE{(this.MinCandles > 1 ? "S" : "")} ", this.dataFont, Brushes.Gray, tx, ty+5);
            this.lastPanelHeight = ty - panelRect.Y + 30;
            ty += 18;
            //return;
        }
        else if (this.candlesResults == null || this.candlesResults.Count == 0)
        {
            if (this.currentAnalysisText != "Loading new Analysis...") this.currentAnalysisText = null;
            gr.DrawString("WAITING FOR DATA...", this.dataFont, Brushes.Gray, tx, ty+5);
            this.lastPanelHeight = ty - panelRect.Y + 30;
            ty += 18;
            //return;
        }
        else
        {
            // SESSION DATA
            Brush edgeBrush = Brushes.White;
            if (this.candlesSummary.EdgeRatio > 2.0 && Math.Abs(this.candlesSummary.Confidence) > 70)
                edgeBrush = (this.candlesSummary.AvgUp > Math.Abs(this.candlesSummary.AvgDown)) ? Brushes.LimeGreen : Brushes.Red;
            else if (this.candlesSummary.EdgeRatio > 1.2)
                edgeBrush = Brushes.Gold;
            Rectangle candleSummaryRect = new (tx, ty, panelRect.Width - 24, 45);
            gr.DrawRectangle(Pens.DimGray, candleSummaryRect);

            string summaryText = $"SESS U:+{this.candlesSummary.AvgUp:F1} | D:-{this.candlesSummary.AvgDown:F1} | ";
            gr.DrawString(summaryText, this.dataFont, Brushes.White, tx + 5, ty + 5);
            float summaryWidth = gr.MeasureString(summaryText, this.dataFont).Width;
            string edgeText = $"E:{(Math.Abs(this.candlesSummary.AvgDown) > this.candlesSummary.AvgUp ? "-" : "+")}{this.candlesSummary.EdgeRatio:F2} ({this.candlesSummary.Confidence:F1}%)";
            gr.DrawString(edgeText, this.dataFont, edgeBrush, tx + 5 + summaryWidth, ty + 5);

            ty += 24;

            // NEXT CANDLE DATA
            edgeBrush = Brushes.White;
            if (this.nextCandle.EdgeRatio > 2.0 && Math.Abs(this.nextCandle.Confidence) > 70)
                edgeBrush = (this.nextCandle.AvgUp > Math.Abs(this.nextCandle.AvgDown)) ? Brushes.LimeGreen : Brushes.Red;
            else if (this.nextCandle.EdgeRatio > 1.2)
                edgeBrush = Brushes.Gold;

            summaryText = $"NXT U:+{this.nextCandle.AvgUp:F1} | D:-{this.nextCandle.AvgDown:F1} | ";
            gr.DrawString(summaryText, this.dataFont, Brushes.White, tx + 5, ty);
            summaryWidth = gr.MeasureString(summaryText, this.dataFont).Width;
            edgeText = $"E:{(Math.Abs(this.nextCandle.AvgDown) > this.nextCandle.AvgUp ? "-" : "+")}{this.nextCandle.EdgeRatio:F2} ({this.nextCandle.Confidence:F1}%)";
            gr.DrawString(edgeText, this.dataFont, edgeBrush, tx + 5 + summaryWidth, ty);

            ty += 30;

            foreach (var m in this.candlesResults)
            {
                var matchRect = new RectangleF(tx, ty, panelRect.Width - 24, 18);
                gr.FillRectangle(this.matchBgBrush, matchRect);
                this.matchClickAreas.Add(matchRect);

                DateTime matchDt = DateTimeOffset.FromUnixTimeSeconds(m.Timestamp).DateTime;
                string dateStr = matchDt.ToString("yyyy-MM-dd");

                this.matchDates.Add(dateStr);

                gr.DrawString($"{dateStr} (MATCH:{m.Similarity:F1}%)", this.dataFont, Brushes.Gray, tx, ty);

                ty += 18;
            }
        }
        

        if (this.contextResults != null && this.contextResults.Count > 0)
        {
             ty += 10;
            gr.DrawString("CONTEXT MATCHES", this.headerFont, Brushes.Orange, tx, ty);
            ty += 18;

            Brush ctxEdgeBrush = Brushes.White;
            if (this.contextSummary.EdgeRatio > 2.0 && Math.Abs(this.contextSummary.Confidence) > 70)
                ctxEdgeBrush = (this.contextSummary.AvgUp > Math.Abs(this.contextSummary.AvgDown)) ? Brushes.LimeGreen : Brushes.Red;
            else if (this.contextSummary.EdgeRatio > 1.2)
                ctxEdgeBrush = Brushes.Gold;

            Rectangle ctxSummaryRect = new (tx, ty, panelRect.Width - 24, 25);
            gr.DrawRectangle(Pens.DimGray, ctxSummaryRect);


            string ctxSummaryText = $"SESS U:+{this.contextSummary.AvgUp:F1} | D:-{this.contextSummary.AvgDown:F1} | ";
            gr.DrawString(ctxSummaryText, this.dataFont, Brushes.LightGray, tx + 5, ty + 5);
            float ctxSummaryWidth = gr.MeasureString(ctxSummaryText, this.dataFont).Width;
            gr.DrawString($"E: {(Math.Abs(this.contextSummary.AvgDown) > this.contextSummary.AvgUp ? "-" : "+")}{this.contextSummary.EdgeRatio:F2} ({this.contextSummary.Confidence:F1}%)", this.dataFont, ctxEdgeBrush, tx + 5 + ctxSummaryWidth, ty + 5);

            ty += 35;

            foreach (var m in this.contextResults)
            {

                var ctxRect = new RectangleF(tx, ty, panelRect.Width - 24, 18);
                gr.FillRectangle(this.matchBgBrush, ctxRect);
                this.contextClickAreas.Add(ctxRect);

                DateTime matchDt = DateTimeOffset.FromUnixTimeSeconds(m.Timestamp).DateTime;
                string dateStr = matchDt.ToString("yyyy-MM-dd");

                this.contextDates.Add(dateStr);

                gr.DrawString($"{dateStr} (MATCH:{m.Similarity:F1}%)", this.dataFont, Brushes.Gray, tx, ty);
                ty += 18;

            }
            
        }
        this.lastPanelHeight = ty - panelRect.Y + 10;
    }

    
    private void OnChartMouseDown(object sender, ChartMouseNativeEventArgs e)
    {
        this.chartMouseDownReceived = true;
        //Log("OnChartMouseDown - setting chartMouseDownReceived: " + chartMouseDownReceived);
    }
    private void OnChartMouseUp(object sender, ChartMouseNativeEventArgs e)
    {
        //Log("OnChartMouseUp - reading chartMouseDownReceived: " + chartMouseDownReceived);
        //Log("OnChartMouseUp - checking chartMouseDownReceived: ");
        if (!this.chartMouseDownReceived) return;  // click orfano, ignoralo
        //Log("OnChartMouseUp - checking checked ");
        this.chartMouseDownReceived = false;

        if (this.toggleRect.Contains(e.Location))
        {
            this.isPanelVisible = !this.isPanelVisible;
            //Log("OnChartMouseUp - Target: toggleRect");
            return;
        }

        if (this.backtestBtnRect.Contains(e.Location))
        {
            this.isBacktestMode = !this.isBacktestMode;
            if (!this.isBacktestMode) this.lastProcessedBarTime = default; // Reset per tornare in real-time
            //Log("OnChartMouseUp - Target: backtestBtnRect");
            return;
        }

        if (this.isPanelVisible)
        {
            for (int i = 0; i < this.matchClickAreas.Count; i++)
            {
                if (this.matchClickAreas[i].Contains(e.Location))
                {
                    string date = this.matchDates[i];
                    string url = $"http://127.0.0.1:9999/visualize?symbol={this.ticker}&date={date}&timeframe={this.GetFormattedTimeframe()}";
                    
                    try
                    {
                        System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                        {
                            FileName = url,
                            UseShellExecute = true
                        });
                        //Log("OnChartMouseUp - Target: Candle match");
                        return;
                    }
                    catch (Exception ex)
                    {
                        Log("Errore apertura browser: " + ex.Message);
                    }

                    break; 
                }
            }
            for (int i = 0; i < this.contextClickAreas.Count; i++)
            {
                if (this.contextClickAreas[i].Contains(e.Location))
                {
                    string date = this.contextDates[i];
                    string url = $"http://127.0.0.1:9999/visualize?symbol={this.ticker}&date={date}";
                    try { 
                        System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo { FileName = url, UseShellExecute = true });
                        //Log("OnChartMouseUp - Target: Context match");
                        return; 
                    }
                    catch (Exception ex) { Log("Errore apertura browser: " + ex.Message); }
                    break;
                }
            }

        }

        if (this.isBacktestMode)
        {
            //Log("OnChartMouseUp - Target: Backtest");
            DateTime clickTime = e.Window.CoordinatesConverter.GetTime(e.X);
            int candleIndex = this.GetIndexByTime(clickTime);
            this.RunManualAnalysis(candleIndex);
        }
    }

    protected void RunManualAnalysis(int candleIndex)
    {
        // Pulizia visiva prima del nuovo calcolo
        this.Reset();
        // Esegui l'analisi sulla candela cliccata
        this.ProcessAnalysis(candleIndex);
    }

    private void ProcessAnalysis(int refIndex)
    {
        this.analysisIndex = refIndex;
        //Log("ProcessAnalysis called with refIndex: " + refIndex);
        DateTime referenceTimeUtc = this.Time(refIndex).ToUniversalTime();


        DateTime londonNow = TimeZoneInfo.ConvertTimeFromUtc(referenceTimeUtc, LondonTz);
        DateTime nyNow = TimeZoneInfo.ConvertTimeFromUtc(referenceTimeUtc, NyTz);

        string sessionToUse = this.SessionId;
        if (this.SessionId == "AUTO")
        {
            int londonTime = londonNow.Hour * 100 + londonNow.Minute;
            int nyTime = nyNow.Hour * 100 + nyNow.Minute;

            if (londonTime >= 800 && londonTime < 1425 && nyTime < 930) sessionToUse = "LONDON_OPEN";
            else if (nyTime >= 930 && nyTime < 1705) sessionToUse = "NY_OPEN";
            else sessionToUse = this.effectiveSession = null;
        }

        if (sessionToUse == null) return;

        // --- LOGICA TARGET TIME ---
        int targetHour = (sessionToUse == "NY_OPEN") ? 9 : 8;
        int targetMinute = (sessionToUse == "NY_OPEN") ? 30 : 0;
        TimeZoneInfo sessionTz = (sessionToUse == "NY_OPEN") ? NyTz : LondonTz;

        // --- RICERCA OPEN BAR ---
        // Cerchiamo l'apertura andando a ritroso partendo da refIndex
        this.openBarIndex = -1;
        for (int i = refIndex; i < Math.Min(this.Count, refIndex + 200); i++) // Aumentato a 200 per sicurezza
        {
            DateTime barInSessionTime = TimeZoneInfo.ConvertTimeFromUtc(this.Time(i).ToUniversalTime(), sessionTz);
            if (barInSessionTime.Hour == targetHour && barInSessionTime.Minute == targetMinute)
            {
                this.openBarIndex = i;
                break;
            }
        }
        //Log("openBarIndex: "+ openBarIndex);

        if (this.openBarIndex == -1) return;
        else if (this.openBarIndex >= this.MaxCandles && this.candlesResults.Count > 0) return;

        if (this.openBarIndex != -1)
        {
            DateTime foundBarTime = TimeZoneInfo.ConvertTimeFromUtc(this.Time(this.openBarIndex).ToUniversalTime(), sessionTz);

            if (foundBarTime.Hour == 9 && foundBarTime.Minute == 30)
                this.effectiveSession = "NY_OPEN";
            else if (foundBarTime.Hour == 8 && foundBarTime.Minute == 0)
                this.effectiveSession = "LONDON_OPEN";
        }
        //Log("effectiveSession: " + this.effectiveSession);
        // --- FETCH CONTEXT ---
        string sessionDateKey = $"{sessionToUse}_{this.Time(this.openBarIndex):yyyy-MM-dd}";
        //Log("sessionDateKey: " + sessionDateKey+ " - lastContextSessionDate: "+ this.lastContextSessionDate);
        if (sessionDateKey != this.lastContextSessionDate)
        {
            this.lastContextSessionDate = sessionDateKey;
            //Log("FetchContextMatchesAsync");
            _ = this.FetchContextMatchesAsync();
        }
        if ((this.openBarIndex - refIndex) < this.MinCandles) return;
        // --- PREPARAZIONE CANDLES (Usa refIndex come limite superiore) ---
        var candlesToInquire = new List<RawCandle>();
        int limit = Math.Min(this.openBarIndex - refIndex, this.MaxCandles);

        // Ciclo che parte dall'apertura e arriva fino alla candela di riferimento
        for (int i = this.openBarIndex; i > refIndex && i > (this.openBarIndex - limit); i--)
        {
            DateTime bTime = TimeZoneInfo.ConvertTimeFromUtc(this.Time(i).ToUniversalTime(), NyTz);
            candlesToInquire.Add(new RawCandle
            {
                Timestamp = (uint)((DateTimeOffset)bTime).ToUnixTimeSeconds(),
                Open = (float)this.Open(i),
                High = (float)this.High(i),
                Low = (float)this.Low(i),
                Close = (float)this.Close(i),
                Volume = (float)this.Volume(i)
            });
        }

        this.currentPriceBase = (float)this.Open(this.openBarIndex);
        this.currentDelta = (float)this.Close(refIndex) - this.currentPriceBase;
        this.currentCandleCount = candlesToInquire.Count;
        _ = this.FetchMatchesAsync(candlesToInquire);
    }

    private string GetFormattedTimeframe()
    {
        HistoryAggregationTime aggregation = (HistoryAggregationTime)this.HistoricalData.Aggregation;


        // Trasforma "5m" in "M5", "15m" in "M15", "1h" in "H1", ecc.
        string tf = aggregation.Period.Format();

        if (tf.EndsWith('m'))
            return "M" + tf.Replace("m", "");

        if (tf.EndsWith('h'))
            return "H" + tf.Replace("h", "");

        return tf; // Fallback se è già nel formato corretto o ignoto
    }

    public override void Dispose()
    {
        //Log("ondispose");
        this.CurrentChart.MouseUp -= this.OnChartMouseUp;
        this.CurrentChart.MouseDown -= this.OnChartMouseDown;
        this.alertSoundPlayer?.Dispose();
        GC.SuppressFinalize(this);
    }

    protected void InitTicker()
    {
        string symbol = this.Symbol.Description.ToLower();
        if (symbol.Contains("nasdaq"))
        {
            this.ticker = "NQ";
        }
        else if (symbol.Contains("s&p"))
        {
            this.ticker = "ES";
        }
        else if (symbol.Contains("gold"))
        {
            this.ticker = "GC";
        }
        else if (symbol.Contains("dow"))
        {
            this.ticker = "YM";
        }
    }


    private int GetIndexByTime(DateTime clickedTime)
    {
        // 'Count' è il numero totale di candele sul grafico
        for (int i = 0; i < this.Count; i++)
        {
            // Time(i) restituisce l'orario della candela all'indice i
            if (this.Time(i) <= clickedTime)
            {
                return Math.Max(0, i-1);
            }
        }
        return -1;
    }
    private static DateTime PrevTradingDay(DateTime date)
    {
        DateTime prev = date.AddDays(-1);
        while (prev.DayOfWeek == DayOfWeek.Saturday || prev.DayOfWeek == DayOfWeek.Sunday)
            prev = prev.AddDays(-1);
        return prev;
    }

    public static void Log(string m){
        
        Core.Instance.Loggers.Log($"[RetroVis] {m}", LoggingLevel.Trading);
    } 

    #region Models
    

    public class RawCandle
    {
        [JsonPropertyName("timestamp")] public uint Timestamp { get; set; }
        [JsonPropertyName("open")] public float Open { get; set; }
        [JsonPropertyName("high")] public float High { get; set; }
        [JsonPropertyName("low")] public float Low { get; set; }
        [JsonPropertyName("close")] public float Close { get; set; }
        [JsonPropertyName("_volume")] public float Volume { get; set; }
    }

    public class MatchesResponse
    {
        [JsonPropertyName("summary")] public AggregateData Summary { get; set; }
        [JsonPropertyName("matches")] public List<MatchResponse> Matches { get; set; }
        [JsonPropertyName("next_candle")] public NextCandleData NextCandle { get; set; }
    }

    public class AggregateData
    {
        [JsonPropertyName("avg_up")] public float AvgUp { get; set; }
        [JsonPropertyName("avg_down")] public float AvgDown { get; set; }
        [JsonPropertyName("avg_eos")] public float AvgEos { get; set; }

        [JsonPropertyName("avg_eoh")] public float AvgEoh { get; set; }

        [JsonPropertyName("edge_ratio")] public float EdgeRatio { get; set; }

        [JsonPropertyName("bias_pct")] public float BiasPct { get; set; }

        [JsonPropertyName("confidence")] public float Confidence { get; set; }
    }

    public class NextCandleData
    {
        [JsonPropertyName("avg_up")] public float AvgUp { get; set; }
        [JsonPropertyName("avg_down")] public float AvgDown { get; set; }
        [JsonPropertyName("edge_ratio")] public float EdgeRatio { get; set; }
        [JsonPropertyName("bias_pct")] public float BiasPct { get; set; }
        [JsonPropertyName("confidence")] public float Confidence { get; set; }
    }
    public class MatchResponse
    {
        [JsonPropertyName("timestamp")] public uint Timestamp { get; set; }
        [JsonPropertyName("similarity")] public float Similarity { get; set; } 
        [JsonPropertyName("up_pts")] public float UpPts { get; set; }           
        [JsonPropertyName("down_pts")] public float DownPts { get; set; }      
        [JsonPropertyName("eos_pts")] public float EosPts { get; set; }

        [JsonPropertyName("eoh_pts")] public float EohPts { get; set; }
        [JsonPropertyName("next_candle_up")] public float NextCandleUp { get; set; }

        [JsonPropertyName("next_candle_down")] public float NextCandleDown { get; set; }
    }
    public class QueryRequest
    {
        [JsonPropertyName("symbol")] public string Symbol { get; set; }
        [JsonPropertyName("session_id")] public string SessionId { get; set; }
        [JsonPropertyName("candles")] public List<RawCandle> Candles { get; set; }
        [JsonPropertyName("top_k")] public int TopK { get; set; }
        [JsonPropertyName("timestamp")] public uint Timestamp { get; set; }
        [JsonPropertyName("timeframe")] public string Timeframe { get; set; }
        [JsonPropertyName("session_open")] public float SessionOpen { get; set; }
    }
    public class ContextQueryRequest
    {
        [JsonPropertyName("symbol")] public string Symbol { get; set; }
        [JsonPropertyName("session_id")] public string SessionId { get; set; }
        [JsonPropertyName("session_open")] public float SessionOpen { get; set; }
        [JsonPropertyName("prev_close")] public float PrevClose { get; set; }
        [JsonPropertyName("prev_high")] public float PrevHigh { get; set; }
        [JsonPropertyName("prev_low")] public float PrevLow { get; set; }
        [JsonPropertyName("intraday_high")] public float IntradayHigh { get; set; }
        [JsonPropertyName("intraday_low")] public float IntradayLow { get; set; }
        [JsonPropertyName("top_k")] public int TopK { get; set; }
        [JsonPropertyName("timestamp")] public uint Timestamp { get; set; }
    }

    #endregion
}