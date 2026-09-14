use axum::{routing::get, Json, Router};
use serde_json::json;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/healthz", get(healthz))
        .fallback_service(ServeDir::new("static").append_index_html_on_directories(true));

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
