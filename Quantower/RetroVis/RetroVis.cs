using System;
using System.Collections.Generic;
using System.ComponentModel.DataAnnotations;
using System.Drawing;
using System.Drawing.Printing;
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

[SupportedOSPlatform("windows")]

public class RetroVis : Indicator
{
    #region Parameters
    [InputParameter("Session Type", 0, variants: new object[] { "AUTO", "AUTO", "NY_OPEN", "NY_OPEN", "LONDON_OPEN", "LONDON_OPEN" })]
    public string SessionId = "AUTO";

    [InputParameter("Min Candles for Pattern", 1, 1, 15, 1)]
    public int MinCandles = 1;

    [InputParameter("Max Candles for Pattern", 2, 1, 20, 1)]
    public int MaxCandles = 12;

    [InputParameter("Webserver URL", 3)]
    public string ServerUrl = "http://127.0.0.1:9999";

    [InputParameter("Enable AI Analysis",4)]
    public bool EnableAiAnalysis { get; set; } = false;

    [InputParameter("Draw Session Levels", 5)]
    public bool DrawSessionLevels { get; set; } = true;

    [InputParameter("Draw Next Candle Levels", 5)]
    public bool DrawNextCandleLevels { get; set; } = true;

    #endregion

    #region Internal Fields
    private readonly HttpClient httpClient = new HttpClient();
    private AggregateData candlesSummary = new AggregateData();
    private NextCandleData nextCandle = new NextCandleData();
    private List<MatchResponse> candlesResults = new List<MatchResponse>();
    private bool isPanelVisible = false;
    private Rectangle toggleRect = new Rectangle(5, 80, 120, 25);
    private int openBarIndex = -1;
    private double currentPriceBase = 0;
    private DateTime lastProcessedBarTime;
    
    private int currentCandleCount = 0;
    private string ticker = "";
    private string effectiveSession;

    private static TimeZoneInfo NyTz ;
    private static TimeZoneInfo LondonTz;

    double currentDelta = 0;

    private List<RectangleF> matchClickAreas = new List<RectangleF>();
    private List<string> matchDates = new List<string>();

    public string SearchUrl { get; set; }
    public string ContextUrl { get; set; }
    private AggregateData contextSummary = new AggregateData();
    private List<MatchResponse> contextResults = new List<MatchResponse>();
    private string lastContextSessionDate = ""; // traccia se il contesto è già stato caricato per questa apertura

    private List<RectangleF> contextClickAreas = new List<RectangleF>();
    private List<string> contextDates = new List<string>();

    private int lastPanelHeight = 0;

    private bool isBacktestMode = false;
    private Rectangle backtestBtnRect = new Rectangle(130, 80, 130, 25); // Accanto al primo bottone

    private string currentAnalysisText;
    
    private Font dataFont = new Font("Consolas", 9);
    private Font btnfont = new Font("Arial", 7, FontStyle.Bold);
    private Font headerFont = new Font("Consolas", 11, FontStyle.Bold);
    private Font bodyFont = new Font("Consolas", 10);
    private SolidBrush panelBgBrush = new SolidBrush(Color.FromArgb(220, 10, 10, 10));
    private SolidBrush ctxBgBrush = new SolidBrush(Color.FromArgb(0, 180, 100, 30));
    private Pen upPen = new Pen(Color.LimeGreen, 2);
    private Pen downPen = new Pen(Color.Sienna, 2);


    private float aiBoxHeight = 100f;

    private bool chartMouseDownReceived = false;

    private int analysisIndex;
    #endregion

    public RetroVis() : base()
    {
        Name = "RetroVis";
        SeparateWindow = false;
    }
    protected override void OnInit()
    {
        httpClient.Timeout = TimeSpan.FromSeconds(60);

        this.SearchUrl = $"{ServerUrl.TrimEnd('/')}/search";
        this.ContextUrl = $"{ServerUrl.TrimEnd('/')}/context-search";
        try
        {
            NyTz = TimeZoneInfo.FindSystemTimeZoneById("Eastern Standard Time");
            LondonTz = TimeZoneInfo.FindSystemTimeZoneById("GMT Standard Time");
        }
        catch (Exception ex)
        {
            Log("CRITICAL: Timezone non trovata! " + ex.Message);
        }

        lastProcessedBarTime = DateTime.MinValue;
        if (this.CurrentChart != null)
        {
            this.CurrentChart.MouseUp -= OnChartMouseUp;
            this.CurrentChart.MouseUp += OnChartMouseUp;

            this.CurrentChart.MouseDown -= OnChartMouseDown;
            this.CurrentChart.MouseDown += OnChartMouseDown;
        }
            

        

        initTicker();
        
    }

    protected void ResetLists()
    {
        matchClickAreas.Clear();
        matchDates.Clear();
        contextClickAreas.Clear();
        contextDates.Clear();
    }

    protected void Reset()
    {
        this.lastProcessedBarTime = default(DateTime);
        candlesResults.Clear();
        contextResults.Clear();
        ResetLists();
        this.currentCandleCount = 0;
        lastContextSessionDate = "";
        currentAnalysisText = "Loading new Analysis...";
    }

    protected override void OnSettingsUpdated()
    {
        base.OnSettingsUpdated();
        Reset();
    }

    protected override void OnUpdate(UpdateArgs args)
    {
        if (isBacktestMode || !isPanelVisible) return;
        currentDelta = Close(0) - currentPriceBase;

        DateTime currentBarTime = Time();
        if (currentBarTime == lastProcessedBarTime)
        {
            currentDelta = Close(0) - currentPriceBase;
            return;
        }

        Reset();


        HistoryAggregationTime aggregation = (HistoryAggregationTime)this.HistoricalData.Aggregation;

        if (currentBarTime.ToUniversalTime() < DateTime.UtcNow.AddMinutes(-aggregation.Period.Duration.TotalMinutes)) // real time filter: only last bar
            return;


        lastProcessedBarTime = currentBarTime;
        ProcessAnalysis(0);

    }

    private async Task FetchMatchesAsync(List<RawCandle> candles)
    {
        try
        {
            var lastCandle = candles[candles.Count - 1];
            uint finalTimestamp = (uint)lastCandle.Timestamp;
            var request = new QueryRequest
            {
                Symbol = ticker,
                SessionId = effectiveSession,
                Candles = candles,
                TopK = 5,
                Timestamp = finalTimestamp,
                Timeframe = GetFormattedTimeframe(),
                SessionOpen = (float)currentPriceBase
            };
            
            string json = JsonSerializer.Serialize(request);
            // Log("request timestamp:" + finalTimestamp);
            //Log("request json:" + json);
            var content = new StringContent(json, Encoding.UTF8, "application/json");
            var response = await httpClient.PostAsync(SearchUrl, content);
            //Log(response.ToString());
            if (response.IsSuccessStatusCode)
            {
                string resJson = await response.Content.ReadAsStringAsync();
                var fullData = JsonSerializer.Deserialize<MatchesResponse>(resJson);
                if (fullData == null) return;
                // Log("Success");
                candlesResults = fullData.Matches;
                candlesSummary = fullData.Summary;
                nextCandle = fullData.NextCandle;
                _ =FetchAiAnalysis(candles);
            }
        }
        catch (Exception ex) { Log("Error:"+ ex.Message); }
    }

    private async Task FetchAiAnalysis(List<RawCandle> candles)
    {
        if(!EnableAiAnalysis) return;
        try
        {
            var lastCandle = candles[candles.Count - 1];
            uint finalTimestamp = (uint)lastCandle.Timestamp;
            ContextQueryRequest contextData = BuildContextQueryRequest();
            var request = new AiAnalysisRequest
            {

                Symbol = ticker,
                SessionId = effectiveSession,
                Candles = candles,
                Timeframe = GetFormattedTimeframe(),
                Timestamp = finalTimestamp,
                CurrentPriceAction = lastCandle.Close,
                SessionOpen = contextData.SessionOpen,
                PrevClose = contextData.PrevClose,
                PrevHigh = contextData.PrevHigh,
                PrevLow = contextData.PrevLow,
                IntradayHigh = contextData.IntradayHigh,
                IntradayLow = contextData.IntradayLow,
                TopK = contextData.TopK
            };
            //Log("CurrentPriceAction: " + request.CurrentPriceAction);
            string json = JsonSerializer.Serialize(request);
            //Log("AI json:" + json);
            var content = new StringContent(json, Encoding.UTF8, "application/json");
            var response = await httpClient.PostAsync($"{ServerUrl.TrimEnd('/')}/get-ai-analysis", content);
            if (response.IsSuccessStatusCode)
            {
                var result = await response.Content.ReadFromJsonAsync<AiResponse>();

                if (result != null && result.status == "success")
                {
                    this.currentAnalysisText = result.analysis;
                }
            }
        }
        catch (Exception ex) { Log("AI Analysis Error: " + ex.Message); }
    }

    private ContextQueryRequest BuildContextQueryRequest()
    {
        int closeHour, closeMinute;
        TimeZoneInfo sessionTz;

        if (effectiveSession == "NY_OPEN")
        {
            closeHour = 17;
            closeMinute = 0;
            sessionTz = NyTz;
        }
        else if (effectiveSession == "LONDON_OPEN")
        {
            closeHour = 17;
            closeMinute = 0;
            sessionTz = LondonTz;
        }
        else
        {
            closeHour = 17;
            closeMinute = 0;
            sessionTz = NyTz;
        }

        float sessionOpen = (float)Open(openBarIndex);

        float intradayHigh = sessionOpen;
        float intradayLow = sessionOpen;
        DateTime openBarDate = TimeZoneInfo.ConvertTimeFromUtc(Time(openBarIndex).ToUniversalTime(), NyTz).Date;

        for (int i = openBarIndex + 1; i < Math.Min(Count, openBarIndex + 72); i++)
        {
            DateTime barNy = TimeZoneInfo.ConvertTimeFromUtc(Time(i).ToUniversalTime(), NyTz);
            if (barNy.Date != openBarDate) break;
            if ((float)High(i) > intradayHigh) intradayHigh = (float)High(i);
            if ((float)Low(i) < intradayLow) intradayLow = (float)Low(i);
        }

        float prevClose = 0, prevHigh = float.MinValue, prevLow = float.MaxValue;
        bool foundPrevDay = false;

        DateTime currentSessionOpen = new DateTime(openBarDate.Year, openBarDate.Month, openBarDate.Day, closeHour, closeMinute, 0);

        DateTime prevSessionEnd = new DateTime(PrevTradingDay(currentSessionOpen).Year, PrevTradingDay(currentSessionOpen).Month, PrevTradingDay(currentSessionOpen).Day, closeHour, closeMinute, 0);
        DateTime prevSessionStart = new DateTime(PrevTradingDay(prevSessionEnd).Year, PrevTradingDay(prevSessionEnd).Month, PrevTradingDay(prevSessionEnd).Day, closeHour, closeMinute, 0);

        for (int i = openBarIndex + 1; i < Math.Min(Count, openBarIndex + 1000); i++)
        {
            DateTime barNy = TimeZoneInfo.ConvertTimeFromUtc(Time(i).ToUniversalTime(), NyTz);

            if (barNy >= prevSessionEnd) continue;   
            if (barNy < prevSessionStart) break;  

            if (!foundPrevDay)
            {
                prevClose = (float)Close(i);
                foundPrevDay = true;
            }
            if ((float)High(i) > prevHigh) prevHigh = (float)High(i);
            if ((float)Low(i) < prevLow) prevLow = (float)Low(i);
        }

        uint openTimestamp = (uint)new DateTimeOffset(Time(openBarIndex)).ToUnixTimeSeconds();

        var request = new ContextQueryRequest
        {
            Symbol = ticker,
            SessionId = effectiveSession,
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

    private async Task FetchContextMatchesAsync(int openBarIndex)
    {
        try
        {
            ContextQueryRequest request = BuildContextQueryRequest();

            string json = JsonSerializer.Serialize(request);
            var content = new StringContent(json, Encoding.UTF8, "application/json");
            var response = await httpClient.PostAsync(ContextUrl, content);
            //Log(response.ToString());

            if (response.IsSuccessStatusCode)
            {
                string resJson = await response.Content.ReadAsStringAsync();

                var fullData = JsonSerializer.Deserialize<MatchesResponse>(resJson);
                if (fullData == null) return;
                contextResults = fullData.Matches;
                contextSummary = fullData.Summary;
            }
        }
        catch (Exception ex) { Log("ContextError: " + ex.Message); }
    }

    public override void OnPaintChart(PaintChartEventArgs args)
    {
        Graphics gr = args.Graphics;
        gr.FillRectangle(isPanelVisible ? Brushes.IndianRed : Brushes.SteelBlue, toggleRect);
        string label = isPanelVisible ? "HIDE PANEL" : "SHOW PANEL";
        
        float titleWidth = gr.MeasureString(label, btnfont).Width;
        gr.DrawString(label, btnfont, Brushes.White, toggleRect.X + (float)Math.Round((toggleRect.Width - titleWidth) / 2), toggleRect.Y + 7);

        gr.FillRectangle(isBacktestMode ? Brushes.Orange : Brushes.DimGray, backtestBtnRect);
        string btLabel = isBacktestMode ? "GO TO REAL TIME MODE" : " GO TO ANALYSIS MODE";
        float labelWidth = gr.MeasureString(btLabel, btnfont).Width;
        gr.DrawString(btLabel, btnfont, Brushes.White, backtestBtnRect.X + (float)Math.Round((backtestBtnRect.Width - labelWidth) / 2), backtestBtnRect.Y + 7);

        if (isPanelVisible)
        {
            DrawLevels(gr);
            DrawResultsPanel(gr);
            
            if (EnableAiAnalysis)
                DrawAiPanel(gr);
            
        }
    }

    private void DrawLevels(Graphics gr)
    {
        if(!DrawSessionLevels && !DrawNextCandleLevels) return;

        if(this.currentCandleCount < this.MinCandles) return;
        if (DrawSessionLevels)
        {
            var mainWindow = this.CurrentChart.MainWindow;
            double up = currentPriceBase + candlesSummary.AvgUp;
            double down = currentPriceBase - candlesSummary.AvgDown;
            int upYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(up));
            int downYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(down));
            int drawBarIndex = Math.Max(0, openBarIndex);
            int startXCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartX(Time(drawBarIndex)));
            int rightX = mainWindow.ClientRectangle.Right;

            gr.DrawLine(upPen, startXCoord, upYCoord, rightX, upYCoord);
            gr.DrawLine(downPen, startXCoord, downYCoord, rightX, downYCoord);
        }
        if (DrawNextCandleLevels)
        {
            //Log("Clicked Candle Index: " + (analysisIndex +1)+ " - Time:" + Time(analysisIndex+1) +"Close: " + Close(analysisIndex+1));
            var mainWindow = this.CurrentChart.MainWindow;
            double up = Close(analysisIndex+1) + nextCandle.AvgUp;
            double down = Close(analysisIndex+1) - nextCandle.AvgDown;
            int upYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(up));
            int downYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(down));
            int drawBarIndex = Math.Max(0, openBarIndex);
            int startXCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartX(Time(drawBarIndex)));
            int rightX = mainWindow.ClientRectangle.Right;
            using (Pen dashedUpPen = new Pen(Color.Turquoise, 2f))
            {
                dashedUpPen.DashStyle = System.Drawing.Drawing2D.DashStyle.Dash;
                gr.DrawLine(dashedUpPen, startXCoord, upYCoord, rightX, upYCoord);
            }
            using (Pen dashedDownPen = new Pen(Color.LightGoldenrodYellow, 2f))
            {
                dashedDownPen.DashStyle = System.Drawing.Drawing2D.DashStyle.Dash;
                gr.DrawLine(dashedDownPen, startXCoord, downYCoord, rightX, downYCoord);
            }
        }
    }

    private void DrawResultsPanel(Graphics gr)
    {
        // ResetLists();
        Rectangle panelRect = new Rectangle(toggleRect.X, toggleRect.Y + 35, 330, lastPanelHeight);
        gr.FillRectangle(panelBgBrush, panelRect);

        int tx = panelRect.X + 12, ty = panelRect.Y + 12;

        string deltaSign = currentDelta >= 0 ? "+" : "";
        string deltaText = $" NOW: {deltaSign}{currentDelta:F1}";
        Brush deltaBrush = currentDelta >= 0 ? Brushes.LimeGreen : Brushes.Red;

        

        string dynamicTitle = "";
        if(effectiveSession != null)
        {
            string candlesInfo = this.currentCandleCount < this.MinCandles ? $"Waiting for {(this.MinCandles > 1 ? this.MinCandles : "FIRST")} candle{(this.MinCandles > 1 ? "s" : "")}" : $"{this.currentCandleCount} candles";
            dynamicTitle = $"{effectiveSession} ({candlesInfo})";
            //currentAnalysisText = null;
        }
        else
        {
            dynamicTitle = "NO ACTIVE SESSION";
            lastPanelHeight = 100;
            currentAnalysisText = null;

        }
        gr.DrawString(dynamicTitle, headerFont, Brushes.Orange, tx, ty);
        float titleWidth = gr.MeasureString(dynamicTitle, headerFont).Width;
        if (candlesResults.Count > 0)
        {
            gr.DrawString(deltaText, headerFont, deltaBrush,30+ titleWidth, ty);
        }
        if (effectiveSession == null)
        {
            lastPanelHeight = ty - panelRect.Y + 30;
            return;
        }
        ty += 32;
        gr.DrawString("CANDLE MATCHES", headerFont, Brushes.Orange, tx, ty);
        ty += 18;
        

        if ( this.currentCandleCount < this.MinCandles)
        {
            currentAnalysisText = null;
            gr.DrawString($"WAITING FOR {(this.MinCandles > 1 ? this.MinCandles : "FIRST")} CLOSED CANDLE{(this.MinCandles > 1 ? "S" : "")} ", dataFont, Brushes.Gray, tx, ty+5);
            lastPanelHeight = ty - panelRect.Y + 30;
            ty += 18;
            //return;
        }
        else if (candlesResults == null || candlesResults.Count == 0)
        {
            if (currentAnalysisText != "Loading new Analysis...") currentAnalysisText = null;
            gr.DrawString("WAITING FOR DATA...", dataFont, Brushes.Gray, tx, ty+5);
            lastPanelHeight = ty - panelRect.Y + 30;
            ty += 18;
            //return;
        }
        else
        {
            // SESSION DATA
            Brush edgeBrush = Brushes.White;
            if (candlesSummary.EdgeRatio > 2.0)
                edgeBrush = (candlesSummary.AvgUp > Math.Abs(candlesSummary.AvgDown)) ? Brushes.LimeGreen : Brushes.Red;
            else if (candlesSummary.EdgeRatio > 1.2)
                edgeBrush = Brushes.Gold;
            Rectangle candleSummaryRect = new Rectangle(tx, ty, panelRect.Width - 24, 45);
            gr.DrawRectangle(Pens.DimGray, candleSummaryRect);

            string summaryText = $"SESS UP:+{candlesSummary.AvgUp:F1} | DOWN:-{candlesSummary.AvgDown:F1} | ";
            gr.DrawString(summaryText, dataFont, Brushes.White, tx + 5, ty + 5);
            float summaryWidth = gr.MeasureString(summaryText, dataFont).Width;
            string edgeText = $"EDGE:{(Math.Abs(candlesSummary.AvgDown) > candlesSummary.AvgUp ? "-" : "+")}{candlesSummary.EdgeRatio:F2}";
            gr.DrawString(edgeText, dataFont, edgeBrush, tx + 5 + summaryWidth, ty + 5);

            ty += 24;

            // NEXT CANDLE DATA


            edgeBrush = Brushes.White;
            if (nextCandle.EdgeRatio > 2.0)
                edgeBrush = (nextCandle.AvgUp > Math.Abs(nextCandle.AvgDown)) ? Brushes.LimeGreen : Brushes.Red;
            else if (nextCandle.EdgeRatio > 1.2)
                edgeBrush = Brushes.Gold;

            summaryText = $"NXT UP:+{nextCandle.AvgUp:F1} | DOWN:-{nextCandle.AvgDown:F1} | ";
            gr.DrawString(summaryText, dataFont, Brushes.White, tx + 5, ty);
            summaryWidth = gr.MeasureString(summaryText, dataFont).Width;
            edgeText = $"EDGE:{(Math.Abs(nextCandle.AvgDown) > nextCandle.AvgUp ? "-" : "+")}{nextCandle.EdgeRatio:F2}";
            gr.DrawString(edgeText, dataFont, edgeBrush, tx + 5 + summaryWidth, ty);

            ty += 30;

            foreach (var m in candlesResults)
            {
                matchClickAreas.Add(new RectangleF(tx, ty, panelRect.Width - 30, 50));

                DateTime matchDt = DateTimeOffset.FromUnixTimeSeconds(m.Timestamp).DateTime;
                string dateStr = matchDt.ToString("yyyy-MM-dd");

                matchDates.Add(dateStr);

                gr.DrawString($"{dateStr} (MATCH:{m.Similarity:F1}%)", dataFont, Brushes.Gray, tx, ty);
                ty += 15;

                string vals = $"SESS UP: {m.UpPts:+0.0} | DOWN: {m.DownPts:0.0}";
                gr.DrawString(vals, dataFont, Brushes.White, tx + 10, ty);
                ty += 15;
                vals = $"NXT UP: {m.NextCandleUp:+0.0;-0.0} | DOWN: {m.NextCandleDown:+0.0;-0.0}";
                gr.DrawString(vals, dataFont, Brushes.White, tx + 10, ty);

                ty += 25;
            }
        }
        

        if (contextResults != null && contextResults.Count > 0)
        {
             ty += 10;
            gr.DrawString("CONTEXT MATCHES", headerFont, Brushes.Orange, tx, ty);
            ty += 18;

            Brush ctxEdgeBrush = Brushes.White;
            if (contextSummary.EdgeRatio > 2.0)
                ctxEdgeBrush = (contextSummary.AvgUp > Math.Abs(contextSummary.AvgDown)) ? Brushes.LimeGreen : Brushes.Red;
            else if (contextSummary.EdgeRatio > 1.2)
                ctxEdgeBrush = Brushes.Gold;

            Rectangle ctxSummaryRect = new Rectangle(tx, ty, panelRect.Width - 24, 25);
            gr.DrawRectangle(Pens.DimGray, ctxSummaryRect);


            string ctxSummaryText = $"SESS UP:+{contextSummary.AvgUp:F1} | DOWN:-{contextSummary.AvgDown:F1} | ";
            gr.DrawString(ctxSummaryText, dataFont, Brushes.LightGray, tx + 5, ty + 5);
            float ctxSummaryWidth = gr.MeasureString(ctxSummaryText, dataFont).Width;
            gr.DrawString($"EDGE: {(Math.Abs(contextSummary.AvgDown) > contextSummary.AvgUp ? "-" : "+")}{contextSummary.EdgeRatio:F2}", dataFont, ctxEdgeBrush, tx + 5 + ctxSummaryWidth, ty + 5);

            ty += 35;

            foreach (var m in contextResults)
            {

                var ctxRect = new RectangleF(tx, ty, panelRect.Width - 24, 32);
                gr.FillRectangle(ctxBgBrush, ctxRect);
                contextClickAreas.Add(ctxRect);

                DateTime matchDt = DateTimeOffset.FromUnixTimeSeconds(m.Timestamp).DateTime;
                string dateStr = matchDt.ToString("yyyy-MM-dd");

                contextDates.Add(dateStr);

                gr.DrawString($"{dateStr} (MATCH:{m.Similarity:F1}%)", dataFont, Brushes.Gray, tx, ty);
                ty += 15;

                string vals = $"UP: {m.UpPts:+0.0}  DOWN: {m.DownPts:0.0}";
                gr.DrawString(vals, dataFont, Brushes.White, tx + 10, ty);
                ty += 25;
            }
            
        }
        lastPanelHeight = ty - panelRect.Y + 10;
    }

    private void DrawAiPanel(Graphics gr)
    {
        if (string.IsNullOrEmpty(this.currentAnalysisText)) return;

        string cleanedAnalysis = this.currentAnalysisText
            .Replace("###", "")         // Rimuovi il Markdown     
            .Replace("**", "")         // Rimuovi il Markdown
            .Replace("\n\n", "\n")     // Rimpiazza i doppi a capo con a capo singolo
            .Trim();                   // Rimuovi spazi extra
        

        // --- CONFIGURAZIONE ---
        float x = 340;
        float y = toggleRect.Y + 35; // Punto di partenza sotto i tuoi moduli
        float width = 450; // Larghezza fissa del blocco
        float padding = 12;

        // --- 1. CALCOLO ALTEZZA NECESSARIA ---
        // Puliamo il markdown come prima per avere misure reali
        string cleanText = this.currentAnalysisText.Replace("**", "");

        // Misuriamo quanto spazio occupa il testo data la larghezza 'width'
        SizeF textSize = gr.MeasureString(cleanText, bodyFont, (int)width);

        // L'altezza totale è: Titolo + Testo misurato + Padding vari
        float newHeight = headerFont.Height + textSize.Height + (padding * 3);

        // --- 2. DISEGNO SFONDO (Autoadattivo) ---
        // Usiamo l'altezza calcolata al volo o quella salvata al ciclo precedente
        var bgRect = new RectangleF(x, y, width + (padding * 2), newHeight);
        
        gr.FillRectangle(panelBgBrush, bgRect);
        //gr.DrawRectangle(Pens.Gray, Rectangle.Round(bgRect)); // Bordo sottile


        // --- 3. DISEGNO TESTO ---
        // Titolo
        gr.DrawString("AI ANALYSIS", headerFont, Brushes.Orange, x + padding, y + padding);

        // Corpo del testo (senza alcun flag di troncamento o NoWrap)
        var bodyRect = new RectangleF(
            x + padding,
            y + padding + headerFont.Height + 5,
            width,
            textSize.Height + 10
        );

        gr.DrawString(cleanText, bodyFont, Brushes.LightGray, bodyRect);

        // --- 4. AGGIORNAMENTO VARIABILE PER CICLO SUCCESSIVO ---
        this.aiBoxHeight = newHeight;

    }
    private void OnChartMouseDown(object sender, ChartMouseNativeEventArgs e)
    {
        chartMouseDownReceived = true;
        //Log("OnChartMouseDown - setting chartMouseDownReceived: " + chartMouseDownReceived);
    }
    private void OnChartMouseUp(object sender, ChartMouseNativeEventArgs e)
    {
        //Log("OnChartMouseUp - reading chartMouseDownReceived: " + chartMouseDownReceived);
        //Log("OnChartMouseUp - checking chartMouseDownReceived: ");
        if (!chartMouseDownReceived) return;  // click orfano, ignoralo
        //Log("OnChartMouseUp - checking checked ");
        chartMouseDownReceived = false;

        if (toggleRect.Contains(e.Location))
        {
            isPanelVisible = !isPanelVisible;
            //Log("OnChartMouseUp - Target: toggleRect");
            return;
        }

        if (backtestBtnRect.Contains(e.Location))
        {
            isBacktestMode = !isBacktestMode;
            if (!isBacktestMode) lastProcessedBarTime = default; // Reset per tornare in real-time
            //Log("OnChartMouseUp - Target: backtestBtnRect");
            return;
        }

        if (isPanelVisible)
        {
            for (int i = 0; i < matchClickAreas.Count; i++)
            {
                if (matchClickAreas[i].Contains(e.Location))
                {
                    string date = matchDates[i];
                    string url = $"http://127.0.0.1:9999/visualize?symbol={ticker}&date={date}&timeframe={GetFormattedTimeframe()}";
                    
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
            for (int i = 0; i < contextClickAreas.Count; i++)
            {
                if (contextClickAreas[i].Contains(e.Location))
                {
                    string date = contextDates[i];
                    string url = $"http://127.0.0.1:9999/visualize?symbol={ticker}&date={date}";
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

        if (isBacktestMode)
        {
            //Log("OnChartMouseUp - Target: Backtest");
            DateTime clickTime = e.Window.CoordinatesConverter.GetTime(e.X);
            int candleIndex = GetIndexByTime(clickTime);
            RunManualAnalysis(candleIndex);
        }
    }

    protected void RunManualAnalysis(int candleIndex)
    {
        // Pulizia visiva prima del nuovo calcolo
        Reset();
        // Esegui l'analisi sulla candela cliccata
        ProcessAnalysis(candleIndex);
    }

    private void ProcessAnalysis(int refIndex)
    {
        analysisIndex = refIndex;
        //Log("ProcessAnalysis called with refIndex: " + refIndex);
        DateTime referenceTimeUtc = Time(refIndex).ToUniversalTime();


        DateTime londonNow = TimeZoneInfo.ConvertTimeFromUtc(referenceTimeUtc, LondonTz);
        DateTime nyNow = TimeZoneInfo.ConvertTimeFromUtc(referenceTimeUtc, NyTz);

        string sessionToUse = SessionId;
        if (SessionId == "AUTO")
        {
            int londonTime = londonNow.Hour * 100 + londonNow.Minute;
            int nyTime = nyNow.Hour * 100 + nyNow.Minute;

            if (londonTime >= 800 && londonTime < 1425 && nyTime < 930) sessionToUse = "LONDON_OPEN";
            else if (nyTime >= 930 && nyTime < 1705) sessionToUse = "NY_OPEN";
            else sessionToUse = effectiveSession = null;
        }

        if (sessionToUse == null) return;

        // --- LOGICA TARGET TIME (Riciclata) ---
        int targetHour = (sessionToUse == "NY_OPEN") ? 9 : 8;
        int targetMinute = (sessionToUse == "NY_OPEN") ? 30 : 0;
        TimeZoneInfo sessionTz = (sessionToUse == "NY_OPEN") ? NyTz : LondonTz;

        // --- RICERCA OPEN BAR ---
        // Cerchiamo l'apertura andando a ritroso partendo da refIndex
        openBarIndex = -1;
        for (int i = refIndex; i < Math.Min(Count, refIndex + 200); i++) // Aumentato a 200 per sicurezza
        {
            DateTime barInSessionTime = TimeZoneInfo.ConvertTimeFromUtc(Time(i).ToUniversalTime(), sessionTz);
            if (barInSessionTime.Hour == targetHour && barInSessionTime.Minute == targetMinute)
            {
                openBarIndex = i;
                break;
            }
        }
        //Log("openBarIndex: "+ openBarIndex);

        if (openBarIndex == -1) return;
        else if (openBarIndex >= MaxCandles && candlesResults.Count > 0) return;

        if (openBarIndex != -1)
        {
            DateTime foundBarTime = TimeZoneInfo.ConvertTimeFromUtc(Time(openBarIndex).ToUniversalTime(), sessionTz);

            if (foundBarTime.Hour == 9 && foundBarTime.Minute == 30)
                effectiveSession = "NY_OPEN";
            else if (foundBarTime.Hour == 8 && foundBarTime.Minute == 0)
                effectiveSession = "LONDON_OPEN";
        }
        //Log("effectiveSession: " + effectiveSession);
        // --- FETCH CONTEXT ---
        string sessionDateKey = $"{sessionToUse}_{Time(openBarIndex):yyyy-MM-dd}";
        //Log("sessionDateKey: " + sessionDateKey+ " - lastContextSessionDate: "+ lastContextSessionDate);
        if (sessionDateKey != lastContextSessionDate)
        {
            lastContextSessionDate = sessionDateKey;
            //Log("FetchContextMatchesAsync");
            _ = FetchContextMatchesAsync(openBarIndex);
        }
        if ((openBarIndex - refIndex) < MinCandles) return;
        // --- PREPARAZIONE CANDLES (Usa refIndex come limite superiore) ---
        var candlesToInquire = new List<RawCandle>();
        int limit = Math.Min(openBarIndex - refIndex, MaxCandles);

        // Ciclo che parte dall'apertura e arriva fino alla candela di riferimento
        for (int i = openBarIndex; i > refIndex && i > (openBarIndex - limit); i--)
        {
            DateTime bTime = TimeZoneInfo.ConvertTimeFromUtc(Time(i).ToUniversalTime(), NyTz);
            candlesToInquire.Add(new RawCandle
            {
                Timestamp = (uint)((DateTimeOffset)bTime).ToUnixTimeSeconds(),
                Open = (float)Open(i),
                High = (float)High(i),
                Low = (float)Low(i),
                Close = (float)Close(i),
                Volume = (float)Volume(i)
            });
        }

        currentPriceBase = (float)Open(openBarIndex);
        currentDelta = (float)Close(refIndex) - currentPriceBase;
        this.currentCandleCount = candlesToInquire.Count;
        _ = FetchMatchesAsync(candlesToInquire);
    }

    private string GetFormattedTimeframe()
    {
        HistoryAggregationTime aggregation = (HistoryAggregationTime)this.HistoricalData.Aggregation;


        // Trasforma "5m" in "M5", "15m" in "M15", "1h" in "H1", ecc.
        string tf = aggregation.Period.Format();

        if (tf.EndsWith("m"))
            return "M" + tf.Replace("m", "");

        if (tf.EndsWith("h"))
            return "H" + tf.Replace("h", "");

        return tf; // Fallback se è già nel formato corretto o ignoto
    }

    public override void Dispose()
    {
        //Log("ondispose");
        this.CurrentChart.MouseUp -= OnChartMouseUp;
        this.CurrentChart.MouseDown -= OnChartMouseDown;
    }

    protected void initTicker()
    {
        string symbol = this.Symbol.Description.ToLower();
        if (symbol.Contains("nasdaq"))
        {
            ticker = "NQ";
        }
        else if (symbol.Contains("s&p"))
        {
            ticker = "ES";
        }
        else if (symbol.Contains("gold"))
        {
            ticker = "GC";
        }
        else if (symbol.Contains("dow"))
        {
            ticker = "YM";
        }
    }


    private int GetIndexByTime(DateTime clickedTime)
    {
        // 'Count' è il numero totale di candele sul grafico
        for (int i = 0; i < Count; i++)
        {
            // Time(i) restituisce l'orario della candela all'indice i
            if (Time(i) <= clickedTime)
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
    }

    public class NextCandleData
    {
        [JsonPropertyName("avg_up")] public float AvgUp { get; set; }
        [JsonPropertyName("avg_down")] public float AvgDown { get; set; }
        [JsonPropertyName("edge_ratio")] public float EdgeRatio { get; set; }
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

    public class AiAnalysisRequest
    {

        [JsonPropertyName("symbol")] public string Symbol { get; set; }
        [JsonPropertyName("session_id")] public string SessionId { get; set; }
        [JsonPropertyName("candles")] public List<RawCandle> Candles { get; set; }
        [JsonPropertyName("timestamp")] public uint Timestamp { get; set; }
        [JsonPropertyName("timeframe")] public string Timeframe { get; set; }
        [JsonPropertyName("current_price_action")] public float CurrentPriceAction { get; set; }
        [JsonPropertyName("session_open")] public float SessionOpen { get; set; }
        [JsonPropertyName("prev_close")] public float PrevClose { get; set; }
        [JsonPropertyName("prev_high")] public float PrevHigh { get; set; }
        [JsonPropertyName("prev_low")] public float PrevLow { get; set; }
        [JsonPropertyName("intraday_high")] public float IntradayHigh { get; set; }
        [JsonPropertyName("intraday_low")] public float IntradayLow { get; set; }
        [JsonPropertyName("top_k")] public int TopK { get; set; }
    }
    public class AiResponse
    {
        public string status { get; set; }
        public string analysis { get; set; }
        public bool cached { get; set; }
    }
    #endregion
}