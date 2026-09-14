use axum::{routing::get, Json, Router};
use serde_json::json;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(root))
        .route("/healthz", get(healthz));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("impossible de binder le port 8080");

    println!("ProxyLLM proxy en écoute sur 0.0.0.0:8080");
    axum::serve(listener, app)
        .await
        .expect("le serveur proxy s'est arrêté de façon inattendue");
}

async fn root() -> &'static str {
    "ProxyLLM Glacis — proxy (en construction)"
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "proxyllm-proxy" }))
}
