use axum::response::Html;

pub async fn visualize_handler() -> Html<String> {
    Html(r#"
<!DOCTYPE html>
<html>
<head>
    <title>RetroVis Historical Chart</title>
    <script src="https://unpkg.com/lightweight-charts/dist/lightweight-charts.standalone.production.js"></script>
    <style>
        body { background: #131722; margin: 0; padding: 0; overflow: hidden; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; }
        #chart { height: 100vh; width: 100vw; }
        #info { position: absolute; top: 15px; left: 15px; z-index: 10; color: #d1d4dc; background: rgba(28, 32, 48, 0.8); padding: 8px 12px; border-radius: 4px; border: 1px solid #363c4e; pointer-events: none; }
    </style>
</head>
<body>
    <div id="info">Caricamento...</div>
    <div id="chart"></div>
    <script>
        const urlParams = new URLSearchParams(window.location.search);
        const dateParam = urlParams.get('date');
        const symbolParam = urlParams.get('symbol') || 'NQ';
        const timeframeParam = urlParams.get('timeframe') || 'M5';
        
        if (dateParam) document.getElementById('info').innerText = `${symbolParam} - ${dateParam}`;

        const chart = LightweightCharts.createChart(document.getElementById('chart'), {
            layout: { background: { color: '#131722' }, textColor: '#d1d4dc' },
            grid: { vertLines: { color: '#2b2e3a' }, horzLines: { color: '#2b2e3a' } },
            crosshair: {
                mode: LightweightCharts.CrosshairMode.Normal,
            },
            timeScale: { timeVisible: true, secondsVisible: false, borderColor: '#485c7b' },
            rightPriceScale: { borderColor: '#485c7b' },
            timeScale: {
                timeVisible: true,
                secondsVisible: false,
            },
        });

        const candleSeries = chart.addSeries(LightweightCharts.CandlestickSeries, {
            upColor: '#26a69a', 
            downColor: '#ef5350', 
            borderVisible: false,
            wickUpColor: '#26a69a', 
            wickDownColor: '#ef5350'
        });

        console.log(`/day-details?symbol=${symbolParam}&date=${dateParam}&timeframe=${timeframeParam}`);
        fetch(`/day-details?symbol=${symbolParam}&date=${dateParam}&timeframe=${timeframeParam}`)
            .then(res => res.json())
            .then(data => {
                if (!data || data.length === 0) {
                    document.getElementById('info').innerText = "Dati non trovati per: " + dateParam;
                    return;
                }
                
                const formattedData = data.map(d => {
                    return {
                        time: Number(d.timestamp), 
                        open: d.open, 
                        high: d.high, 
                        low: d.low, 
                        close: d.close
                    };
                }).sort((a, b) => a.time - b.time);
                
                candleSeries.setData(formattedData);

                const dateGroups = {};
                formattedData.forEach(d => {
                    const dateKey = new Date(d.time * 1000).toISOString().slice(0, 10);
                    if (!dateGroups[dateKey]) dateGroups[dateKey] = [];
                    dateGroups[dateKey].push(d);
                });

                markers = [];
                Object.keys(dateGroups).forEach(dateKey => {
                    const dayCandles = dateGroups[dateKey];

                    const nyOpenCandle = dayCandles.find(d => {
                        const date = new Date(d.time * 1000);
                        return date.getUTCHours() === 9 && date.getUTCMinutes() === 30;
                    });

                    const londonOpenTime = getLondonOpenInNY(dateKey);
                    const londonOpenCandle = dayCandles.find(d => {
                        const date = new Date(d.time * 1000);
                        const timeStr = date.getUTCHours().toString().padStart(2, '0') + ":" +
                                        date.getUTCMinutes().toString().padStart(2, '0') + ":00";
                        return timeStr === londonOpenTime;
                    });

                    if (londonOpenCandle) {
                        markers.push({
                            time: londonOpenCandle.time,
                            position: 'aboveBar',
                            color: '#f6d32d',
                            shape: 'arrowDown',
                            text: 'LONDON OPEN',
                            size: 2
                        });
                    }

                    if (nyOpenCandle) {
                        markers.push({
                            time: nyOpenCandle.time,
                            position: 'aboveBar',
                            color: '#4caf50',
                            shape: 'arrowDown',
                            text: 'NY OPEN',
                            size: 2
                        });
                    }
                });

                markers.sort((a, b) => a.time - b.time);
                LightweightCharts.createSeriesMarkers(candleSeries, markers);
                const secondDayStart = formattedData.find(d => {
                    const dateKey = new Date(d.time * 1000).toISOString().slice(0, 10);
                    return dateKey === dateParam;
                });

                if (secondDayStart) {
                    chart.timeScale().setVisibleRange({
                        from: secondDayStart.time - 1800, // 30 min prima dell'apertura del secondo giorno
                        to: formattedData[formattedData.length - 1].time
                    });
                } else {
                    chart.timeScale().fitContent();
                }
            })
            .catch(err => {
                document.getElementById('info').innerText = "Errore di connessione al server Rust";
                console.error(err);
            });

            function getLondonOpenInNY(dateStr) {
                // 1. Creiamo un istante temporale "neutro" (UTC) per le 08:00 di quel giorno
                const baseDate = new Date(`${dateStr}T08:00:00Z`);

                // 2. Funzione per ottenere l'offset in minuti di un fuso rispetto a UTC
                const getOffset = (tz, date) => {
                    const parts = new Intl.DateTimeFormat('en-US', {
                        timeZone: tz,
                        timeZoneName: 'short',
                        year: 'numeric', month: 'numeric', day: 'numeric',
                        hour: 'numeric', minute: 'numeric', second: 'numeric',
                        hour12: false
                    }).formatToParts(date);
                    
                    // Calcoliamo la differenza tra l'ora "trasformata" e quella UTC
                    const dt = new Date(date.toLocaleString('en-US', { timeZone: tz }));
                    return (dt - date) / 60000;
                };

                // 3. Calcoliamo gli offset reali di Londra e NY rispetto a UTC in quel momento
                const londonOffset = getOffset('Europe/London', baseDate);
                const nyOffset = getOffset('America/New_York', baseDate);

                // 4. Se Londra è a +0 (GMT) e NY è a -4 (EDT), la differenza è 4 ore.
                // L'apertura di Londra (08:00) meno la differenza ci dà l'ora di NY.
                const diffHours = (londonOffset - nyOffset) / 60;
                const targetHour = 8 - diffHours;

                // Formattiamo il risultato come HH:MM:00
                return `${String(Math.floor(targetHour)).padStart(2, '0')}:00:00`;
            }
    </script>
</body>
</html>
"#.to_string())
}
