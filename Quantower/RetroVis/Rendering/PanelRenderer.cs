using System;
using System.Collections.Generic;
using System.Drawing;
using System.Runtime.Versioning;
using TradingPlatform.BusinessLayer.Chart;
using Ldazrz.RetroVis.Models;

namespace Ldazrz.RetroVis.Rendering;

public readonly record struct MatchClickArea(RectangleF Rect, string DateStr);

public readonly record struct PanelState(
    string EffectiveSession,
    int CurrentCandleCount,
    int MinCandles,
    double CurrentDelta,
    AggregateData CandlesSummary,
    NextCandleData NextCandle,
    IReadOnlyList<MatchResponse> CandlesResults,
    AggregateData ContextSummary,
    IReadOnlyList<MatchResponse> ContextResults,
    bool IsLoadingNewAnalysis,
    double PositionSizeLong,
    double PositionSizeShort,
    bool HasValidPositionSize
);

public readonly record struct PanelDrawResult(
    int PanelHeight,
    IReadOnlyList<MatchClickArea> MatchAreas,
    IReadOnlyList<MatchClickArea> ContextAreas
);

/// <summary>
/// Pure renderer: takes a Graphics surface, a rectangle origin, and a
/// PanelState snapshot. Touches no Indicator API and holds no mutable state
/// of its own — click areas are returned, not accumulated.
/// </summary>
[SupportedOSPlatform("windows")]
public class PanelRenderer
{
    private readonly Font _dataFont = new("Consolas", 9);
    private readonly Font _headerFont = new("Consolas", 11, FontStyle.Bold);
    private readonly Font _btnFont = new("Arial", 7, FontStyle.Bold);
    private readonly SolidBrush _panelBgBrush = new(Color.FromArgb(220, 10, 10, 10));
    private readonly SolidBrush _matchBgBrush = new(Color.FromArgb(0, 180, 100, 30));

    public void DrawToggleButtons(Graphics gr, Rectangle toggleRect, Rectangle backtestBtnRect, bool isPanelVisible, bool isBacktestMode)
    {
        gr.FillRectangle(isPanelVisible ? Brushes.IndianRed : Brushes.SteelBlue, toggleRect);
        string label = isPanelVisible ? "HIDE PANEL" : "SHOW PANEL";
        float titleWidth = gr.MeasureString(label, _btnFont).Width;
        gr.DrawString(label, _btnFont, Brushes.White, toggleRect.X + (float)Math.Round((toggleRect.Width - titleWidth) / 2), toggleRect.Y + 7);

        gr.FillRectangle(isBacktestMode ? Brushes.Orange : Brushes.DimGray, backtestBtnRect);
        string btLabel = isBacktestMode ? "GO TO REAL TIME MODE" : " GO TO ANALYSIS MODE";
        float labelWidth = gr.MeasureString(btLabel, _btnFont).Width;
        gr.DrawString(btLabel, _btnFont, Brushes.White, backtestBtnRect.X + (float)Math.Round((backtestBtnRect.Width - labelWidth) / 2), backtestBtnRect.Y + 7);
    }

    public void DrawOrderButtons(Graphics gr, Rectangle longRect, Rectangle shortRect)
    {
        gr.FillRectangle(Brushes.LimeGreen, longRect);
        string longLabel = "BUY";
        float longWidth = gr.MeasureString(longLabel, _btnFont).Width;
        gr.DrawString(longLabel, _btnFont, Brushes.Black, longRect.X + (float)Math.Round((longRect.Width - longWidth) / 2), longRect.Y + 10);

        gr.FillRectangle(Brushes.Crimson, shortRect);
        string shortLabel = "SELL";
        float shortWidth = gr.MeasureString(shortLabel, _btnFont).Width;
        gr.DrawString(shortLabel, _btnFont, Brushes.White, shortRect.X + (float)Math.Round((shortRect.Width - shortWidth) / 2), shortRect.Y + 10);
    }

    public PanelDrawResult DrawResultsPanel(Graphics gr, Point origin, int panelWidth, int lastPanelHeight, in PanelState state)
    {
        var matchAreas = new List<MatchClickArea>();
        var contextAreas = new List<MatchClickArea>();

        Rectangle panelRect = new(origin.X, origin.Y, panelWidth, lastPanelHeight);
        gr.FillRectangle(_panelBgBrush, panelRect);

        int tx = panelRect.X + 12, ty = panelRect.Y + 12;

        // Show only the position size that matches the next candle edge bias
        string positionText = "";
        if (state.HasValidPositionSize && state.CandlesResults.Count > 0)
        {
            bool isLongBias = state.NextCandle.AvgUp > Math.Abs(state.NextCandle.AvgDown);
            positionText = isLongBias 
                ? $" SIZE: L:{state.PositionSizeLong:F0}"
                : $" SIZE: S:{state.PositionSizeShort:F0}";
        }
        Brush positionBrush = Brushes.Orange;

        string dynamicTitle;
        if (state.EffectiveSession != null)
        {
            string candlesInfo = state.CurrentCandleCount < state.MinCandles
                ? $"Waiting for {(state.MinCandles > 1 ? state.MinCandles.ToString() : "FIRST")} candle{(state.MinCandles > 1 ? "s" : "")}"
                : $"{state.CurrentCandleCount} candles";
            dynamicTitle = $"{state.EffectiveSession} ({candlesInfo})";
        }
        else
        {
            dynamicTitle = "NO ACTIVE SESSION";
            gr.DrawString(dynamicTitle, _headerFont, Brushes.Orange, tx, ty);
            return new PanelDrawResult(ty - panelRect.Y + 30, matchAreas, contextAreas);
        }

        gr.DrawString(dynamicTitle, _headerFont, Brushes.Orange, tx, ty);
        float titleWidth = gr.MeasureString(dynamicTitle, _headerFont).Width;
        if (state.CandlesResults.Count > 0 && state.HasValidPositionSize)
            gr.DrawString(positionText, _headerFont, positionBrush, tx + titleWidth + 10, ty);

        ty += 32;
        gr.DrawString("CANDLE MATCHES", _headerFont, Brushes.Orange, tx, ty);
        ty += 18;

        if (state.CurrentCandleCount < state.MinCandles)
        {
            gr.DrawString(
                $"WAITING FOR {(state.MinCandles > 1 ? state.MinCandles.ToString() : "FIRST")} CLOSED CANDLE{(state.MinCandles > 1 ? "S" : "")} ",
                _dataFont, Brushes.Gray, tx, ty + 5);
            ty += 18;
        }
        else if (state.CandlesResults == null || state.CandlesResults.Count == 0)
        {
            gr.DrawString("WAITING FOR DATA...", _dataFont, Brushes.Gray, tx, ty + 5);
            ty += 18;
        }
        else
        {
            ty = DrawSummaryRow(gr, tx, ty, panelRect.Width, "SESS", state.CandlesSummary.AvgUp, state.CandlesSummary.AvgDown, state.CandlesSummary.EdgeRatio, state.CandlesSummary.Confidence, withBorder: true);
            ty = DrawSummaryRow(gr, tx, ty, panelRect.Width, "NXT", state.NextCandle.AvgUp, state.NextCandle.AvgDown, state.NextCandle.EdgeRatio, state.NextCandle.Confidence, withBorder: false);
            ty += 6;

            ty = DrawMatchRows(gr, tx, ty, panelRect.Width, state.CandlesResults, matchAreas);
        }

        if (state.ContextResults != null && state.ContextResults.Count > 0)
        {
            ty += 10;
            gr.DrawString("CONTEXT MATCHES", _headerFont, Brushes.Orange, tx, ty);
            ty += 18;

            Brush ctxEdgeBrush = EdgeBrushFor(state.ContextSummary.EdgeRatio, state.ContextSummary.Confidence, state.ContextSummary.AvgUp, state.ContextSummary.AvgDown);
            Rectangle ctxSummaryRect = new(tx, ty, panelRect.Width - 24, 25);
            gr.DrawRectangle(Pens.DimGray, ctxSummaryRect);

            string ctxSummaryText = $"SESS U:+{state.ContextSummary.AvgUp:F1} | D:-{state.ContextSummary.AvgDown:F1} | ";
            gr.DrawString(ctxSummaryText, _dataFont, Brushes.LightGray, tx + 5, ty + 5);
            float ctxSummaryWidth = gr.MeasureString(ctxSummaryText, _dataFont).Width;
            string ctxEdgeText = $"E: {(Math.Abs(state.ContextSummary.AvgDown) > state.ContextSummary.AvgUp ? "-" : "+")}{state.ContextSummary.EdgeRatio:F2} ({state.ContextSummary.Confidence:F1}%)";
            gr.DrawString(ctxEdgeText, _dataFont, ctxEdgeBrush, tx + 5 + ctxSummaryWidth, ty + 5);

            ty += 35;

            ty = DrawMatchRows(gr, tx, ty, panelRect.Width, state.ContextResults, contextAreas);
        }

        int finalHeight = ty - panelRect.Y + 10;
        return new PanelDrawResult(finalHeight, matchAreas, contextAreas);
    }

    private int DrawMatchRows(Graphics gr, int tx, int ty, int panelWidth, IReadOnlyList<MatchResponse> matches, List<MatchClickArea> areas)
    {
        foreach (var m in matches)
        {
            var rect = new RectangleF(tx, ty, panelWidth - 24, 18);
            gr.FillRectangle(_matchBgBrush, rect);

            DateTime matchDt = DateTimeOffset.FromUnixTimeSeconds(m.Timestamp).DateTime;
            string dateStr = matchDt.ToString("yyyy-MM-dd");
            areas.Add(new MatchClickArea(rect, dateStr));

            gr.DrawString($"{dateStr} (MATCH:{m.Similarity:F1}%)", _dataFont, Brushes.Gray, tx, ty);
            ty += 18;
        }
        return ty;
    }

    private int DrawSummaryRow(Graphics gr, int tx, int ty, int panelWidth, string label, float avgUp, float avgDown, float edgeRatio, float confidence, bool withBorder)
    {
        if (withBorder)
        {
            Rectangle rect = new(tx, ty, panelWidth - 24, 45);
            gr.DrawRectangle(Pens.DimGray, rect);
        }

        Brush edgeBrush = EdgeBrushFor(edgeRatio, confidence, avgUp, avgDown);
        string summaryText = $"{label} U:+{avgUp:F1} | D:-{avgDown:F1} | ";
        gr.DrawString(summaryText, _dataFont, Brushes.White, tx + 5, ty + 5);
        float summaryWidth = gr.MeasureString(summaryText, _dataFont).Width;
        string edgeText = $"E:{(Math.Abs(avgDown) > avgUp ? "-" : "+")}{edgeRatio:F2} ({confidence:F1}%)";
        gr.DrawString(edgeText, _dataFont, edgeBrush, tx + 5 + summaryWidth, ty + 5);

        return ty + (withBorder ? 24 : 30);
    }

    private static Brush EdgeBrushFor(float edgeRatio, float confidence, float avgUp, float avgDown)
    {
        if (/*edgeRatio > 2.0 && */Math.Abs(confidence) >= 70)
            return avgUp > Math.Abs(avgDown) ? Brushes.LimeGreen : Brushes.Red;
        if (/*edgeRatio > 1.2*/Math.Abs(confidence) >= 60)
            return Brushes.Gold;
        return Brushes.White;
    }

    public void DrawBlinkingCircle(Graphics gr, Rectangle chartRect, bool edgePositive)
    {
        int circleSize = 24;
        int margin = 15;
        int x = chartRect.X + chartRect.Width - circleSize - margin;
        int y = chartRect.Y + margin;

        Color fillColor = edgePositive ? Color.FromArgb(200, 0, 200, 0) : Color.FromArgb(200, 200, 0, 0);
        Color borderColor = edgePositive ? Color.LimeGreen : Color.Red;

        using var fillBrush = new SolidBrush(fillColor);
        using var borderPen = new Pen(borderColor, 2.5f);

        gr.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
        gr.FillEllipse(fillBrush, x, y, circleSize, circleSize);
        gr.DrawEllipse(borderPen, x, y, circleSize, circleSize);
        gr.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.Default;
    }

    public void DrawLevels(
        Graphics gr,
        IChartWindow mainWindow,
        DateTime sessionStartTime,
        DateTime nextCandleBaseTime,
        double sessionUp,
        double sessionDown,
        double nextCandleUp,
        double nextCandleDown,
        bool drawSessionLevels,
        bool drawNextCandleLevels)
    {
        if (!drawSessionLevels && !drawNextCandleLevels) return;

        int rightX = mainWindow.ClientRectangle.Right;
        int startXCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartX(sessionStartTime));

        if (drawSessionLevels)
        {
            int upYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(sessionUp));
            int downYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(sessionDown));
            using var upPen = new Pen(Color.LimeGreen, 2);
            using var downPen = new Pen(Color.Sienna, 2);
            gr.DrawLine(upPen, startXCoord, upYCoord, rightX, upYCoord);
            gr.DrawLine(downPen, startXCoord, downYCoord, rightX, downYCoord);
        }

        if (drawNextCandleLevels)
        {
            int upYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(nextCandleUp));
            int downYCoord = (int)Math.Round(mainWindow.CoordinatesConverter.GetChartY(nextCandleDown));

            using var dashedUpPen = new Pen(Color.Turquoise, 2f) { DashStyle = System.Drawing.Drawing2D.DashStyle.Dash };
            gr.DrawLine(dashedUpPen, startXCoord, upYCoord, rightX, upYCoord);

            using var dashedDownPen = new Pen(Color.LightGoldenrodYellow, 2f) { DashStyle = System.Drawing.Drawing2D.DashStyle.Dash };
            gr.DrawLine(dashedDownPen, startXCoord, downYCoord, rightX, downYCoord);
        }
    }
}
