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
    public int MinCandles = 3;

    [InputParameter("Max Candles for Pattern", 1, 1, 10, 1)]
    public int MaxCandles = 6;

    [InputParameter("Webserver URL", 2)]
    public string ServerUrl = "http://127.0.0.1:9999/search";
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

    #endregion

    public RetroVis() : base()
    {
        Name = "RetroVis";
        SeparateWindow = false;
    }

    protected override void OnInit()
    {
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
        this.currentCandleCount = -1;

        // Opzionale: Se vuoi un feedback visivo immediato
        // Log.Info("Parametri aggiornati: ricaricamento pattern...");
    }

    protected override void OnUpdate(UpdateArgs args)
    {
        currentDelta = Close(0) - currentPriceBase;

        DateTime currentBarTime = Time();
        if (currentBarTime == lastProcessedBarTime) return;
        // Filtro real-time
        HistoryAggregationTime aggregation = (HistoryAggregationTime)this.HistoricalData.Aggregation;

        if (currentBarTime.ToUniversalTime() < DateTime.UtcNow.AddMinutes(-aggregation.Period.Duration.TotalMinutes))
            return;



        lastProcessedBarTime = currentBarTime;

        // Parametri orario in base alla sessione

        effectiveSession = (SessionId == "AUTO")
            ? ((TimeZoneInfo.ConvertTimeFromUtc(DateTime.UtcNow, NyTz).Hour < 9 || (TimeZoneInfo.ConvertTimeFromUtc(DateTime.UtcNow, NyTz).Hour == 9 && TimeZoneInfo.ConvertTimeFromUtc(DateTime.UtcNow, NyTz).Minute < 30)) ? "LONDON_OPEN" : "NY_OPEN")
            : SessionId;

        

        int targetHour = (effectiveSession == "NY_OPEN") ? 9 : 9;
        int targetMinute = (effectiveSession == "NY_OPEN") ? 30 : 0;
        TimeZoneInfo sessionTz = (effectiveSession == "NY_OPEN") ? NyTz : LondonTz;

        int openBarIndex = -1;
        for (int i = 0; i < Math.Min(Count, 300); i++)
        {
            // Convertiamo l'orario della candela nel fuso orario della sessione
            DateTime barInSessionTime = TimeZoneInfo.ConvertTimeFromUtc(Time(i).ToUniversalTime(), sessionTz);

            if (barInSessionTime.Hour == targetHour && barInSessionTime.Minute == targetMinute)
            {
                openBarIndex = i;
                break;
            }
        }

        if (openBarIndex == -1 || openBarIndex < MinCandles) return;

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
        Log("openBarIndex" + openBarIndex + "currentPriceBase: " + currentPriceBase);
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
            var response = await httpClient.PostAsync(ServerUrl, content);

            if (response.IsSuccessStatusCode)
            {
                
                string resJson = await response.Content.ReadAsStringAsync();
                Log("Success:" + resJson);
                results = JsonSerializer.Deserialize<List<MatchResponse>>(resJson);
            }
        }
        catch (Exception ex) { Log("Error:"+ ex.Message); }
    }

    public override void OnPaintChart(PaintChartEventArgs args)
    {
        Graphics gr = args.Graphics;
        gr.FillRectangle(isPanelVisible ? Brushes.IndianRed : Brushes.SteelBlue, toggleRect);
        gr.DrawString(isPanelVisible ? "HIDE PANEL" : "SHOW PANEL", new Font("Arial", 7, FontStyle.Bold), Brushes.White, toggleRect.X + 15, toggleRect.Y + 7);

        if (isPanelVisible) DrawResultsPanel(gr);
    }

    private void DrawResultsPanel(Graphics gr)
    {
        Rectangle panelRect = new Rectangle(toggleRect.X, toggleRect.Y + 35, 380, 260);
        gr.FillRectangle(new SolidBrush(Color.FromArgb(200, 10, 10, 10)), panelRect);

        int tx = panelRect.X + 12, ty = panelRect.Y + 12;
        if (results == null || results.Count == 0)
        {
            gr.DrawString("WAITING FOR DATA...", titleFont, Brushes.Gray, tx, ty);
            return;
        }
        string deltaSign = currentDelta >= 0 ? "+" : "";
        string deltaText = $" | NOW: {deltaSign}{currentDelta:F1}";
        Brush deltaBrush = currentDelta >= 0 ? Brushes.LimeGreen : Brushes.Red;


        string dynamicTitle = $"{effectiveSession} - TOP MATCHES ({this.currentCandleCount} CANDLES)";
        gr.DrawString(dynamicTitle, titleFont, Brushes.White, tx, ty);
        float titleWidth = gr.MeasureString(dynamicTitle, titleFont).Width;
        gr.DrawString(deltaText, titleFont, deltaBrush, tx + titleWidth, ty);
        ty += 30;

        foreach (var m in results)
        {
            // Reintegro la normalizzazione della distanza in percentuale
            float similarity = 1.0f / (1.0f + m.Distance) * 100f;

            // Calcolo valori assoluti in punti
            double eodPts = (m.EodReturn / 100.0) * currentPriceBase;
            double mfePts = (m.Mfe / 100.0) * currentPriceBase;
            double maePts = (m.Mae / 100.0) * currentPriceBase;

            // 1. Riga intestazione: Data, Ora e Similarità (Match %)
            string header = $"{m.Date} {m.Time} | Match: {similarity:F1}%";
            gr.DrawString(header, dataFont, Brushes.White, tx, ty);
            ty += 18;

            // 2. Riga Statistiche Splittata (Colori Dinamici)
            float currentX = tx + 10;

            // --- EOD ---
            string eodTxt = $"EOS: {eodPts:+0.0;-0.0}";
            Brush eodBrush = eodPts >= 0 ? Brushes.LimeGreen : Brushes.Red;
            gr.DrawString(eodTxt, dataFont, eodBrush, currentX, ty);
            currentX += gr.MeasureString(eodTxt, dataFont).Width;

            // SEPARATOR 
            string separatorTxt = $" | ";
            gr.DrawString(separatorTxt, dataFont, Brushes.White, currentX, ty);
            currentX += gr.MeasureString(separatorTxt, dataFont).Width;

            // --- MFE ---
            string mfeTxt = $"UP: {mfePts:+0.0}";

            Brush mfeBrush = mfePts >= 0 ? Brushes.LimeGreen : Brushes.Red;
            gr.DrawString(mfeTxt, dataFont, mfeBrush, currentX, ty);
            currentX += gr.MeasureString(mfeTxt, dataFont).Width;

            // SEPARATOR 
            gr.DrawString(separatorTxt, dataFont, Brushes.White, currentX, ty);
            currentX += gr.MeasureString(separatorTxt, dataFont).Width;

            // --- MAE ---
            string maeTxt = $"DOWN: {maePts:0.0}";

            Brush maeBrush = maePts <= 0 ? Brushes.Red : Brushes.LimeGreen;
            gr.DrawString(maeTxt, dataFont, maeBrush, currentX, ty);

            ty += 25;
        }
    }

    private void OnChartClick(object sender, ChartMouseNativeEventArgs e) => isPanelVisible = toggleRect.Contains(e.Location) ? !isPanelVisible : isPanelVisible;

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
        
        //Core.Instance.Loggers.Log($"[RetroVis] {m}", LoggingLevel.Trading);
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
    #endregion
}