use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    http::header,
    routing::{get, post, put},
    Router,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use crate::{manager::ServiceManager, service::{ServiceConfig, WindowsOptions}};

/// Constan source of Web
/// Index pages
/// Aria2ng pages
/// Favicon
const INDEX_HTML: &str = include_str!("../web/index.html");
const ARIANG_HTML: &str = include_str!("../web/ariang.html");
const FAVICON_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#2e7d32' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><rect x='2' y='2' width='20' height='8' rx='2' ry='2'></rect><rect x='2' y='14' width='20' height='8' rx='2' ry='2'></rect><line x1='6' y1='6' x2='6.01' y2='6'></line><line x1='6' y1='18' x2='6.01' y2='18'></line></svg>";

pub type SharedManager = Arc<Mutex<ServiceManager>>;

/// Service state structure
#[derive(Clone)]
pub struct AppState {
    pub manager: SharedManager,
    pub shutdown_tx: mpsc::Sender<()>,
}

/// Process yaml importe parsing
#[derive(Deserialize)]
struct ImportRequest {
    yaml: String,
}

/// Api response structure
#[derive(Serialize)]
pub struct ApiResponse<T> {
    success: bool,
    msg: Option<String>,
    data: Option<T>,
}

/// Service config & status
#[derive(Serialize)]
pub struct ServiceDto {
    // config values
    id: String,
    name: String,
    exec: String,
    args: Vec<String>,
    working_dir: Option<String>,
    autorun: bool,
    env: Option<HashMap<String, String>>,
    windows: Option<WindowsOptions>,
    url: Option<String>,
    // status values
    status: String,
    pid: Option<u32>,
}

/// Keep alive config
#[derive(Serialize, Deserialize)]
struct GlobalConfigDto {
    keep_alive: u64,
}

/// Reorder structure
#[derive(Deserialize)]
struct ReorderRequest {
    ids: Vec<String>,
}

/// API response
/// Ok & Error
fn resp_ok<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse { 
        success: true, 
        msg: None, 
        data: Some(data) 
    })
}
fn resp_err(msg: impl ToString) -> (StatusCode, Json<ApiResponse<()>>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiResponse { 
            success: false, 
            msg: Some(msg.to_string()), 
            data: None 
        }),
    )
}

/// Api router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index_page))
        .route("/favicon.svg", get(favicon_handler))
        .route("/ariang", get(ariang_page)) 
        .route("/api/shutdown", post(shutdown_handler))
        .route("/api/config", get(get_config).post(update_config))
        .route("/api/services", get(list_services).post(add_service))
        .route("/api/services/reorder", post(reorder_services))
        .route("/api/services/import", post(import_services))
        .route("/api/services/{id}", put(update_service).delete(delete_service))
        .route("/api/services/{id}/start", post(start_service))
        .route("/api/services/{id}/stop", post(stop_service))
        .route("/api/services/{id}/restart", post(restart_service))
        .with_state(state)
}

/// Embed static resource
/// Index page
async fn index_page() -> impl IntoResponse {
    Html(INDEX_HTML)
}
/// Aria2 NG page
async fn ariang_page() -> impl IntoResponse {
    Html(ARIANG_HTML)
}
/// Favicon
async fn favicon_handler() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/svg+xml")],
        FAVICON_SVG,
    )
}

/// Handle function
/// Handle: shutdown
async fn shutdown_handler(
    State(state): State<AppState>
) -> impl IntoResponse {
    let _ = state.shutdown_tx.try_send(());
    resp_ok("Server is shutting down...")
}
/// Handle: start
async fn start_service(
    State(state): State<AppState>, 
    Path(id): Path<String>
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().await;
    match mgr.start(&id).await {
        Ok(_) => resp_ok("Started").into_response(),
        Err(e) => resp_err(e).into_response(),
    }
}
/// Handle: stop
async fn stop_service(
    State(state): State<AppState>, 
    Path(id): Path<String>
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().await;
    match mgr.stop(&id).await {
        Ok(_) => resp_ok("Stopped").into_response(),
        Err(e) => resp_err(e).into_response(),
    }
}
/// Handle: restart
async fn restart_service(
    State(state): State<AppState>, 
    Path(id): Path<String>
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().await;
    match mgr.restart(&id).await {
        Ok(_) => resp_ok("Restarted").into_response(),
        Err(e) => resp_err(e).into_response(),
    }
}
/// Handle: list all services
async fn list_services(
    State(state): State<AppState>
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().await;
    
    let snapshots = mgr.list();

    let dtos: Vec<ServiceDto> = snapshots.into_iter().map(|s| {
        ServiceDto {
            id: s.config.id,
            name: s.config.name,
            exec: s.config.exec,
            args: s.config.args,
            env: s.config.env,
            working_dir: s.config.working_dir,
            windows: s.config.windows,
            autorun: s.config.autorun.unwrap_or(false),
            url: s.config.url,
            status: if s.running { "Running".into() } else { "Stopped".into() },
            pid: s.pid,
        }
    }).collect();

    resp_ok(dtos).into_response()
}
/// Handle: add serive
async fn add_service(
    State(state): State<AppState>,
    Json(payload): Json<ServiceConfig>,
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().await;
    if mgr.services.contains_key(&payload.id) {
        return resp_err("Service ID already exists").into_response();
    }

    match mgr.upsert_service(payload) {
        Ok(_) => resp_ok("Service added").into_response(),
        Err(e) => resp_err(e).into_response(),
    }
}
/// Handle: mod & update service
async fn update_service(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut payload): Json<ServiceConfig>,
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().await;

    payload.id = id;

    match mgr.upsert_service(payload) {
        Ok(_) => resp_ok("Service updated").into_response(),
        Err(e) => resp_err(e).into_response(),
    }
}
/// Handle: delete service
async fn delete_service(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().await;
    match mgr.remove_service(&id).await {
        Ok(_) => resp_ok("Service deleted").into_response(),
        Err(e) => resp_err(e).into_response(),
    }
}
/// Handle: import service by yaml
async fn import_services(
    State(state): State<AppState>,
    Json(payload): Json<ImportRequest>,
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().await;
    let yaml_str = payload.yaml.trim();

    let configs: Vec<ServiceConfig> = match serde_yaml::from_str(yaml_str) {
        Ok(list) => list,
        Err(_) => {
            match serde_yaml::from_str::<ServiceConfig>(yaml_str) {
                Ok(single) => vec![single],
                Err(e) => return resp_err(format!("Parse YAML failed: {}", e)).into_response(),
            }
        }
    };
    let mut count = 0;
    for config in configs {
        if let Err(e) = mgr.upsert_service(config) {
            let _ = resp_err(format!("Save service failed: {}", e)).into_response();
        }
        count += 1;
    }

    resp_ok(format!("Success import {} services", count)).into_response()
}
/// Handle: get keep alive interval
async fn get_config(
    State(state): State<AppState>
) -> impl IntoResponse{
    let mgr = state.manager.lock().await;
    resp_ok(GlobalConfigDto {
        keep_alive: mgr.keep_alive_interval
    })
}
/// Handle: update keep alive interval
async fn update_config(
    State(state): State<AppState>,
    Json(payload): Json<GlobalConfigDto>
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().await;
    match mgr.set_global_config(payload.keep_alive) {
        Ok(_) => resp_ok("Config updated. Restart required to apply change to Keep-Alive loop").into_response(),
        Err(e) => resp_err(e).into_response()
    }
}
/// Handle: order service processing
async fn reorder_services(
    State(state): State<AppState>,
    Json(payload): Json<ReorderRequest>
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().await;
    match mgr.reorder_services(payload.ids) {
        Ok(_) => resp_ok("Order saved").into_response(),
        Err(e) => resp_err(e).into_response()
    }
}