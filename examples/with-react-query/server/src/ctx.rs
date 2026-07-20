use axum::http::HeaderMap;
use std::path::PathBuf;

#[derive(Clone)]
pub struct AppState {
    pub app_dir: PathBuf,
}

pub struct Ctx {
    pub state: AppState,
    pub headers: HeaderMap,
}
