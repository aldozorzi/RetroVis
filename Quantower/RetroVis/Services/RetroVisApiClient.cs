using System;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;
using Ldazrz.RetroVis.Models;

namespace Ldazrz.RetroVis.Services;

/// <summary>
/// Wraps all HTTP calls to the RetroVis Rust backend. Stateless besides the
/// HttpClient and the two resolved endpoint URLs.
/// </summary>
public class RetroVisApiClient : IDisposable
{
    private readonly HttpClient _http;
    private readonly string _searchUrl;
    private readonly string _contextUrl;

    public RetroVisApiClient(string serverUrl, TimeSpan timeout)
    {
        _http = new HttpClient { Timeout = timeout };
        var baseUrl = serverUrl.TrimEnd('/');
        _searchUrl = $"{baseUrl}/search";
        _contextUrl = $"{baseUrl}/context-search";
    }

    public Task<MatchesResponse> FetchMatchesAsync(QueryRequest request) =>
        PostAsync<QueryRequest, MatchesResponse>(_searchUrl, request);

    public Task<MatchesResponse> FetchContextMatchesAsync(ContextQueryRequest request) =>
        PostAsync<ContextQueryRequest, MatchesResponse>(_contextUrl, request);

    private async Task<TRes> PostAsync<TReq, TRes>(string url, TReq request) where TRes : class
    {
        try
        {
            string json = JsonSerializer.Serialize(request);
            RetroVisLog.Info($"RetroVisApiClient POST {url} with request: {json}");
            var content = new StringContent(json, Encoding.UTF8, "application/json");
            var response = await _http.PostAsync(url, content);

            if (!response.IsSuccessStatusCode) return null;

            string resJson = await response.Content.ReadAsStringAsync();
            return JsonSerializer.Deserialize<TRes>(resJson);
        }
        catch (Exception ex)
        {
            RetroVisLog.Error($"RetroVisApiClient error ({url}): {ex.Message}");
            return null;
        }
    }

    public void Dispose() => _http.Dispose();
}
