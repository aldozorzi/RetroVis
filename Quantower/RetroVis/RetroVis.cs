using System;
using System.Collections.Generic;
using System.Drawing;
using System.Net.Http;
using System.Runtime.Versioning;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Threading.Tasks;
using TradingPlatform.BusinessLayer;
using TradingPlatform.BusinessLayer.Chart;

[SupportedOSPlatform("windows")]
public class RetroVis : Indicator
{
    #region Parameters
    [InputParameter("Session Type", 0, variants: new object[] { "AUTO", "AUTO", "NY_OPEN", "NY_OPEN", "LONDON_OPEN", "LONDON_OPEN" })]
    public string SessionId = "AUTO";

    [InputParameter("Min Candles for Pattern", 1, 1, 10, 1)]
    public int MinCandles = 2;

    [InputParameter("Max Candles for Pattern", 1, 1, 10, 1)]
    public int MaxCandles = 6;

    [InputParameter("Webserver URL", 2)]
    public string ServerUrl = "http://127.0.0.1:9999";
    
    #endregion

    #region Internal Fields
    private readonly HttpClient httpClient = new HttpClient();
    private List<MatchResponse> results = new List<MatchResponse>();
    private bool isPanelVisible = true;
    private Rectangle toggleRect = new Rectangle(5, 80, 120, 25);
    private double currentPriceBase = 0;
    private DateTime lastProcessedBarTime;
    private Font titleFont;
    private Font dataFont;
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
    private List<MatchResponse> contextResults = new List<MatchResponse>();
    private string lastContextSessionDate = ""; // traccia se il contesto è già stato caricato per questa apertura

    private List<RectangleF> contextClickAreas = new List<RectangleF>();
    private List<string> contextDates = new List<string>();

    private int lastPanelHeight = 0;

    #endregion

    public RetroVis() : base()
    {
        Name = "RetroVis";
        SeparateWindow = false;
    }

    protected override void OnInit()
    {
        this.SearchUrl = $"{ServerUrl.TrimEnd('/')}/search";
        this.ContextUrl = $"{ServerUrl.TrimEnd('/')}/context_matches";
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
            this.CurrentChart.MouseClick += OnChartClick;

        this.titleFont = new Font("Arial", 10, FontStyle.Bold);
        this.dataFont = new Font("Consolas", 9);

        initTicker(false);
        
    }

    protected override void OnSettingsUpdated()
    {
        base.OnSettingsUpdated();

        // 1. Azzeriamo il tempo dell'ultima barra elaborata per forzare il passaggio dell'IF
        this.lastProcessedBarTime = default(DateTime);

        // 2. Azzeriamo il tuo contatore interno
        this.currentCandleCount = 0;

        // Opzionale: Se vuoi un feedback visivo immediato
        // Log.Info("Parametri aggiornati: ricaricamento pattern...");
    }

    protected override void OnUpdate(UpdateArgs args)
    {
        currentDelta = Close(0) - currentPriceBase;

        DateTime currentBarTime = Time();
        if (currentBarTime == lastProcessedBarTime) return;
        else
        {
            results.Clear();
            matchClickAreas.Clear();
            contextClickAreas.Clear();
        }
        // Filtro real-time
        HistoryAggregationTime aggregation = (HistoryAggregationTime)this.HistoricalData.Aggregation;

        if (currentBarTime.ToUniversalTime() < DateTime.UtcNow.AddMinutes(-aggregation.Period.Duration.TotalMinutes))
            return;



        lastProcessedBarTime = currentBarTime;

        // Parametri orario in base alla sessione

        DateTime nowUtc = DateTime.UtcNow;
        DateTime londonNow = TimeZoneInfo.ConvertTimeFromUtc(nowUtc, LondonTz);
        DateTime nyNow = TimeZoneInfo.ConvertTimeFromUtc(nowUtc, NyTz);

        if (SessionId == "AUTO")
        {
            // --- LONDRA (08:00 - 17:00 orario di Londra) ---
            int londonTime = londonNow.Hour * 100 + londonNow.Minute;

            // --- NEW YORK (09:30 - 16:00 orario di NY) ---
            int nyTime = nyNow.Hour * 100 + nyNow.Minute;

            if (londonTime >= 800 && londonTime < 1700 && nyTime < 930)
            {
                // Siamo in sessione Londra e NY non è ancora aperta
                effectiveSession = "LONDON_OPEN";
            }
            else if (nyTime >= 930 && nyTime < 1600)
            {
                // Sessione New York
                effectiveSession = "NY_OPEN";
            }
            else
            {
                effectiveSession = null; // Fuori sessione o sessione non gestita
            }
        }
        else
        {
            effectiveSession = SessionId;
        }

        // 2. Assegnazione parametri basata sulla sessione identificata
        int targetHour;
        int targetMinute;
        TimeZoneInfo sessionTz;
        if (effectiveSession != null)
        {
            if (effectiveSession == "NY_OPEN")
            {
                targetHour = 9;
                targetMinute = 30;
                sessionTz = NyTz;
            }
            else if (effectiveSession == "LONDON_OPEN")
            {
                targetHour = 8;
                targetMinute = 0;
                sessionTz = LondonTz;
            }
            else
            {
                // Default o altre sessioni
                targetHour = 0;
                targetMinute = 0;
                sessionTz = NyTz;
            }

            // Procedi con il calcolo dei match...
        }
        else
        {
            return;
        }



        /*effectiveSession = (SessionId == "AUTO")
            ? ((TimeZoneInfo.ConvertTimeFromUtc(DateTime.UtcNow, NyTz).Hour < 9 || (TimeZoneInfo.ConvertTimeFromUtc(DateTime.UtcNow, NyTz).Hour == 9 && TimeZoneInfo.ConvertTimeFromUtc(DateTime.UtcNow, NyTz).Minute < 30)) ? "LONDON_OPEN" : "NY_OPEN")
            : SessionId;

        

        int targetHour = (effectiveSession == "NY_OPEN") ? 9 : 8;
        int targetMinute = (effectiveSession == "NY_OPEN") ? 30 : 0;
        TimeZoneInfo sessionTz = (effectiveSession == "NY_OPEN") ? NyTz : LondonTz;*/

        int openBarIndex = -1;
        for (int i = 0; i < Math.Min(Count, 90); i++)
        {
            DateTime barInSessionTime = TimeZoneInfo.ConvertTimeFromUtc(Time(i).ToUniversalTime(), sessionTz);

            if (barInSessionTime.Hour == targetHour && barInSessionTime.Minute == targetMinute)
            {
                openBarIndex = i;
                break;
            }
        }
        if (openBarIndex == -1 || openBarIndex < MinCandles)
        {
            results.Clear();
            matchClickAreas.Clear();
            contextClickAreas.Clear();
            return;
        }
        string sessionDateKey = $"{effectiveSession}_{Time(openBarIndex):yyyy-MM-dd}";
        if (sessionDateKey != lastContextSessionDate)
        {
            lastContextSessionDate = sessionDateKey;
            _ = FetchContextMatchesAsync(openBarIndex);
        }


        var candlesToInquire = new List<RawCandle>();
        // Prendiamo le candele dall'apertura fino alla corrente (max MaxCandles)
        int limit = Math.Min(openBarIndex, MaxCandles);

        for (int i = openBarIndex; i >= 1 && i > (openBarIndex - limit); i--)
        {
            DateTime bTime = TimeZoneInfo.ConvertTimeFromUtc(Time(i).ToUniversalTime(), NyTz);
            candlesToInquire.Add(new RawCandle
            {
                Date = bTime.ToString("yyyy-MM-dd"),
                Time = bTime.ToString("HH:mm:ss"),
                Open = (float)Open(i),
                High = (float)High(i),
                Low = (float)Low(i),
                Close = (float)Close(i),
                Volume = (float)Volume(i)
            });
        }
        
        currentPriceBase = Open(openBarIndex);
        this.currentCandleCount = candlesToInquire.Count;
        
        _ = FetchMatchesAsync(candlesToInquire);
    }

    private async Task FetchMatchesAsync(List<RawCandle> candles)
    {
        try
        {
            var request = new QueryRequest
            {
                Symbol = ticker,
                SessionId = effectiveSession,
                Candles = candles
            };
            
            string json = JsonSerializer.Serialize(request);
            Log("json:" + json);
            var content = new StringContent(json, Encoding.UTF8, "application/json");
            var response = await httpClient.PostAsync(SearchUrl, content);

            if (response.IsSuccessStatusCode)
            {
                
                string resJson = await response.Content.ReadAsStringAsync();
                Log("Success:" + resJson);
                results = JsonSerializer.Deserialize<List<MatchResponse>>(resJson);
            }
        }
        catch (Exception ex) { Log("Error:"+ ex.Message); }
    }

    private async Task FetchContextMatchesAsync(int openBarIndex)
    {
        
        try
        {
            // --- CONFIGURAZIONE SESSIONE ---
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

            // --- DATI SESSIONE CORRENTE ---
            float sessionOpen = (float)Open(openBarIndex);

            // Max/min delle candele del giorno corrente PRIMA dell'apertura di sessione
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

            // --- DATI GIORNO PRECEDENTE (per sessione: 17:00 NY → 17:00 NY) ---
            float prevClose = 0, prevHigh = float.MinValue, prevLow = float.MaxValue;
            bool foundPrevDay = false;

            // La sessione precedente va dalle 17:00 NY di due giorni fa alle 17:00 NY di ieri
            DateTime prevSessionEnd = new DateTime(openBarDate.Year, openBarDate.Month, openBarDate.Day, closeHour, closeMinute, 0).AddDays(-1);
            DateTime prevSessionStart = prevSessionEnd.AddDays(-1);

            for (int i = openBarIndex + 1; i < Math.Min(Count, openBarIndex + 1000); i++)
            {
                DateTime barNy = TimeZoneInfo.ConvertTimeFromUtc(Time(i).ToUniversalTime(), NyTz);

                if (barNy >= prevSessionEnd) continue;   // ancora nella sessione corrente
                if (barNy < prevSessionStart) break;      // troppo indietro, usciamo

                // Prima barra incontrata = più recente prima delle 17:00 = close di sessione
                if (!foundPrevDay)
                {
                    prevClose = (float)Close(i);
                    foundPrevDay = true;
                }
                if ((float)High(i) > prevHigh) prevHigh = (float)High(i);
                if ((float)Low(i) < prevLow) prevLow = (float)Low(i);
            }

            if (!foundPrevDay) return;

            // --- CHIAMATA AL SERVER ---
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
                TopK = 5
            };

            string json = JsonSerializer.Serialize(request);
            var content = new StringContent(json, Encoding.UTF8, "application/json");
            var response = await httpClient.PostAsync(ContextUrl, content);

            if (response.IsSuccessStatusCode)
            {
                string resJson = await response.Content.ReadAsStringAsync();
                contextResults = JsonSerializer.Deserialize<List<MatchResponse>>(resJson);
            }
        }
        catch (Exception ex) { Log("ContextError: " + ex.Message); }
    }

    public override void OnPaintChart(PaintChartEventArgs args)
    {
        Graphics gr = args.Graphics;
        gr.FillRectangle(isPanelVisible ? Brushes.IndianRed : Brushes.SteelBlue, toggleRect);
        string label = isPanelVisible ? "HIDE PANEL" : "SHOW PANEL";
        Font btnfont = new Font("Arial", 7, FontStyle.Bold);
        float titleWidth = gr.MeasureString(label, btnfont).Width;
        gr.DrawString(label, btnfont, Brushes.White, toggleRect.X + (float)Math.Round((toggleRect.Width - titleWidth) / 2), toggleRect.Y + 7);

        if (isPanelVisible) DrawResultsPanel(gr);
    }

    private void DrawResultsPanel(Graphics gr)
    {
        // Altezza ottimizzata senza winrate
        Rectangle panelRect = new Rectangle(toggleRect.X, toggleRect.Y + 35, 330, lastPanelHeight);
        gr.FillRectangle(new SolidBrush(Color.FromArgb(220, 10, 10, 10)), panelRect);

        int tx = panelRect.X + 12, ty = panelRect.Y + 12;
        

        // --- TESTATA AGNOSTICA ---
        string deltaSign = currentDelta >= 0 ? "+" : "";
        string deltaText = $" | NOW: {deltaSign}{currentDelta:F1}";
        // Il colore del delta attuale serve solo a sapere dove siamo rispetto all'open
        Brush deltaBrush = currentDelta >= 0 ? Brushes.LimeGreen : Brushes.Red;

        string dynamicTitle = "";
        if(effectiveSession != null)
        {
            string candlesInfo = this.currentCandleCount < this.MinCandles ? $"Waiting for {this.MinCandles} candles" : $"{this.currentCandleCount} candles";
            dynamicTitle = $"{effectiveSession} ({candlesInfo})";
        }
        else
        {
            dynamicTitle = "NO ACTIVE SESSION";
            lastPanelHeight = 100;
        }
        gr.DrawString(dynamicTitle, titleFont, Brushes.White, tx, ty);
        float titleWidth = gr.MeasureString(dynamicTitle, titleFont).Width;
        if (results.Count > 0)
        {
            gr.DrawString(deltaText, titleFont, deltaBrush,2+ titleWidth, ty);
        }
        ty += 22;
        ty += 10;
        gr.DrawString("CANDLE MATCHES", titleFont, Brushes.White, tx, ty);
        ty += 18;

        if (results == null || results.Count == 0)
        {
            gr.DrawString("WAITING FOR DATA...", titleFont, Brushes.Gray, tx, ty);
            return;
        }

        // --- SEZIONE SUMMARY (VOLATILITY EDGE) ---
        double sumMfe = 0, sumMae = 0, sumEos = 0;

        foreach (var m in results)
        {
            sumMfe += Math.Abs((m.Mfe / 100.0) * currentPriceBase);
            sumMae += Math.Abs((m.Mae / 100.0) * currentPriceBase);
            sumEos += (m.EodReturn / 100.0) * currentPriceBase;
        }

        double avgMfe = sumMfe / results.Count;
        double avgMae = sumMae / results.Count;
        double avgEos = sumEos / results.Count;

        // L'Edge Ratio qui è: quanto spazio ho mediamente a favore vs quanto contro
        double edgeRatio = avgMfe / (avgMae == 0 ? 1 : avgMae);

        // Box summary
        Rectangle summaryRect = new Rectangle(tx, ty, panelRect.Width - 24, 45);
        gr.DrawRectangle(Pens.DimGray, summaryRect);

        string summaryLine1 = $"AVG UP: +{avgMfe:F1} pts | AVG DOWN: -{avgMae:F1} pts";
        

        // Il colore è solo sull'Edge Ratio: > 2.0 è un segnale di asimmetria forte
        double longEdge = avgMfe / (avgMae == 0 ? 1 : avgMae);
        double shortEdge = avgMae / (avgMfe == 0 ? 1 : avgMfe);
        Brush edgeBrush = Brushes.White;
        if (longEdge > 2.0)
        {
            edgeBrush = Brushes.LimeGreen; // Edge Long netto
        }
        else if (shortEdge > 2.0)
        {
            edgeBrush = Brushes.Red; // Edge Short netto (il rumore UP è piccolo rispetto all'espansione DOWN)
        }
        else if (longEdge > 1.2 || shortEdge > 1.2)
        {
            edgeBrush = Brushes.Gold; // "Macedonia": UP e DOWN si equivalgono (Range)
        }
        string summaryLine2 = $"EDGE RATIO: {Math.Max(longEdge,shortEdge):F2} | AVG EOS: {avgEos:+0.0;-0.0}";

        gr.DrawString(summaryLine1, dataFont, Brushes.LightGray, tx + 5, ty + 5);
        gr.DrawString(summaryLine2, dataFont, edgeBrush, tx + 5, ty + 22);

        ty += 55;

        // --- LISTA MATCH ---
        foreach (var m in results)
        {
            float similarity = 1.0f / (1.0f + m.Distance) * 100f;
            double eodPts = (m.EodReturn / 100.0) * currentPriceBase;
            double mfePts = (m.Mfe / 100.0) * currentPriceBase;
            double maePts = (m.Mae / 100.0) * currentPriceBase;

            matchClickAreas.Add(new RectangleF(tx, ty, panelRect.Width - 30, 35));
            matchDates.Add(m.Date);

            // Intestazione match (grigia per non distrarre)
            gr.DrawString($"{m.Date} {m.Time} (MATCH:{similarity:F1}%)", dataFont, Brushes.Gray, tx, ty);
            ty += 15;

            // Dati puri: UP e DOWN dominano la lettura
            // EOS bianco (agnostico), UP e DOWN evidenziati solo per direzione
            string vals = $"UP: {mfePts:+0.0}  DOWN: {maePts:0.0}  (EOS: {eodPts:+0.0;-0.0})";
            gr.DrawString(vals, dataFont, Brushes.White, tx + 10, ty);

            ty += 25;
        }

        // --- CONTEXT MATCHES ---
        if (contextResults != null && contextResults.Count > 0)
        {
             ty += 10;
            gr.DrawString("CONTEXT MATCHES", titleFont, Brushes.White, tx, ty);
            ty += 18;
            // --- CONTEXT MATCHES SUMMARY ---
            double ctxSumMfe = 0, ctxSumMae = 0, ctxSumEos = 0;
            foreach (var m in contextResults)
            {
                ctxSumMfe += Math.Abs((m.Mfe / 100.0) * currentPriceBase);
                ctxSumMae += Math.Abs((m.Mae / 100.0) * currentPriceBase);
                ctxSumEos += (m.EodReturn / 100.0) * currentPriceBase;
            }
            double ctxAvgMfe = ctxSumMfe / contextResults.Count;
            double ctxAvgMae = ctxSumMae / contextResults.Count;
            double ctxAvgEos = ctxSumEos / contextResults.Count;
            double ctxEdge = ctxAvgMfe / (ctxAvgMae == 0 ? 1 : ctxAvgMae);
            double ctxShortEdge = ctxAvgMae / (ctxAvgMfe == 0 ? 1 : ctxAvgMfe);

            Brush ctxEdgeBrush = Brushes.White;
            if (ctxEdge > 2.0) ctxEdgeBrush = Brushes.LimeGreen;
            else if (ctxShortEdge > 2.0) ctxEdgeBrush = Brushes.Red;
            else if (ctxEdge > 1.2 || ctxShortEdge > 1.2) ctxEdgeBrush = Brushes.Gold;

            Rectangle ctxSummaryRect = new Rectangle(tx, ty, panelRect.Width - 24, 45);
            gr.DrawRectangle(Pens.DimGray, ctxSummaryRect);

            gr.DrawString($"AVG UP: +{ctxAvgMfe:F1} pts | AVG DOWN: -{ctxAvgMae:F1} pts", dataFont, Brushes.LightGray, tx + 5, ty + 5);
            gr.DrawString($"EDGE RATIO: {Math.Max(ctxEdge, ctxShortEdge):F2} | AVG EOS: {ctxAvgEos:+0.0;-0.0}", dataFont, ctxEdgeBrush, tx + 5, ty + 22);

            ty += 55;

            foreach (var m in contextResults)
            {
                float similarity = 1.0f / (1.0f + m.Distance) * 100f;
                double eodPts = (m.EodReturn / 100.0) * currentPriceBase;
                double mfePts = (m.Mfe / 100.0) * currentPriceBase;
                double maePts = (m.Mae / 100.0) * currentPriceBase;

                var ctxRect = new RectangleF(tx, ty, panelRect.Width - 24, 40);
                gr.FillRectangle(new SolidBrush(Color.FromArgb(0, 180, 100, 30)), ctxRect);
                contextClickAreas.Add(ctxRect);
                contextDates.Add(m.Date);

                gr.DrawString($"{m.Date} {m.Time} (MATCH:{similarity:F1}%)", dataFont, Brushes.Gray, tx, ty);
                ty += 15;

                string vals = $"UP: {mfePts:+0.0}  DOWN: {maePts:0.0}  (EOS: {eodPts:+0.0;-0.0})";
                gr.DrawString(vals, dataFont, Brushes.White, tx + 10, ty);
                ty += 25;
            }
            lastPanelHeight = ty - panelRect.Y + 10;
            //gr.FillRectangle(new SolidBrush(Color.FromArgb(220, 10, 10, 10)), panelRect);
        }
    }

    private void OnChartClick(object sender, ChartMouseNativeEventArgs e)
    {
        // 1. Gestione Toggle Pannello (Tua logica esistente)
        if (toggleRect.Contains(e.Location))
        {
            isPanelVisible = !isPanelVisible;
            return; // Esci subito dopo il toggle
        }

        // 2. Gestione Link Storici (Solo se il pannello è aperto)
        if (isPanelVisible)
        {
            for (int i = 0; i < matchClickAreas.Count; i++)
            {
                if (matchClickAreas[i].Contains(e.Location))
                {
                    string date = matchDates[i];

                    string url = $"http://127.0.0.1:9999/visualize?symbol={ticker}&date={date}";

                    try
                    {
                        System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                        {
                            FileName = url,
                            UseShellExecute = true
                        });
                    }
                    catch (Exception ex)
                    {
                        Log("Errore apertura browser: " + ex.Message);
                    }

                    break; // Clic intercettato, non serve controllare altri match
                }
            }
            for (int i = 0; i < contextClickAreas.Count; i++)
            {
                if (contextClickAreas[i].Contains(e.Location))
                {
                    string date = contextDates[i];
                    string url = $"http://127.0.0.1:9999/visualize?symbol={ticker}&date={date}";
                    try { System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo { FileName = url, UseShellExecute = true }); }
                    catch (Exception ex) { Log("Errore apertura browser: " + ex.Message); }
                    break;
                }
            }

        }
    }


    protected override void OnClear()
    {
        if (this.CurrentChart != null) this.CurrentChart.MouseClick -= OnChartClick;
        titleFont?.Dispose(); dataFont?.Dispose();
    }

    protected void initTicker(bool useMicro = true)
    {
        string symbol = this.Symbol.Description.ToLower();
        if (useMicro && symbol.Contains("nasdaq") && symbol.Contains("micro"))
        {
            ticker = "MNQ";
        }
        else if (symbol.Contains("nasdaq"))
        {
            ticker = "NQ";
        }
        else if (useMicro &&symbol.Contains("s&p") && symbol.Contains("micro"))
        {
            ticker = "MES";
        }
        else if (symbol.Contains("s&p"))
        {
            ticker = "ES";
        }
        else if (useMicro && symbol.Contains("gold") && symbol.Contains("micro"))
        {
            ticker = "MGC";
        }
        else if (symbol.Contains("gold"))
        {
            ticker = "GC";
        }
    }

    public static void Log(string m){
        
        Core.Instance.Loggers.Log($"[RetroVis] {m}", LoggingLevel.Trading);
    } 

    #region Models
    public class QueryRequest
    {
        [JsonPropertyName("symbol")] public string Symbol { get; set; }
        [JsonPropertyName("session_id")] public string SessionId { get; set; }
        [JsonPropertyName("candles")] public List<RawCandle> Candles { get; set; }
    }

    public class RawCandle
    {
        [JsonPropertyName("date")] public string Date { get; set; }
        [JsonPropertyName("time")] public string Time { get; set; }
        [JsonPropertyName("open")] public float Open { get; set; }
        [JsonPropertyName("high")] public float High { get; set; }
        [JsonPropertyName("low")] public float Low { get; set; }
        [JsonPropertyName("close")] public float Close { get; set; }
        [JsonPropertyName("_volume")] public float Volume { get; set; }
    }

    public class MatchResponse
    {
        [JsonPropertyName("date")] public string Date { get; set; }
        [JsonPropertyName("time")] public string Time { get; set; }
        [JsonPropertyName("distance")] public float Distance { get; set; }
        [JsonPropertyName("eod_return")] public float EodReturn { get; set; }
        [JsonPropertyName("mfe")] public float Mfe { get; set; }
        [JsonPropertyName("mae")] public float Mae { get; set; }
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
    }
    #endregion
}