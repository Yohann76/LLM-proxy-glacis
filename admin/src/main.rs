use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
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
        .route("/api/observability", get(api_observability))
        .route("/api/analyze", post(api_analyze))
        .route("/api/rules", get(api_rules))
        .route("/api/rules/test", post(api_rules_test))
        .route("/api/compliance-report", get(api_compliance_report))
        .route("/api/dashboard-summary", get(api_dashboard_summary))
        .route("/api/chargeback", get(api_chargeback))
        .route("/api/fallback", get(api_fallback))
        .route("/api/providers", get(api_providers))
        .route("/api/virtual-keys", post(api_create_virtual_key))
        .route("/api/virtual-keys/:id", delete(api_delete_virtual_key))
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
    /// Étape 6 : clé API virtuelle, envoyée en `Authorization: Bearer` —
    /// distincte de `api_key` (qui surcharge la clé du VRAI fournisseur).
    virtual_key: Option<String>,
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
    if let Some(vk) = req.virtual_key.filter(|k| !k.is_empty()) {
        builder = builder.header("authorization", format!("Bearer {vk}"));
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

/// Relaie l'historique des unités d'observabilité (`GET /internal/unites`
/// côté proxy, réseau interne Docker) pour la vue Observabilité de l'admin.
async fn api_observability(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let mut url = format!("{}/internal/unites", state.proxy_url.trim_end_matches('/'));
    if let Some(limit) = params.get("limit") {
        url = format!("{url}?limit={limit}");
    }

    match state.http.get(&url).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(value) => Json(value).into_response(),
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                format!("réponse du proxy illisible : {e}"),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("erreur de connexion au proxy : {e}"),
        )
            .into_response(),
    }
}

// Note : l'axe Risque n'est PAS demandé ici — depuis l'Étape 3, il est
// calculé par le vrai moteur de règles symboliques (config/rules.yaml), pas
// deviné par un LLM. Le mélanger à une estimation sémantique reviendrait à
// dégrader une valeur déterministe en une supposition.
const FOURMI_SYSTEM_PROMPT: &str = r#"Tu es un moteur d'analyse selon la méthode Fourmi : une action est un système complet — qui agit, dans quel contexte, avec quelle ressource, pour produire quoi, au service de quel objectif et de quelle mission.

On te donne une action (le prompt envoyé à un assistant IA). Déduis chaque axe du SENS RÉEL de cette action, jamais de sa forme technique. Réponds UNIQUEMENT avec un objet JSON strictement de cette forme, sans texte autour, sans balises markdown :

{"acteur": "...", "contexte": "...", "ressource": "...", "objectif": "...", "mission": "..."}

Chaque valeur est une phrase courte (moins de 12 mots), en français.

Exemple — action "Dire bonjour en une phrase" :
{"acteur": "celui qui exécute le prompt", "contexte": "avoir quelqu'un avec qui dialoguer", "ressource": "un support de communication", "objectif": "transmettre un message", "mission": "communiquer"}"#;

#[derive(Debug, Deserialize)]
struct AnalyzeRequest {
    action: String,
    provider: Option<String>,
    model: Option<String>,
    /// Clé API fournie par le client (navigateur), jamais lue depuis
    /// l'environnement du conteneur admin. Transmise telle quelle au proxy
    /// via `X-ProxyLLM-Api-Key`, qui prend priorité sur la clé de `.env`.
    api_key: Option<String>,
}

#[derive(Debug, Serialize, Default)]
struct AnalyzeResponse {
    ok: bool,
    fourmi: Option<serde_json::Value>,
    raw: Option<String>,
    error: Option<String>,
}

/// Fait analyser une action par le LLM lui-même (à la demande, jamais
/// automatique) : réutilise le passthrough `/v1/chat/completions` du proxy
/// avec un prompt système dédié, pour déduire Acteur/Contexte/Ressource/
/// Risque/Objectif/Mission du sens réel du prompt plutôt que de métadonnées
/// techniques. Double le nombre d'appels facturés pour l'appel analysé —
/// c'est pourquoi ce n'est déclenché que sur action explicite de l'admin.
async fn api_analyze(State(state): State<AppState>, Json(req): Json<AnalyzeRequest>) -> impl IntoResponse {
    let model = req.model.filter(|m| !m.is_empty()).unwrap_or_else(|| "gpt-4.1-nano".to_string());

    let body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": FOURMI_SYSTEM_PROMPT },
            { "role": "user", "content": req.action }
        ]
    });

    let url = format!("{}/v1/chat/completions", state.proxy_url.trim_end_matches('/'));
    let mut builder = state.http.post(&url).json(&body);
    if let Some(provider) = req.provider.filter(|p| !p.is_empty()) {
        builder = builder.header("x-proxyllm-provider", provider);
    }
    if let Some(api_key) = req.api_key.filter(|k| !k.is_empty()) {
        builder = builder.header("x-proxyllm-api-key", api_key);
    }

    let resp = match builder.send().await {
        Ok(r) => r,
        Err(e) => {
            return Json(AnalyzeResponse {
                error: Some(format!("erreur de connexion au proxy : {e}")),
                ..Default::default()
            })
            .into_response()
        }
    };

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Json(AnalyzeResponse {
            raw: Some(text),
            error: Some(format!("le proxy/fournisseur a répondu {status}")),
            ..Default::default()
        })
        .into_response();
    }

    let parsed: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            return Json(AnalyzeResponse {
                raw: Some(text),
                error: Some(format!("réponse du fournisseur illisible : {e}")),
                ..Default::default()
            })
            .into_response()
        }
    };

    let content = parsed
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let cleaned = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    match serde_json::from_str::<serde_json::Value>(cleaned) {
        Ok(fourmi) => Json(AnalyzeResponse {
            ok: true,
            fourmi: Some(fourmi),
            ..Default::default()
        })
        .into_response(),
        Err(e) => Json(AnalyzeResponse {
            raw: Some(content.to_string()),
            error: Some(format!("le LLM n'a pas renvoyé de JSON exploitable : {e}")),
            ..Default::default()
        })
        .into_response(),
    }
}

/// Relaie le jeu de règles courant (`GET /internal/rules` côté proxy) pour
/// la vue "Règles" de l'admin.
async fn api_rules(State(state): State<AppState>) -> impl IntoResponse {
    let url = format!("{}/internal/rules", state.proxy_url.trim_end_matches('/'));
    match state.http.get(&url).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(value) => Json(value).into_response(),
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                format!("réponse du proxy illisible : {e}"),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("erreur de connexion au proxy : {e}"),
        )
            .into_response(),
    }
}

/// Relaie un test de décision (`POST /internal/rules/test` côté proxy) —
/// évalue des faits saisis à la main contre le jeu de règles courant, sans
/// faire de vraie requête LLM.
async fn api_rules_test(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let url = format!(
        "{}/internal/rules/test",
        state.proxy_url.trim_end_matches('/')
    );
    match state.http.post(&url).json(&body).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(value) => Json(value).into_response(),
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                format!("réponse du proxy illisible : {e}"),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("erreur de connexion au proxy : {e}"),
        )
            .into_response(),
    }
}

/// Relaie le rapport de conformité (`GET /internal/compliance-report` côté
/// proxy) — JSON ou PDF selon `?format=`, transmis tel quel (le PDF n'est
/// jamais rechargé en mémoire sous une autre forme, juste transmis).
async fn api_compliance_report(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let mut url = format!(
        "{}/internal/compliance-report",
        state.proxy_url.trim_end_matches('/')
    );
    if !params.is_empty() {
        let qs: String = params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        url = format!("{url}?{qs}");
    }

    match state.http.get(&url).send().await {
        Ok(resp) => {
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_string();
            let content_disposition = resp
                .headers()
                .get(reqwest::header::CONTENT_DISPOSITION)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);

            match resp.bytes().await {
                Ok(bytes) => {
                    let mut headers = axum::http::HeaderMap::new();
                    if let Ok(v) = content_type.parse() {
                        headers.insert(axum::http::header::CONTENT_TYPE, v);
                    }
                    if let Some(cd) = content_disposition {
                        if let Ok(v) = cd.parse() {
                            headers.insert(axum::http::header::CONTENT_DISPOSITION, v);
                        }
                    }
                    (StatusCode::OK, headers, bytes).into_response()
                }
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    format!("réponse du proxy illisible : {e}"),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("erreur de connexion au proxy : {e}"),
        )
            .into_response(),
    }
}

/// Relaie le résumé du tableau de bord (`GET /internal/dashboard-summary`
/// côté proxy) pour la page d'accueil de l'admin.
async fn api_dashboard_summary(State(state): State<AppState>) -> impl IntoResponse {
    let url = format!(
        "{}/internal/dashboard-summary",
        state.proxy_url.trim_end_matches('/')
    );
    match state.http.get(&url).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(value) => Json(value).into_response(),
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                format!("réponse du proxy illisible : {e}"),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("erreur de connexion au proxy : {e}"),
        )
            .into_response(),
    }
}

/// Relaie le rapport chargeback (`GET /internal/chargeback` côté proxy —
/// clés virtuelles jamais exposées en clair, toujours masquées) pour la
/// vue FinOps de l'admin.
async fn api_chargeback(State(state): State<AppState>) -> impl IntoResponse {
    let url = format!("{}/internal/chargeback", state.proxy_url.trim_end_matches('/'));
    match state.http.get(&url).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(value) => Json(value).into_response(),
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                format!("réponse du proxy illisible : {e}"),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("erreur de connexion au proxy : {e}"),
        )
            .into_response(),
    }
}

/// Relaie l'introspection du fallback (`GET /internal/fallback` côté
/// proxy) pour la vue "Fournisseurs" de l'admin.
async fn api_fallback(State(state): State<AppState>) -> impl IntoResponse {
    let url = format!("{}/internal/fallback", state.proxy_url.trim_end_matches('/'));
    match state.http.get(&url).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(value) => Json(value).into_response(),
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                format!("réponse du proxy illisible : {e}"),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("erreur de connexion au proxy : {e}"),
        )
            .into_response(),
    }
}

/// Relaie l'introspection des fournisseurs (`GET /internal/providers` côté
/// proxy) pour la vue "Fournisseurs" de l'admin.
async fn api_providers(State(state): State<AppState>) -> impl IntoResponse {
    let url = format!("{}/internal/providers", state.proxy_url.trim_end_matches('/'));
    match state.http.get(&url).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(value) => Json(value).into_response(),
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                format!("réponse du proxy illisible : {e}"),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("erreur de connexion au proxy : {e}"),
        )
            .into_response(),
    }
}

/// Crée une clé virtuelle (`POST /internal/virtual-keys` côté proxy) —
/// Étape 8, "gestion des accès". La réponse contient la clé en clair : ne
/// transite qu'ici, une seule fois, jamais stockée côté admin.
async fn api_create_virtual_key(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let url = format!("{}/internal/virtual-keys", state.proxy_url.trim_end_matches('/'));
    match state.http.post(&url).json(&body).send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            match resp.text().await {
                Ok(text) => (status, [(axum::http::header::CONTENT_TYPE, "application/json")], text)
                    .into_response(),
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    format!("réponse du proxy illisible : {e}"),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("erreur de connexion au proxy : {e}"),
        )
            .into_response(),
    }
}

/// Révoque une clé virtuelle par son `id` non secret (`DELETE
/// /internal/virtual-keys/:id` côté proxy).
async fn api_delete_virtual_key(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let url = format!(
        "{}/internal/virtual-keys/{id}",
        state.proxy_url.trim_end_matches('/')
    );
    match state.http.delete(&url).send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            match resp.text().await {
                Ok(text) => (status, [(axum::http::header::CONTENT_TYPE, "application/json")], text)
                    .into_response(),
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    format!("réponse du proxy illisible : {e}"),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("erreur de connexion au proxy : {e}"),
        )
            .into_response(),
    }
}
