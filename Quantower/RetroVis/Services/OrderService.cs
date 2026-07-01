using System;
using TradingPlatform.BusinessLayer;

namespace Ldazrz.RetroVis.Services;

/// <summary>
/// Computes position size from a fixed dollar risk and places a market
/// order with automatic SL/TP. Sizing logic: risk is applied to half the
/// distance to TP, so a full TP hit nets ~2x the risk amount (RR ~1:2).
/// Quantity is floored, but never below 1 (intended for micro contracts).
/// </summary>
public class OrderService
{
    public void PlaceMarketOrder(Side side, Symbol symbol, Account account, double entryPrice, double tpPrice, double riskAmount)
    {
        if (symbol == null || account == null)
        {
            RetroVisLog.Error("OrderService: Symbol o Account non selezionato, ordine non piazzato.");
            return;
        }

        double distancePoints = Math.Abs(tpPrice - entryPrice);
        if (distancePoints <= 0)
        {
            RetroVisLog.Error("OrderService: distanza dal TP non valida (<=0), ordine non piazzato.");
            return;
        }

        double sizingDistance = distancePoints / 2.0;

        double slTicks = symbol.CalculateTicks(entryPrice, entryPrice + sizingDistance);
        double tpTicks = symbol.CalculateTicks(entryPrice, entryPrice + distancePoints);
        slTicks = Math.Abs(slTicks);
        tpTicks = Math.Abs(tpTicks);

        double tickCost = symbol.GetTickCost(entryPrice);
        if (slTicks <= 0 || tickCost <= 0)
        {
            RetroVisLog.Error($"OrderService: dati tick non validi (slTicks={slTicks}, tickCost={tickCost}), ordine non piazzato.");
            return;
        }

        double rawQuantity = riskAmount / (slTicks * tickCost);
        double quantity = Math.Max(1, Math.Floor(rawQuantity));

        var request = new PlaceOrderRequestParameters
        {
            Account = account,
            Symbol = symbol,
            OrderTypeId = OrderType.Market,
            Side = side,
            Quantity = quantity,
            StopLoss = SlTpHolder.CreateSL(slTicks, PriceMeasurement.Offset),
            TakeProfit = SlTpHolder.CreateTP(tpTicks, PriceMeasurement.Offset)
        };

        RetroVisLog.Info($"OrderService: piazzo {side} {quantity} {symbol.Name} @ ~{entryPrice} (SL {slTicks} ticks, TP {tpTicks} ticks, risk ${riskAmount})");
        Core.Instance.PlaceOrder(request);
    }
}
