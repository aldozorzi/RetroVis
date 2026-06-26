using System.Text.Json.Serialization;

namespace Ldazrz.RetroVis.Models;

public class RawCandle
{
    [JsonPropertyName("timestamp")] public uint Timestamp { get; set; }
    [JsonPropertyName("open")] public float Open { get; set; }
    [JsonPropertyName("high")] public float High { get; set; }
    [JsonPropertyName("low")] public float Low { get; set; }
    [JsonPropertyName("close")] public float Close { get; set; }
    [JsonPropertyName("_volume")] public float Volume { get; set; }
}
