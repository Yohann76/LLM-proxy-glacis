use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::services::ServeDir;

#[derive(Clone)]
struct AppState {
    http: reqwest::Client,
    proxy_url: String,
}

#[tokio::main]
async fn main() {
    let proxy_url =
        std::env::var("PROXY_INTERNAL_URL").unwrap_or_else(|_| "http://proxy:8080".to_string());

    let state = AppState {
        http: reqwest::Client::new(),
        proxy_url,
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/test", post(api_test))
        .fallback_service(ServeDir::new("static").append_index_html_on_directories(true))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8081")
        .await
        .expect("impossible de binder le port 8081");

    println!("ProxyLLM admin en écoute sur 0.0.0.0:8081");
    axum::serve(listener, app)
        .await
        .expect("le serveur admin s'est arrêté de façon inattendue");
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "proxyllm-admin" }))
}

#[derive(Debug, Deserialize)]
struct TestRequest {
    provider: Option<String>,
    #[serde(default = "default_method")]
    method: String,
    path: String,
    body: Option<String>,
    api_key: Option<String>,
}

fn default_method() -> String {
    "POST".to_string()
}

#[derive(Debug, Serialize)]
struct TestResponse {
    status: u16,
    latency_ms: u128,
    body: String,
}

/// Relaie une requête de test vers le proxy (réseau interne Docker), pour
/// que la page de test dans l'admin puisse appeler l'API sans souci de CORS.
async fn api_test(State(state): State<AppState>, Json(req): Json<TestRequest>) -> impl IntoResponse {
    let method = reqwest::Method::from_bytes(req.method.as_bytes()).unwrap_or(reqwest::Method::POST);
    let url = format!("{}{}", state.proxy_url.trim_end_matches('/'), req.path);

    let mut builder = state.http.request(method, &url).header("content-type", "application/json");
    if let Some(provider) = req.provider.filter(|p| !p.is_empty()) {
        builder = builder.header("x-proxyllm-provider", provider);
    }
    if let Some(api_key) = req.api_key.filter(|k| !k.is_empty()) {
        builder = builder.header("x-proxyllm-api-key", api_key);
    }
    if let Some(body) = req.body {
        builder = builder.body(body);
    }

    let start = std::time::Instant::now();
    let result = builder.send().await;
    let latency_ms = start.elapsed().as_millis();

    match result {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Json(TestResponse { status, latency_ms, body }).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(TestResponse {
                status: 502,
                latency_ms,
                body: format!("erreur de connexion au proxy : {e}"),
            }),
        )
            .into_response(),
    }
}
