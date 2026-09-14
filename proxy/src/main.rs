mod unite;

use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
    routing::{any, get},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use unite::{LogsAxis, Store, UniteRecord};

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
    unites: Store,
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

    match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(v) => println!("Export OpenTelemetry activé vers {v}"),
        Err(_) => println!(
            "Export OpenTelemetry désactivé (variable OTEL_EXPORTER_OTLP_ENDPOINT absente)"
        ),
    }

    let state = AppState {
        config: Arc::new(config),
        http: reqwest::Client::new(),
        unites: Store::new(),
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/healthz", get(healthz))
        .route("/internal/unites", get(list_unites))
        .route("/v1/*rest", any(passthrough))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("impossible de binder le port 8080");

    println!("ProxyLLM proxy en écoute sur 0.0.0.0:8080");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("le serveur proxy s'est arrêté de façon inattendue");
}

async fn root() -> &'static str {
    "ProxyLLM Glacis — proxy (passthrough actif sur /v1/*)"
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "proxyllm-proxy" }))
}

#[derive(Debug, Deserialize)]
struct ListUnitesParams {
    limit: Option<usize>,
}

/// Historique en mémoire des dernières unités d'observabilité (§2.1 du
/// cahier des charges), consommé par la vue Observabilité de l'admin.
/// Aucune donnée sensible (pas de corps de requête/réponse) n'y figure.
async fn list_unites(
    State(state): State<AppState>,
    Query(params): Query<ListUnitesParams>,
) -> Json<Vec<UniteRecord>> {
    let limit = params.limit.unwrap_or(50).min(unite::HISTORY_CAPACITY);
    Json(state.unites.recent(limit).await)
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
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
///
/// Chaque requête est également décomposée selon la grille Unité (§2.1 du
/// cahier des charges) et journalisée en tâche asynchrone, sans impact sur
/// la latence de la réponse.
async fn passthrough(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let start = std::time::Instant::now();
    let request_id = unite::new_request_id();

    let provider_name = header_str(&headers, "x-proxyllm-provider")
        .unwrap_or(state.config.default_provider.as_str())
        .to_string();

    let method_str = method.to_string();
    let path = uri.path().to_string();
    let request_bytes = body.len();
    let model_hint = extract_model(&body);
    let action = extract_action(&body, &method_str, &path);

    let acteur = header_str(&headers, "x-proxyllm-actor")
        .map(str::to_string)
        .unwrap_or_else(|| peer.ip().to_string());
    let contexte = header_str(&headers, "x-proxyllm-context")
        .or_else(|| header_str(&headers, "user-agent"))
        .unwrap_or("inconnu")
        .to_string();
    let relation = header_str(&headers, "x-proxyllm-session")
        .unwrap_or("isolee")
        .to_string();
    let objectif = header_str(&headers, "x-proxyllm-objective")
        .unwrap_or("non_precise")
        .to_string();
    let mission = header_str(&headers, "x-proxyllm-mission")
        .unwrap_or("non_precise")
        .to_string();
    let ressource = match &model_hint {
        Some(model) => format!("{provider_name}/{model}"),
        None => provider_name.clone(),
    };

    let Some(provider) = state.config.providers.get(&provider_name) else {
        emit_unite(
            &state,
            request_id,
            action,
            acteur,
            contexte,
            ressource,
            relation,
            objectif,
            mission,
            method_str,
            path,
            StatusCode::BAD_GATEWAY.as_u16(),
            start.elapsed().as_millis(),
            request_bytes,
            "fournisseur inconnu".to_string(),
        );
        return (
            StatusCode::BAD_GATEWAY,
            format!("fournisseur inconnu : {provider_name}"),
        )
            .into_response();
    };

    let override_key = header_str(&headers, "x-proxyllm-api-key")
        .filter(|v| !v.is_empty())
        .map(str::to_string);

    let api_key = match override_key {
        Some(v) => v,
        None => match std::env::var(&provider.api_key_env) {
            Ok(v) if !v.is_empty() => v,
            _ => {
                emit_unite(
                    &state,
                    request_id,
                    action,
                    acteur,
                    contexte,
                    ressource,
                    relation,
                    objectif,
                    mission,
                    method_str,
                    path,
                    StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    start.elapsed().as_millis(),
                    request_bytes,
                    "clé API manquante".to_string(),
                );
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "variable d'environnement {} absente ou vide pour le fournisseur {provider_name}",
                        provider.api_key_env
                    ),
                )
                    .into_response();
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
            "host"
                | "authorization"
                | "content-length"
                | "x-proxyllm-provider"
                | "x-proxyllm-api-key"
                | "x-proxyllm-actor"
                | "x-proxyllm-context"
                | "x-proxyllm-session"
                | "x-proxyllm-objective"
                | "x-proxyllm-mission"
        ) {
            continue;
        }
        req = req.header(name, value);
    }
    req = req.header("authorization", format!("Bearer {api_key}"));

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            emit_unite(
                &state,
                request_id,
                action,
                acteur,
                contexte,
                ressource,
                relation,
                objectif,
                mission,
                method_str,
                path,
                StatusCode::BAD_GATEWAY.as_u16(),
                start.elapsed().as_millis(),
                request_bytes,
                format!("erreur fournisseur : {e}"),
            );
            return (StatusCode::BAD_GATEWAY, format!("erreur fournisseur : {e}")).into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let result_size = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let realisation = if status.is_success() {
        format!(
            "succes ({} octets)",
            result_size.unwrap_or_else(|| "stream".to_string())
        )
    } else {
        format!("erreur http {}", status.as_u16())
    };

    let mut out_headers = HeaderMap::new();
    for (name, value) in resp.headers().iter() {
        if matches!(name.as_str(), "transfer-encoding" | "connection") {
            continue;
        }
        out_headers.insert(name.clone(), value.clone());
    }

    emit_unite(
        &state,
        request_id,
        action,
        acteur,
        contexte,
        ressource,
        relation,
        objectif,
        mission,
        method_str,
        path,
        status.as_u16(),
        start.elapsed().as_millis(),
        request_bytes,
        realisation,
    );

    let stream = resp.bytes_stream();
    (status, out_headers, Body::from_stream(stream)).into_response()
}

/// Best-effort : tente d'extraire le champ "model" du corps JSON de la
/// requête, pour renseigner l'axe Ressource. Ne bloque jamais (silencieux
/// si le corps n'est pas du JSON exploitable, ex. requêtes GET).
fn extract_model(body: &Bytes) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    value.get("model")?.as_str().map(str::to_string)
}

/// Extrait un aperçu de l'intention réelle envoyée au LLM (dernier message
/// `"role": "user"` du tableau `"messages"`, format OpenAI chat), tronqué à
/// 80 caractères. Remplace le simple "méthode + chemin" HTTP comme label de
/// l'axe Action, conformément à la méthode Fourmi (verbe + objet de
/// l'action réelle, cf. Docs/Fourmi.md §1.1).
///
/// ⚠️ Peut donc contenir des données saisies par l'utilisateur, potentiellement
/// sensibles. Aucun masquage PII n'existe encore (Étape 4 de la roadmap) :
/// c'est un compromis assumé en attendant, pas un oubli. Retombe sur
/// "méthode + chemin" si le corps n'est pas exploitable (ex. requêtes GET,
/// formats d'API non conversationnels).
fn extract_action(body: &Bytes, method: &str, path: &str) -> String {
    const MAX_CHARS: usize = 80;
    let fallback = || format!("{method} {path}");

    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return fallback();
    };
    let Some(messages) = value.get("messages").and_then(|m| m.as_array()) else {
        return fallback();
    };

    let content = messages.iter().rev().find_map(|m| {
        if m.get("role").and_then(|r| r.as_str()) != Some("user") {
            return None;
        }
        match m.get("content") {
            Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
            Some(serde_json::Value::Array(parts)) => {
                let joined = parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ");
                (!joined.trim().is_empty()).then_some(joined)
            }
            _ => None,
        }
    });

    match content {
        Some(text) => {
            let trimmed = text.trim();
            let truncated: String = trimmed.chars().take(MAX_CHARS).collect();
            if trimmed.chars().count() > MAX_CHARS {
                format!("{truncated}…")
            } else {
                truncated
            }
        }
        None => fallback(),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_unite(
    state: &AppState,
    request_id: String,
    action: String,
    acteur: String,
    contexte: String,
    ressource: String,
    relation: String,
    objectif: String,
    mission: String,
    method: String,
    path: String,
    status: u16,
    latency_ms: u128,
    request_bytes: usize,
    realisation: String,
) {
    let record = UniteRecord {
        request_id,
        timestamp_unix_ms: unite::now_unix_ms(),
        action,
        acteur,
        contexte,
        ressource,
        logs: LogsAxis {
            method,
            path,
            status,
            latency_ms,
            request_bytes,
        },
        // Le moteur de règles symboliques (Étape 3 de la roadmap) n'existe
        // pas encore : aucun score de risque n'est calculé pour l'instant.
        risques: "non_evalue".to_string(),
        relation,
        realisation,
        objectif,
        mission,
    };

    let http = state.http.clone();
    let store = state.unites.clone();
    tokio::spawn(async move {
        unite::emit(record, http, store).await;
    });
}
