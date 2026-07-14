use std::path::PathBuf;

use axum::http::HeaderMap;

#[derive(Clone)]
pub struct AppState {
    pub app_dir: PathBuf,
}

pub struct Ctx {
    pub state: AppState,
    pub headers: HeaderMap,
}
