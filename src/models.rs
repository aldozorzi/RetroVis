mod domain;
mod requests;
mod matching;
mod cache_keys;
mod app_state;

pub use domain::{RawCandle, NormalizedCandle, CandleStore, SessionConfig, Context};
pub use requests::{QueryRequest, ContextQueryRequest, AiAnalysisRequest};
pub use matching::{MatchOutcome, MatchResponse, MatchesResponse, AggregateData, CachedResult};
pub use cache_keys::{CandleCacheKey, ContextCacheKey};
pub use app_state::{AppState, AiAnalysisService};