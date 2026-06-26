using System.Text.Json.Serialization;
using System.Collections.Generic;

namespace Ldazrz.RetroVis.Models;

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
