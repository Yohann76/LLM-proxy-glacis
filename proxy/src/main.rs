use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
    routing::{any, get},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Deserialize, Clone)]
struct ProviderConfig {
    base_url: String,
    api_key_env: String,
}

#[derive(Debug, Deserialize, Clone)]
struct Config {
    default_provider: String,
    providers: HashMap<String, ProviderConfig>,
}

#[derive(Clone)]
struct AppState {
    config: Arc<Config>,
    http: reqwest::Client,
}

#[tokio::main]
async fn main() {
    let config_path =
        std::env::var("PROXY_CONFIG").unwrap_or_else(|_| "config/providers.yaml".to_string());

    let raw = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|e| panic!("impossible de lire le fichier de config {config_path}: {e}"));
    let config: Config = serde_yaml::from_str(&raw)
        .unwrap_or_else(|e| panic!("fichier de config invalide {config_path}: {e}"));

    println!(
        "Fournisseurs chargés depuis {config_path} : {:?} (défaut : {})",
        config.providers.keys().collect::<Vec<_>>(),
        config.default_provider
    );

    let state = AppState {
        config: Arc::new(config),
        http: reqwest::Client::new(),
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/healthz", get(healthz))
        .route("/v1/*rest", any(passthrough))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("impossible de binder le port 8080");

    println!("ProxyLLM proxy en écoute sur 0.0.0.0:8080");
    axum::serve(listener, app)
        .await
        .expect("le serveur proxy s'est arrêté de façon inattendue");
}

async fn root() -> &'static str {
    "ProxyLLM Glacis — proxy (passthrough actif sur /v1/*)"
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "proxyllm-proxy" }))
}

/// Relaie toute requête `/v1/*` vers le fournisseur LLM configuré.
/// Le fournisseur est choisi via l'en-tête `X-ProxyLLM-Provider`, ou à
/// défaut le `default_provider` du fichier de config.
///
/// La clé API vient, par ordre de priorité :
/// 1. de l'en-tête `X-ProxyLLM-Api-Key` (surcharge ponctuelle, ex. depuis
///    l'interface de test) ;
/// 2. sinon de la variable d'environnement déclarée dans la config
///    (`api_key_env`), configurée une fois par l'admin.
async fn passthrough(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let provider_name = headers
        .get("x-proxyllm-provider")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(state.config.default_provider.as_str())
        .to_string();

    let Some(provider) = state.config.providers.get(&provider_name) else {
        return (
            StatusCode::BAD_GATEWAY,
            format!("fournisseur inconnu : {provider_name}"),
        )
            .into_response();
    };

    let override_key = headers
        .get("x-proxyllm-api-key")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .map(str::to_string);

    let api_key = match override_key {
        Some(v) => v,
        None => match std::env::var(&provider.api_key_env) {
            Ok(v) if !v.is_empty() => v,
            _ => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "variable d'environnement {} absente ou vide pour le fournisseur {provider_name}",
                        provider.api_key_env
                    ),
                )
                    .into_response()
            }
        },
    };

    let path_and_query = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let target_url = format!(
        "{}{}",
        provider.base_url.trim_end_matches('/'),
        path_and_query
    );

    let mut req = state.http.request(method, &target_url).body(body);
    for (name, value) in headers.iter() {
        if matches!(
            name.as_str(),
            "host" | "authorization" | "content-length" | "x-proxyllm-provider" | "x-proxyllm-api-key"
        ) {
            continue;
        }
        req = req.header(name, value);
    }
    req = req.header("authorization", format!("Bearer {api_key}"));

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("erreur fournisseur : {e}")).into_response()
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut out_headers = HeaderMap::new();
    for (name, value) in resp.headers().iter() {
        if matches!(name.as_str(), "transfer-encoding" | "connection") {
            continue;
        }
        out_headers.insert(name.clone(), value.clone());
    }

    let stream = resp.bytes_stream();
    (status, out_headers, Body::from_stream(stream)).into_response()
}
