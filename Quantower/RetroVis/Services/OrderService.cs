using System;
using TradingPlatform.BusinessLayer;

namespace Ldazrz.RetroVis.Services;

/// <summary>
/// Places market orders with automatic SL/TP using shared position sizing logic.
/// </summary>
public class OrderService
{
    public void PlaceMarketOrder(Side side, Symbol symbol, Account account, double entryPrice, double tpPrice, double riskAmount)
    {
        if (account == null)
        {
            RetroVisLog.Error("OrderService: Account not selected, order not placed.");
            return;
        }

        var calc = PositionSizer.CalculatePositionSize(symbol, entryPrice, tpPrice, riskAmount);
        if (!calc.IsValid)
        {
            RetroVisLog.Error($"OrderService: {calc.ErrorMessage}");
            return;
        }

        double quantity = calc.Quantity;
        double slTicks = calc.SlTicks;
        double tpTicks = calc.TpTicks;

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

        RetroVisLog.Info($"OrderService: placing {side} {quantity} {symbol.Name} @ ~{entryPrice:F2} (SL {slTicks:F1} ticks, TP {tpTicks:F1} ticks, risk ${riskAmount})");
        Core.Instance.PlaceOrder(request);
    }
}
