using System;
using System.Collections.Generic;
using System.Drawing;
using System.Runtime.Versioning;
using Ldazrz.RetroVis.Models;

namespace Ldazrz.RetroVis.Rendering;

public readonly record struct TfClickArea(RectangleF Rect, string Timeframe);

public readonly record struct MultiTfDrawResult(
    int PanelHeight,
    IReadOnlyList<TfClickArea> ClickAreas
);

/// <summary>
/// Carries the per-timeframe data needed by the multi-timeframe panel renderer.
/// </summary>
public readonly record struct HigherTfDisplayData(
    string TimeframeLabel,
    AggregateData SessionSummary,
    NextCandleData NextCandle
);

/// <summary>
/// Renders the multi-timeframe panel at the top-right of the chart, replacing the
/// old single alert circle. Shows one vertical block per timeframe (current + higher),
/// each with percentage labels above and a colored circle below. The selected
/// timeframe circle is highlighted with a thick bright border.
/// </summary>
[SupportedOSPlatform("windows")]
public class MultiTimeframePanelRenderer : IDisposable
{
    private readonly Font _dataFont = new("Consolas", 8);
    private readonly Font _smallFont = new("Consolas", 7);
    private readonly SolidBrush _panelBgBrush = new(Color.FromArgb(200, 10, 10, 10));

    private const int CircleSize = 18;
    private const int BlockSpacing = 10;
    private const int LabelHeight = 16;
    private const int LabelTopPadding = 10;
    private const int CircleRowHeight = 22;
    private const int PanelPadding = 4;
    private const int PanelWidth = 115;

    /// <summary>
    /// Draws the multi-timeframe panel at the top-right of the chart.
    /// </summary>
    /// <param name="gr">Graphics surface</param>
    /// <param name="chartRect">Chart client rectangle (for positioning)</param>
    /// <param name="timeframes">Ordered list of timeframe display data (current first, then higher TFs)</param>
    /// <param name="selectedTimeframe">The currently selected timeframe label (e.g., "M5")</param>
    /// <returns>Draw result with panel height and click areas</returns>
    public MultiTfDrawResult DrawMultiTimeframePanel(
        Graphics gr,
        Rectangle chartRect,
        IReadOnlyList<HigherTfDisplayData> timeframes,
        string selectedTimeframe)
    {
        var clickAreas = new List<TfClickArea>();
        int count = timeframes.Count;
        if (count == 0)
            return new MultiTfDrawResult(0, clickAreas);

        int blockHeight = LabelTopPadding + LabelHeight + CircleRowHeight;
        int totalHeight = count * blockHeight + (count) * BlockSpacing + PanelPadding * 2;

        int margin = 50;
        int panelX = chartRect.X + chartRect.Width - PanelWidth -10;
        int panelY = chartRect.Y + margin;

        Rectangle panelRect = new(panelX, panelY, PanelWidth, totalHeight);
        gr.FillRectangle(this._panelBgBrush, panelRect);

        int currentY = panelY;

        foreach (var tf in timeframes)
        {
            float avgUp = tf.SessionSummary.AvgUp;
            float avgDown = tf.SessionSummary.AvgDown;
            float sessionConf = tf.SessionSummary.Confidence;
            float nextConf = tf.NextCandle.Confidence;
            bool isSelected = tf.TimeframeLabel == selectedTimeframe;

            // --- Draw percentage labels side by side ---
            float nextAvgUp = tf.NextCandle.AvgUp;
            float nextAvgDown = tf.NextCandle.AvgDown;

            Brush sessionBrush = EdgeBrushFor(sessionConf, avgUp, avgDown);
            Brush nextBrush = EdgeBrushFor(nextConf, nextAvgUp, nextAvgDown);

            string sessionArrow = avgUp > Math.Abs(avgDown) ? "↑" : "↓";
            string nextArrow = nextAvgUp > Math.Abs(nextAvgDown) ? "↑" : "↓";
            string sessionText = $"S:{sessionArrow}{sessionConf:F0}%";
            string nextText = $"N:{nextArrow}{nextConf:F0}%";

            int labelY = currentY + LabelTopPadding;
            int labelCenterX = panelX + PanelWidth / 2;

            // Measure session label width
            float sWidth = gr.MeasureString(sessionText, this._dataFont).Width;
            float nWidth = gr.MeasureString(nextText, this._dataFont).Width;
            float totalWidth = sWidth + 4 + nWidth; // 4px gap
            float startX = labelCenterX - totalWidth / 2;

            gr.DrawString(sessionText, this._dataFont, sessionBrush, startX, labelY);
            gr.DrawString(nextText, this._dataFont, nextBrush, startX + sWidth + 4, labelY);

            // --- Draw circle below ---
            int circleY = currentY + LabelHeight + LabelTopPadding + 2;

            // Determine circle fill color
            // Both session and next candle must agree on edge direction
            bool showStrongSignal = sessionConf >= 70f && nextConf >= 70f;
            bool sessionEdgePositive = avgUp > Math.Abs(avgDown);
            bool nextEdgePositive = tf.NextCandle.AvgUp > Math.Abs(tf.NextCandle.AvgDown);

            Color fillColor;
            if (showStrongSignal && sessionEdgePositive && nextEdgePositive)
                fillColor = Color.FromArgb(200, 0, 180, 0); // green
            else if (showStrongSignal && !sessionEdgePositive && !nextEdgePositive)
                fillColor = Color.FromArgb(200, 180, 0, 0); // red
            else
                fillColor = Color.FromArgb(100, 80, 80, 80); // dim gray (no strong signal)

            Color borderColor;
            float borderWidth;
            if (isSelected)
            {
                borderColor = Color.White;
                borderWidth = 1f;
            }
            else
            {
                borderColor = Color.FromArgb(120, 160, 160, 160);
                borderWidth = 1f;
            }

            int circleX = labelCenterX - CircleSize / 2;

            using var fillBrush = new SolidBrush(fillColor);
            using var borderPen = new Pen(borderColor, borderWidth);

            gr.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
            gr.FillEllipse(fillBrush, circleX, circleY, CircleSize, CircleSize);
            gr.DrawEllipse(borderPen, circleX, circleY, CircleSize, CircleSize);
            gr.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.Default;

            // Store click area for this timeframe's circle (plus a small hit margin)
            int hitMargin = 4;
            RectangleF clickRect = new(
                circleX - hitMargin,
                circleY - hitMargin,
                CircleSize + hitMargin * 2,
                CircleSize + hitMargin * 2);
            clickAreas.Add(new TfClickArea(clickRect, tf.TimeframeLabel));

            // Draw timeframe label faintly below the circle
            string tfLabel = tf.TimeframeLabel;
            float tfLabelWidth = gr.MeasureString(tfLabel, this._smallFont).Width;
            gr.DrawString(tfLabel, this._smallFont, Brushes.Gray, labelCenterX - tfLabelWidth / 2, circleY + CircleSize + 1);

            currentY += blockHeight + BlockSpacing;
        }

        int finalHeight = totalHeight;
        return new MultiTfDrawResult(finalHeight, clickAreas);
    }

    /// <summary>
    /// Matches the coloring logic from PanelRenderer.EdgeBrushFor:
    /// confidence >= 70% → green (positive edge) or red (negative edge)
    /// confidence >= 60% → gold/yellow
    /// else → white
    /// </summary>
    private static Brush EdgeBrushFor(float confidence, float avgUp, float avgDown)
    {
        if (Math.Abs(confidence) >= 70)
            return avgUp > Math.Abs(avgDown) ? Brushes.LimeGreen : Brushes.Red;
        if (Math.Abs(confidence) >= 60)
            return Brushes.Gold;
        return Brushes.White;
    }

    public void Dispose()
    {
        this._dataFont?.Dispose();
        this._smallFont?.Dispose();
        this._panelBgBrush?.Dispose();
        GC.SuppressFinalize(this);
    }
}
