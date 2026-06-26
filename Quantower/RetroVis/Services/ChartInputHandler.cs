using System;
using System.Collections.Generic;
using System.Drawing;
using System.Runtime.Versioning;
using TradingPlatform.BusinessLayer;
using TradingPlatform.BusinessLayer.Chart;
using Ldazrz.RetroVis.Rendering;

namespace Ldazrz.RetroVis.Services;

public enum InputAction { None, TogglePanel, ToggleBacktest, RunManualAnalysis }

public readonly record struct InputResult(InputAction Action, int CandleIndex = -1);

/// <summary>
/// Handles chart mouse input: panel/backtest toggle buttons, match-area
/// clicks (open visualize URL), and backtest candle-click selection.
/// Holds no business state of its own beyond the mouse-down flag — reads
/// click-areas and rects passed per call, returns an InputResult for the
/// caller to act on.
/// </summary>
[SupportedOSPlatform("windows")]
public class ChartInputHandler
{
    private readonly HistoricalData _data;
    private bool _mouseDownReceived;

    public ChartInputHandler(HistoricalData data)
    {
        _data = data;
    }

    public void OnMouseDown() => _mouseDownReceived = true;

    public InputResult OnMouseUp(
        ChartMouseNativeEventArgs e,
        Rectangle toggleRect,
        Rectangle backtestBtnRect,
        bool isPanelVisible,
        bool isBacktestMode,
        IReadOnlyList<MatchClickArea> matchAreas,
        IReadOnlyList<MatchClickArea> contextAreas,
        string ticker,
        string timeframe)
    {
        if (!_mouseDownReceived) return new InputResult(InputAction.None);
        _mouseDownReceived = false;

        if (toggleRect.Contains(e.Location))
            return new InputResult(InputAction.TogglePanel);

        if (backtestBtnRect.Contains(e.Location))
            return new InputResult(InputAction.ToggleBacktest);

        if (isPanelVisible)
        {
            string clickedDate = FindClickedDate(matchAreas, e.Location) ?? FindClickedDate(contextAreas, e.Location);
            if (clickedDate != null)
            {
                OpenVisualizeUrl(ticker, clickedDate, timeframe);
                return new InputResult(InputAction.None);
            }
        }

        if (isBacktestMode)
        {
            DateTime clickTime = e.Window.CoordinatesConverter.GetTime(e.X);
            int candleIndex = GetIndexByTime(clickTime);
            return new InputResult(InputAction.RunManualAnalysis, candleIndex);
        }

        return new InputResult(InputAction.None);
    }

    private static string FindClickedDate(IReadOnlyList<MatchClickArea> areas, Point location)
    {
        foreach (var area in areas)
        {
            if (area.Rect.Contains(location))
                return area.DateStr;
        }
        return null;
    }

    private static void OpenVisualizeUrl(string ticker, string date, string timeframe)
    {
        string url = $"http://127.0.0.1:9999/visualize?symbol={ticker}&date={date}&timeframe={timeframe}";
        try
        {
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo { FileName = url, UseShellExecute = true });
        }
        catch (Exception ex)
        {
            RetroVisLog.Error("Errore apertura browser: " + ex.Message);
        }
    }

    private int GetIndexByTime(DateTime clickedTime)
    {
        for (int i = 0; i < _data.Count; i++)
        {
            if (_data.Time(i) <= clickedTime)
                return Math.Max(0, i - 1);
        }
        return -1;
    }
}