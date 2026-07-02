using System;
using TradingPlatform.BusinessLayer;

namespace Ldazrz.RetroVis.Services;

/// <summary>
/// Pure position sizing calculations shared between OrderService and UI display.
/// Sizing logic: risk is applied to half the distance to TP, so a full TP hit
/// nets ~2x the risk amount (RR ~1:2). Quantity is floored, never below 1.
/// </summary>
public static class PositionSizer
{
    public readonly record struct SizeCalculation(
        double Quantity,
        double SlTicks,
        double TpTicks,
        bool IsValid,
        string ErrorMessage = null
    );

    /// <summary>
    /// Calculates position size for a given entry, TP level, and risk amount.
    /// Returns SizeCalculation with IsValid=false if inputs are invalid.
    /// </summary>
    public static SizeCalculation CalculatePositionSize(
        Symbol symbol,
        double entryPrice,
        double tpPrice,
        double riskAmount)
    {
        if (symbol == null)
            return new SizeCalculation(0, 0, 0, false, "Symbol is null");

        double distancePoints = Math.Abs(tpPrice - entryPrice);
        if (distancePoints <= 0)
            return new SizeCalculation(0, 0, 0, false, "Invalid TP distance (<=0)");

        double sizingDistance = distancePoints / 2.0;

        double slTicks = symbol.CalculateTicks(entryPrice, entryPrice + sizingDistance);
        double tpTicks = symbol.CalculateTicks(entryPrice, entryPrice + distancePoints);
        slTicks = Math.Abs(slTicks);
        tpTicks = Math.Abs(tpTicks);

        double tickCost = symbol.GetTickCost(entryPrice);
        if (slTicks <= 0 || tickCost <= 0)
            return new SizeCalculation(0, slTicks, tpTicks, false, $"Invalid tick data (slTicks={slTicks}, tickCost={tickCost})");

        double rawQuantity = riskAmount / (slTicks * tickCost);
        double quantity = Math.Max(1, Math.Floor(rawQuantity));

        return new SizeCalculation(quantity, slTicks, tpTicks, true);
    }
}
