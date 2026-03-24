use crate::state::KernelState;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

pub async fn auth_middleware(
    State(state): State<Arc<KernelState>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // 1. Extract the Authorization header
    if let Some(auth_header) = headers.get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if auth_str == format!("Bearer {}", state.auth_token) {
                return Ok(next.run(request).await);
            }
        }
    }

    println!("-> [SECURITY ALERT] Blocked unauthorized network connection attempt!");
    Err(StatusCode::UNAUTHORIZED)
}
