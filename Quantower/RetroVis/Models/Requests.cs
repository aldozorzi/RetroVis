using System.Text.Json.Serialization;
using System.Collections.Generic;

namespace Ldazrz.RetroVis.Models;

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
