use std::collections::HashMap;
use std::sync::Arc;
use crate::engine::SearchEngine;
use crate::models::{SessionConfig};

pub struct AppState {
    pub engines: HashMap<String, SearchEngine>,
    pub sessions: HashMap<String, SessionConfig>,
    pub cache: Arc<crate::cache::RetroVisCache>,
}