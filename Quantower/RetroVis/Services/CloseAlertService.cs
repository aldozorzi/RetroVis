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
public class CloseAlertService
{
    private readonly HistoricalData _data;
    private SoundPlayer _player;
    private string _loadedPath = "";
    private static DateTime _lastAlertBarTime = DateTime.MinValue;

    public CloseAlertService(HistoricalData data)
    {
        _data = data;
    }

    public void LoadSound(string path)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(path))
            {
                _player = null;
                _loadedPath = "";
                return;
            }
            if (path == _loadedPath && _player != null) return;

            if (!File.Exists(path))
            {
                RetroVisLog.Error("Alert sound file non trovato: " + path);
                _player = null;
                _loadedPath = "";
                return;
            }

            _player = new SoundPlayer(path);
            _player.Load();
            _loadedPath = path;
        }
        catch (Exception ex)
        {
            RetroVisLog.Error("Errore caricamento alert sound: " + ex.Message);
            _player = null;
            _loadedPath = "";
        }
    }

    public void Reset() => _lastAlertBarTime = DateTime.MinValue;

    public void CheckAndPlay(bool enabled, bool isBacktestMode, int secondsBeforeClose)
    {
        if (!enabled || isBacktestMode || _player == null) return;

        var aggregation = (HistoryAggregationTime)_data.Aggregation;
        TimeSpan duration = aggregation.Period.Duration;

        if (duration >= TimeSpan.FromDays(1) || duration <= TimeSpan.Zero) return;

        DateTime currentBarTime = _data.Time(0);
        if (currentBarTime == _lastAlertBarTime) return;

        DateTime barCloseTimeUtc = currentBarTime.ToUniversalTime() + duration;
        double secondsRemaining = (barCloseTimeUtc - DateTime.UtcNow).TotalSeconds;

        if (secondsRemaining > 0 && secondsRemaining <= secondsBeforeClose)
        {
            _lastAlertBarTime = currentBarTime;
            try { _player.Play(); }
            catch (Exception ex) { RetroVisLog.Error("Errore riproduzione alert sound: " + ex.Message); }
        }
    }

    public void Dispose() => _player?.Dispose();
}
