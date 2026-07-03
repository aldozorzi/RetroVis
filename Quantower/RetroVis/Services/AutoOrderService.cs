using System;
using System.Media;
using System.Runtime.Versioning;
using TradingPlatform.BusinessLayer;
using Ldazrz.RetroVis.Models;

namespace Ldazrz.RetroVis.Services;

/// <summary>
/// Automatically places limit orders when high-confidence signals align.
/// Rules:
/// 1. NextCandleData.Confidence > 90
/// 2. Session edge sign agrees with next candle edge sign
/// 3. Uses next candle levels for entry/TP/SL
/// </summary>
[SupportedOSPlatform("windows")]
public class AutoOrderService : IDisposable
{
    private readonly OrderService _orderService;
    private readonly string _alertSoundPath;
    private SoundPlayer _player;
    private bool _isDisposed;

    public AutoOrderService(OrderService orderService, string alertSoundPath)
    {
        _orderService = orderService;
        _alertSoundPath = alertSoundPath;
        LoadSound(alertSoundPath);
    }

    private void LoadSound(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
            return;

        try
        {
            _player = new SoundPlayer(path);
            _player.Load();
        }
        catch (Exception ex)
        {
            RetroVisLog.Error("AutoOrderService: Failed to load sound: " + ex.Message);
        }
    }

    private void PlaySound()
    {
        if (_player == null)
            return;

        try
        {
            _player.Play();
        }
        catch (Exception ex)
        {
            RetroVisLog.Error("AutoOrderService: Failed to play sound: " + ex.Message);
        }
    }

    /// <summary>
    /// Checks if auto-order conditions are met and places the order if so.
    /// Returns true if an order was placed.
    /// </summary>
    public bool TryPlaceAutoOrder(
        Symbol symbol,
        Account account,
        double riskAmount,
        AggregateData candlesSummary,
        NextCandleData nextCandle,
        (double Up, double Down) nextCandleLevels)
    {
        if (symbol == null || account == null || nextCandle == null)
            return false;

        // Rule 1: Next candle confidence > 90
        if (nextCandle.Confidence <= 90f)
            return false;

        // Rule 2: Session edge sign agrees with next candle edge sign
        bool sessionLongBias = candlesSummary.AvgUp > Math.Abs(candlesSummary.AvgDown);
        bool nextCandleLongBias = nextCandle.AvgUp > Math.Abs(nextCandle.AvgDown);
        
        if (sessionLongBias != nextCandleLongBias)
            return false;

        // Determine order side based on the shared bias
        Side side = sessionLongBias ? Side.Buy : Side.Sell;

        // Set entry, TP, and SL levels based on next candle levels
        double entryLevel = side == Side.Buy ? nextCandleLevels.Down : nextCandleLevels.Up;
        double tpLevel = side == Side.Buy ? nextCandleLevels.Up : nextCandleLevels.Down;

        // Place the limit order
        PlaceLimitOrder(side, symbol, account, entryLevel, tpLevel, riskAmount);
        PlaySound();

        return true;
    }

    private void PlaceLimitOrder(Side side, Symbol symbol, Account account, double entryPrice, double tpPrice, double riskAmount)
    {
        var calc = PositionSizer.CalculateSize(symbol, entryPrice, tpPrice, riskAmount);
        if (!calc.IsValid)
        {
            RetroVisLog.Error($"AutoOrderService: {calc.ErrorMessage}");
            return;
        }

        double quantity = calc.Quantity;
        double slTicks = calc.SlTicks;
        double tpTicks = calc.TpTicks;

        // Calculate SL offset from entry (half distance to TP)
        double slOffsetTicks = slTicks;

        var request = new PlaceOrderRequestParameters
        {
            Account = account,
            Symbol = symbol,
            OrderTypeId = OrderType.Limit,
            Side = side,
            Quantity = quantity,
            Price = entryPrice,
            StopLoss = SlTpHolder.CreateSL(slOffsetTicks, PriceMeasurement.Offset),
            TakeProfit = SlTpHolder.CreateTP(tpTicks, PriceMeasurement.Offset)
        };

        RetroVisLog.Info($"AutoOrderService: placing {side} {quantity} {symbol.Name} @ {entryPrice:F2} (SL {slOffsetTicks:F1} ticks, TP {tpTicks:F1} ticks, risk ${riskAmount})");
        Core.Instance.PlaceOrder(request);
    }

    public void Dispose()
    {
        if (_isDisposed) return;
        _player?.Dispose();
        _isDisposed = true;
        GC.SuppressFinalize(this);
    }
}