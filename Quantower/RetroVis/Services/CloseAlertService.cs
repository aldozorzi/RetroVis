using System;
using System.IO;
using System.Media;
using System.Runtime.Versioning;
using TradingPlatform.BusinessLayer;

namespace Ldazrz.RetroVis.Services;

/// <summary>
/// Plays a sound shortly before the current bar closes. Self-contained:
/// only needs the chart's HistoricalData, nothing else from RetroVis's state.
/// </summary>
[SupportedOSPlatform("windows")]
public class CloseAlertService(HistoricalData data)
{
    private readonly HistoricalData _data = data;
    private SoundPlayer _player;
    private string _loadedPath = "";
    private static DateTime _lastAlertBarTime = DateTime.MinValue;

    public void LoadSound(string path)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(path))
            {
                this._player = null;
                this._loadedPath = "";
                return;
            }
            if (path == this._loadedPath && this._player != null) return;

            if (!File.Exists(path))
            {
                RetroVisLog.Error("Alert sound file non trovato: " + path);
                this._player = null;
                this._loadedPath = "";
                return;
            }

            this._player = new SoundPlayer(path);
            this._player.Load();
            this._loadedPath = path;
        }
        catch (Exception ex)
        {
            RetroVisLog.Error("Errore caricamento alert sound: " + ex.Message);
            this._player = null;
            this._loadedPath = "";
        }
    }

    public static void Reset() => CloseAlertService._lastAlertBarTime = DateTime.MinValue;

    public void CheckAndPlay(bool enabled, bool isBacktestMode, int secondsBeforeClose)
    {
        if (!enabled || isBacktestMode || this._player == null) return;

        var aggregation = (HistoryAggregationTime)this._data.Aggregation;
        TimeSpan duration = aggregation.Period.Duration;

        if (duration >= TimeSpan.FromDays(1) || duration <= TimeSpan.Zero) return;

        DateTime currentBarTime = this._data.Time(0);
        if (currentBarTime == _lastAlertBarTime) return;

        DateTime barCloseTimeUtc = currentBarTime.ToUniversalTime() + duration;
        double secondsRemaining = (barCloseTimeUtc - DateTime.UtcNow).TotalSeconds;

        if (secondsRemaining > 0 && secondsRemaining <= secondsBeforeClose)
        {
            CloseAlertService._lastAlertBarTime = currentBarTime;
            try { this._player.Play(); }
            catch (Exception ex) { RetroVisLog.Error("Errore riproduzione alert sound: " + ex.Message); }
        }
    }

    public void Dispose() => this._player?.Dispose();
}
