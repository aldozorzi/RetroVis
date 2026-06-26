using TradingPlatform.BusinessLayer;

namespace Ldazrz.RetroVis.Services;

public static class RetroVisLog
{
    public static void Info(string message) =>
        Core.Instance.Loggers.Log($"[RetroVis] {message}", LoggingLevel.Trading);

    public static void Error(string message) =>
        Core.Instance.Loggers.Log($"[RetroVis] {message}", LoggingLevel.Trading);
}
