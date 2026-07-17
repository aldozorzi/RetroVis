using System;
using System.Collections.Generic;
using TradingPlatform.BusinessLayer;
using Ldazrz.RetroVis.Models;

namespace Ldazrz.RetroVis.Services;

public readonly record struct PrevDayStats(float PrevClose, float PrevHigh, float PrevLow, bool Found);

public readonly record struct IntradayRange(float High, float Low);

/// <summary>
/// Reads bars from the chart's HistoricalData in a session-aware way. This is
/// the single class allowed to call bar-access methods (Time/Open/High/
/// Low/Close) — everything downstream works with plain data.
/// </summary>
public class SessionBarReader
{
    private readonly HistoricalData _data;
    private readonly TimeZoneInfo _nyTz;
    private readonly TimeZoneInfo _londonTz;

    public SessionBarReader(HistoricalData data, TimeZoneInfo nyTz, TimeZoneInfo londonTz)
    {
        _data = data;
        _nyTz = nyTz;
        _londonTz = londonTz;
    }

    /// <summary>
    /// AUTO session resolution based on London/NY local time at refIndex.
    /// Returns null if no session is active (outside both windows).
    /// </summary>
    public string ResolveAutoSession(int refIndex)
    {
        DateTime referenceTimeUtc = _data.Time(refIndex).ToUniversalTime();
        DateTime londonNow = TimeZoneInfo.ConvertTimeFromUtc(referenceTimeUtc, _londonTz);
        DateTime nyNow = TimeZoneInfo.ConvertTimeFromUtc(referenceTimeUtc, _nyTz);

        int londonTime = londonNow.Hour * 100 + londonNow.Minute;
        int nyTime = nyNow.Hour * 100 + nyNow.Minute;

        if (londonTime >= 800 && londonTime < 1425 && nyTime < 930) return "LONDON_OPEN";
        if (nyTime >= 930 && nyTime < 1705) return "NY_OPEN";
        return null;
    }

    /// <summary>
    /// Searches forward from refIndex (up to 200 bars) for the bar matching
    /// the session's open time in its own timezone. Returns -1 if not found.
    /// News-event sessions (CPI, PPI, NFP, FOMC) use NY time as their native
    /// timezone; LONDON_OPEN uses London time; NY_OPEN uses NY time.
    /// </summary>
    public int FindSessionOpenBarIndex(int refIndex, string sessionId, int chartCount)
    {
        int targetHour, targetMinute;
        TimeZoneInfo sessionTz;

        switch (sessionId)
        {
            case "CPI":
            case "PPI":
            case "NFP":
                targetHour = 8; targetMinute = 30; sessionTz = _nyTz;
                break;
            case "FOMC":
                targetHour = 14; targetMinute = 0; sessionTz = _nyTz;
                break;
            case "NY_OPEN":
                targetHour = 9; targetMinute = 30; sessionTz = _nyTz;
                break;
            case "LONDON_OPEN":
                targetHour = 8; targetMinute = 0; sessionTz = _londonTz;
                break;
            default:
                return -1;
        }

        for (int i = refIndex; i < Math.Min(chartCount, refIndex + 200); i++)
        {
            DateTime barInSessionTime = TimeZoneInfo.ConvertTimeFromUtc(_data.Time(i).ToUniversalTime(), sessionTz);
            if (barInSessionTime.Hour == targetHour && barInSessionTime.Minute == targetMinute)
                return i;
        }
        return -1;
    }

    private DateTime BarTimeNy(int i) => TimeZoneInfo.ConvertTimeFromUtc(_data.Time(i).ToUniversalTime(), _nyTz);

    /// <summary>
    /// Builds the candle slice from openBarIndex back to refIndex (capped at
    /// maxCandles), in NY-localized timestamps, exactly as ProcessAnalysis did.
    /// </summary>
    public List<RawCandle> ExtractCandles(int openBarIndex, int refIndex, int maxCandles)
    {
        var result = new List<RawCandle>();
        int limit = Math.Min(openBarIndex - refIndex, maxCandles);

        for (int i = openBarIndex; i > refIndex && i > (openBarIndex - limit); i--)
        {
            result.Add(new RawCandle
            {
                Timestamp = (uint)new DateTimeOffset(BarTimeNy(i), TimeSpan.Zero).ToUnixTimeSeconds(),
                Open = (float)_data.Open(i),
                High = (float)_data.High(i),
                Low = (float)_data.Low(i),
                Close = (float)_data.Close(i),
                Volume = (float)_data.Volume(i)
            });
        }
        return result;
    }

    /// <summary>
    /// Intraday high/low from session open up to (but not including) the next
    /// calendar day in NY time, scanning at most 72 bars forward.
    /// </summary>
    public IntradayRange ExtractIntradayRange(int openBarIndex, int chartCount)
    {
        float sessionOpen = (float)_data.Open(openBarIndex);
        float high = sessionOpen;
        float low = sessionOpen;
        DateTime openBarDate = BarTimeNy(openBarIndex).Date;

        for (int i = openBarIndex + 1; i < Math.Min(chartCount, openBarIndex + 72); i++)
        {
            if (BarTimeNy(i).Date != openBarDate) break;
            if ((float)_data.High(i) > high) high = (float)_data.High(i);
            if ((float)_data.Low(i) < low) low = (float)_data.Low(i);
        }
        return new IntradayRange(high, low);
    }

    /// <summary>
    /// Previous-day close/high/low ahead of the given session-close window,
    /// scanning back up to 1000 bars. closeHour/closeMinute mark the boundary
    /// used to define "previous trading session" (currently 17:00 NY for all
    /// session types, mirroring the original BuildContextQueryRequest).
    /// </summary>
    public PrevDayStats ExtractPrevDayStats(int openBarIndex, int chartCount, int closeHour, int closeMinute)
    {
        DateTime openBarDate = BarTimeNy(openBarIndex).Date;

        DateTime currentSessionOpen = new(openBarDate.Year, openBarDate.Month, openBarDate.Day, closeHour, closeMinute, 0);
        DateTime prevSessionEnd = PrevTradingDayAt(currentSessionOpen, closeHour, closeMinute);
        DateTime prevSessionStart = PrevTradingDayAt(prevSessionEnd, closeHour, closeMinute);

        float prevClose = 0, prevHigh = float.MinValue, prevLow = float.MaxValue;
        bool found = false;

        for (int i = openBarIndex + 1; i < Math.Min(chartCount, openBarIndex + 1000); i++)
        {
            DateTime barNy = BarTimeNy(i);

            if (barNy >= prevSessionEnd) continue;
            if (barNy < prevSessionStart) break;

            if (!found)
            {
                prevClose = (float)_data.Close(i);
                found = true;
            }
            if ((float)_data.High(i) > prevHigh) prevHigh = (float)_data.High(i);
            if ((float)_data.Low(i) < prevLow) prevLow = (float)_data.Low(i);
        }

        return new PrevDayStats(prevClose, prevHigh, prevLow, found);
    }

    private static DateTime PrevTradingDayAt(DateTime reference, int hour, int minute)
    {
        DateTime prevDate = PrevTradingDay(reference);
        return new DateTime(prevDate.Year, prevDate.Month, prevDate.Day, hour, minute, 0);
    }

    private static DateTime PrevTradingDay(DateTime date)
    {
        DateTime prev = date.AddDays(-1);
        while (prev.DayOfWeek == DayOfWeek.Saturday || prev.DayOfWeek == DayOfWeek.Sunday)
            prev = prev.AddDays(-1);
        return prev;
    }
}