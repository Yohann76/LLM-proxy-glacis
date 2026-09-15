mod audit;
mod finops;
mod pii;
mod rules;
mod unite;

use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
    routing::{any, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
use unite::{LogsAxis, Store, TokenUsage, UniteRecord};

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
    rule_engine: rules::RuleEngine,
    pii_masking_enabled: bool,
    audit_log_path: Arc<PathBuf>,
    virtual_keys: finops::VirtualKeyStore,
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

    let rules_path =
        std::env::var("PROXY_RULES").unwrap_or_else(|_| "config/rules.yaml".to_string());
    let rule_engine = rules::RuleEngine::load_initial(rules_path.into());
    rule_engine.spawn_watcher(std::time::Duration::from_secs(2));

    // Détection & masquage PII/secrets (Étape 4) : actif par défaut,
    // désactivable UNIQUEMENT côté serveur — jamais par un en-tête client,
    // pour qu'une protection de conformité ne puisse pas être contournée
    // simplement par qui appelle le proxy.
    let pii_masking_enabled = std::env::var("PROXY_PII_MASKING")
        .map(|v| !matches!(v.to_lowercase().as_str(), "off" | "false" | "0"))
        .unwrap_or(true);
    println!(
        "Détection/masquage PII & secrets : {}",
        if pii_masking_enabled { "actif" } else { "désactivé (PROXY_PII_MASKING)" }
    );

    // Auditabilité (Étape 5) : journal d'audit persistant, distinct de
    // l'historique en mémoire de l'Étape 2 (qui reste, lui, borné et non
    // persisté). Monté en volume (docker-compose : ./data:/app/data) pour
    // survivre aux redémarrages du conteneur.
    let audit_log_path: PathBuf = std::env::var("PROXY_AUDIT_LOG")
        .unwrap_or_else(|_| "data/audit.jsonl".to_string())
        .into();
    println!("Journal d'audit : {}", audit_log_path.display());

    // Clés API virtuelles & FinOps (Étape 6). Réhydratation des compteurs
    // de consommation depuis le journal d'audit déjà chargé ci-dessus.
    let virtual_keys_path =
        std::env::var("PROXY_VIRTUAL_KEYS").unwrap_or_else(|_| "config/virtual_keys.yaml".to_string());
    let virtual_keys = finops::VirtualKeyStore::load_initial(virtual_keys_path.into());
    virtual_keys.rehydrate_from_audit(&audit::read_all(&audit_log_path)).await;
    virtual_keys.spawn_watcher(std::time::Duration::from_secs(2));

    let state = AppState {
        config: Arc::new(config),
        http: reqwest::Client::new(),
        unites: Store::new(),
        rule_engine,
        pii_masking_enabled,
        audit_log_path: Arc::new(audit_log_path),
        virtual_keys,
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/healthz", get(healthz))
        .route("/internal/unites", get(list_unites))
        .route("/internal/rules", get(list_rules))
        .route("/internal/rules/test", post(test_rules))
        .route("/internal/compliance-report", get(compliance_report))
        .route("/internal/chargeback", get(chargeback))
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

/// Introspection du jeu de règles courant (utile pour vérifier un
/// rechargement à chaud sans SSH dans le conteneur).
async fn list_rules(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(state.rule_engine.summary().await)
}

#[derive(Debug, Deserialize, Default)]
struct TestFactsInput {
    acteur: Option<String>,
    contexte: Option<String>,
    provider: Option<String>,
    mission: Option<String>,
    objectif: Option<String>,
    action: Option<String>,
}

/// Évalue des faits fournis à la main contre le jeu de règles courant, sans
/// faire de vraie requête LLM — pour la vue "Règles" de l'admin (tester une
/// décision avant de l'appliquer en vrai).
async fn test_rules(
    State(state): State<AppState>,
    Json(input): Json<TestFactsInput>,
) -> Json<serde_json::Value> {
    let facts = rules::Facts {
        acteur: input.acteur.as_deref().unwrap_or(""),
        contexte: input.contexte.as_deref().unwrap_or(""),
        provider: input.provider.as_deref().unwrap_or(""),
        mission: input.mission.as_deref().unwrap_or(""),
        objectif: input.objectif.as_deref().unwrap_or(""),
        action: input.action.as_deref().unwrap_or(""),
    };
    let decision = state.rule_engine.evaluate(&facts).await;
    Json(json!({
        "blocked_by": decision.blocked_by,
        "allowed_by": decision.allowed_by,
        "alerts": decision.alerts,
        "masks": decision.mask_rules,
        "summary": decision.summary(),
    }))
}

#[derive(Debug, Deserialize)]
struct ComplianceQuery {
    /// "json" (défaut) ou "pdf".
    format: Option<String>,
    /// Bornes de période en timestamp unix (ms). Non fournies = tout le
    /// journal d'audit disponible.
    since: Option<u64>,
    until: Option<u64>,
}

/// "1-Click Compliance Report" (Étape 5, cf. cahier des charges §2.4) :
/// agrège le journal d'audit persistant (`data/audit.jsonl`) en rapport
/// JSON ou PDF — volumétrie, blocages, alertes, détections PII, de quoi
/// répondre aux exigences de traçabilité de l'EU AI Act.
async fn compliance_report(
    State(state): State<AppState>,
    Query(params): Query<ComplianceQuery>,
) -> impl IntoResponse {
    let records = audit::read_all(&state.audit_log_path);
    let report = audit::compute_report(
        &records,
        params.since.map(u128::from),
        params.until.map(u128::from),
    );

    if params.format.as_deref() == Some("pdf") {
        let bytes = audit::render_pdf(&report);
        (
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, "application/pdf".to_string()),
                (
                    axum::http::header::CONTENT_DISPOSITION,
                    "attachment; filename=\"rapport-conformite.pdf\"".to_string(),
                ),
            ],
            bytes,
        )
            .into_response()
    } else {
        Json(report).into_response()
    }
}

/// Rapport chargeback (Étape 6) : consommation de tokens et coût estimé
/// par clé API virtuelle. Clés brutes jamais exposées (masquées côté
/// `finops::VirtualKeyStore`).
async fn chargeback(State(state): State<AppState>) -> Json<serde_json::Value> {
    let entries = state.virtual_keys.chargeback_report().await;
    Json(json!({
        "enforced": state.virtual_keys.is_enforced().await,
        "keys": entries,
    }))
}

/// Toutes les valeurs nécessaires pour construire et journaliser une
/// unité d'observabilité (§2.1 du cahier des charges). Struct plutôt que
/// des paramètres positionnels : la liste est longue (9 axes + Étapes
/// 3/4/6) et des champs mal ordonnés à un site d'appel ne seraient pas
/// détectés par le compilateur si les types coïncident.
struct UniteInput {
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
    risques: String,
    realisation: String,
    pii_masked: bool,
    pii_categories: Vec<String>,
    virtual_key: Option<String>,
    tokens: TokenUsage,
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

    // Clés API virtuelles (Étape 6) : question d'identité/accès, traitée
    // avant tout le reste. Sans clé configurée (fichier vide), le proxy
    // reste ouvert et se comporte exactement comme avant cette étape.
    let client_bearer = header_str(&headers, "authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let resolved_key = match &client_bearer {
        Some(token) => state.virtual_keys.resolve(token).await,
        None => None,
    };
    let virtual_key_enforced = state.virtual_keys.is_enforced().await;

    if virtual_key_enforced && resolved_key.is_none() {
        emit_unite(
            &state,
            UniteInput {
                request_id,
                action,
                acteur: "inconnu".to_string(),
                contexte,
                ressource,
                relation,
                objectif,
                mission,
                method: method_str,
                path,
                status: StatusCode::UNAUTHORIZED.as_u16(),
                latency_ms: start.elapsed().as_millis(),
                request_bytes,
                risques: "cle virtuelle invalide ou absente".to_string(),
                realisation: "acces refuse".to_string(),
                pii_masked: false,
                pii_categories: Vec::new(),
                virtual_key: None,
                tokens: TokenUsage::default(),
            },
        );
        return (
            StatusCode::UNAUTHORIZED,
            "clé API virtuelle invalide ou absente (en-tête Authorization: Bearer <clé>)",
        )
            .into_response();
    }

    let acteur = match &resolved_key {
        Some(vk) => vk.name.clone(),
        None => acteur,
    };

    // Quota (Étape 6) : vérifié sur la consommation connue AVANT cet appel
    // (mise à jour asynchrone après chaque réponse — cohérence à terme,
    // pas une garantie stricte anti-rafale, cf. Docs/roadmap.md).
    if let Some(vk) = &resolved_key {
        if let Some(quota) = vk.quota_tokens {
            let used = state.virtual_keys.consumption_of(&vk.key).await;
            if used >= quota {
                emit_unite(
                    &state,
                    UniteInput {
                        request_id,
                        action,
                        acteur,
                        contexte,
                        ressource,
                        relation,
                        objectif,
                        mission,
                        method: method_str,
                        path,
                        status: StatusCode::FORBIDDEN.as_u16(),
                        latency_ms: start.elapsed().as_millis(),
                        request_bytes,
                        risques: format!("quota depasse : {used}/{quota} tokens"),
                        realisation: "acces refuse".to_string(),
                        pii_masked: false,
                        pii_categories: Vec::new(),
                        virtual_key: Some(vk.key.clone()),
                        tokens: TokenUsage::default(),
                    },
                );
                return (
                    StatusCode::FORBIDDEN,
                    format!(
                        "quota de tokens dépassé pour la clé « {} » ({used}/{quota})",
                        vk.name
                    ),
                )
                    .into_response();
            }
        }
    }

    // Moteur de règles symboliques (Étape 3) : évaluation synchrone, sur le
    // chemin critique, avant tout appel réseau au fournisseur — c'est ce qui
    // garde le surcoût largement sous les 5 ms visés (comparaisons de
    // chaînes + regex précompilées, aucune I/O).
    let decision = state
        .rule_engine
        .evaluate(&rules::Facts {
            acteur: &acteur,
            contexte: &contexte,
            provider: &provider_name,
            mission: &mission,
            objectif: &objectif,
            action: &action,
        })
        .await;
    let mut risques = decision.summary();
    if let Some(vk) = &resolved_key {
        if let Some(quota) = vk.quota_tokens {
            let used = state.virtual_keys.consumption_of(&vk.key).await;
            if quota > 0 && (used as f64) >= (quota as f64) * 0.8 {
                risques = format!("{risques} ; alerte surconsommation : {used}/{quota} tokens");
            }
        }
    }

    if let Some(rule_name) = decision.blocked_by.clone() {
        emit_unite(
            &state,
            UniteInput {
                request_id,
                action,
                acteur,
                contexte,
                ressource,
                relation,
                objectif,
                mission,
                method: method_str,
                path,
                status: StatusCode::FORBIDDEN.as_u16(),
                latency_ms: start.elapsed().as_millis(),
                request_bytes,
                risques: risques.clone(),
                realisation: format!("bloque par la regle '{rule_name}'"),
                pii_masked: false,
                pii_categories: Vec::new(),
                virtual_key: resolved_key.as_ref().map(|k| k.key.clone()),
                tokens: TokenUsage::default(),
            },
        );
        return (
            StatusCode::FORBIDDEN,
            format!("requête bloquée par la règle « {rule_name} »"),
        )
            .into_response();
    }

    // "Masquer" (Étape 3, règles écrites à la main) : appliqué à la fois sur
    // l'axe Action (ce qui est journalisé / affiché) et sur le corps
    // forwardé au fournisseur — sinon on masquerait l'observabilité sans
    // masquer ce qui part réellement chez le tiers, ce qui serait pire
    // qu'inutile.
    let (action, body) = if decision.has_masks() {
        let masked_action = decision.apply_masks(&action);
        let masked_body = match std::str::from_utf8(&body) {
            Ok(text) => Bytes::from(decision.apply_masks(text)),
            Err(_) => body,
        };
        (masked_action, masked_body)
    } else {
        (action, body)
    };

    // Détection & masquage PII/secrets (Étape 4, intégré — pas besoin
    // d'écrire une règle). `pii_mapping` (placeholder → vraie valeur) ne
    // sert qu'à la réinjection dans la réponse ci-dessous : jamais
    // persisté, jamais journalisé.
    let (action, _) = if state.pii_masking_enabled {
        let masked = pii::mask(&action);
        (masked.text, masked.mapping)
    } else {
        (action, HashMap::new())
    };
    let (body, pii_mapping, pii_categories) = if state.pii_masking_enabled {
        match std::str::from_utf8(&body) {
            Ok(text) => {
                let masked = pii::mask(text);
                (Bytes::from(masked.text), masked.mapping, masked.categories)
            }
            Err(_) => (body, HashMap::new(), Vec::new()),
        }
    } else {
        (body, HashMap::new(), Vec::new())
    };
    let pii_masked = !pii_categories.is_empty();
    let risques = if pii_categories.is_empty() {
        risques
    } else {
        format!("{risques} ; PII detectee : {}", pii_categories.join(", "))
    };

    let Some(provider) = state.config.providers.get(&provider_name) else {
        emit_unite(
            &state,
            UniteInput {
                request_id,
                action,
                acteur,
                contexte,
                ressource,
                relation,
                objectif,
                mission,
                method: method_str,
                path,
                status: StatusCode::BAD_GATEWAY.as_u16(),
                latency_ms: start.elapsed().as_millis(),
                request_bytes,
                risques,
                realisation: "fournisseur inconnu".to_string(),
                pii_masked,
                pii_categories,
                virtual_key: resolved_key.as_ref().map(|k| k.key.clone()),
                tokens: TokenUsage::default(),
            },
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
                    UniteInput {
                        request_id,
                        action,
                        acteur,
                        contexte,
                        ressource,
                        relation,
                        objectif,
                        mission,
                        method: method_str,
                        path,
                        status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                        latency_ms: start.elapsed().as_millis(),
                        request_bytes,
                        risques,
                        realisation: "clé API manquante".to_string(),
                        pii_masked,
                        pii_categories,
                        virtual_key: resolved_key.as_ref().map(|k| k.key.clone()),
                        tokens: TokenUsage::default(),
                    },
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
                UniteInput {
                    request_id,
                    action,
                    acteur,
                    contexte,
                    ressource,
                    relation,
                    objectif,
                    mission,
                    method: method_str,
                    path,
                    status: StatusCode::BAD_GATEWAY.as_u16(),
                    latency_ms: start.elapsed().as_millis(),
                    request_bytes,
                    risques,
                    realisation: format!("erreur fournisseur : {e}"),
                    pii_masked,
                    pii_categories,
                    virtual_key: resolved_key.as_ref().map(|k| k.key.clone()),
                    tokens: TokenUsage::default(),
                },
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

    // Réinjection (Étape 4) et suivi précis des tokens (Étape 6) exigent
    // tous les deux de lire le corps de la réponse — donc de renoncer au
    // streaming zero-copy pour CET appel précis. Sans PII à réinjecter et
    // sans clé virtuelle à suivre (le cas par défaut), le streaming
    // zero-copy reste intact : c'est le compromis, assumé et documenté.
    let must_buffer = !pii_mapping.is_empty() || resolved_key.is_some();

    if !must_buffer {
        emit_unite(
            &state,
            UniteInput {
                request_id,
                action,
                acteur,
                contexte,
                ressource,
                relation,
                objectif,
                mission,
                method: method_str,
                path,
                status: status.as_u16(),
                latency_ms: start.elapsed().as_millis(),
                request_bytes,
                risques,
                realisation,
                pii_masked,
                pii_categories,
                virtual_key: None,
                tokens: TokenUsage::default(),
            },
        );
        let stream = resp.bytes_stream();
        return (status, out_headers, Body::from_stream(stream)).into_response();
    }

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            emit_unite(
                &state,
                UniteInput {
                    request_id,
                    action,
                    acteur,
                    contexte,
                    ressource,
                    relation,
                    objectif,
                    mission,
                    method: method_str,
                    path,
                    status: StatusCode::BAD_GATEWAY.as_u16(),
                    latency_ms: start.elapsed().as_millis(),
                    request_bytes,
                    risques,
                    realisation: format!("erreur de lecture de la réponse fournisseur : {e}"),
                    pii_masked,
                    pii_categories,
                    virtual_key: resolved_key.as_ref().map(|k| k.key.clone()),
                    tokens: TokenUsage::default(),
                },
            );
            return (
                StatusCode::BAD_GATEWAY,
                format!("erreur de lecture de la réponse fournisseur : {e}"),
            )
                .into_response();
        }
    };

    let final_bytes = if pii_mapping.is_empty() {
        bytes
    } else {
        match std::str::from_utf8(&bytes) {
            Ok(text) => Bytes::from(pii::reinject(text, &pii_mapping)),
            Err(_) => bytes,
        }
    };

    let tokens = finops::extract_usage(&final_bytes).unwrap_or_default();
    if let Some(vk) = &resolved_key {
        if !tokens.is_empty() {
            state.virtual_keys.record_usage(&vk.key, tokens.total_tokens).await;
        }
    }

    let realisation = if pii_mapping.is_empty() {
        realisation
    } else {
        format!(
            "{realisation} ; {} valeur(s) PII reinjectee(s) dans la reponse",
            pii_mapping.len()
        )
    };

    emit_unite(
        &state,
        UniteInput {
            request_id,
            action,
            acteur,
            contexte,
            ressource,
            relation,
            objectif,
            mission,
            method: method_str,
            path,
            status: status.as_u16(),
            latency_ms: start.elapsed().as_millis(),
            request_bytes,
            risques,
            realisation,
            pii_masked,
            pii_categories,
            virtual_key: resolved_key.as_ref().map(|k| k.key.clone()),
            tokens,
        },
    );

    (status, out_headers, Body::from(final_bytes)).into_response()
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
/// ⚠️ Extrait ici en clair : le masquage (Étape 3 puis Étape 4) est
/// appliqué juste après, plus loin dans `passthrough`. Retombe sur
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

fn emit_unite(state: &AppState, input: UniteInput) {
    let record = UniteRecord {
        request_id: input.request_id,
        timestamp_unix_ms: unite::now_unix_ms(),
        action: input.action,
        acteur: input.acteur,
        contexte: input.contexte,
        ressource: input.ressource,
        logs: LogsAxis {
            method: input.method,
            path: input.path,
            status: input.status,
            latency_ms: input.latency_ms,
            request_bytes: input.request_bytes,
        },
        risques: input.risques,
        relation: input.relation,
        realisation: input.realisation,
        objectif: input.objectif,
        mission: input.mission,
        pii_masked: input.pii_masked,
        pii_categories: input.pii_categories,
        virtual_key: input.virtual_key,
        tokens: input.tokens,
    };

    let http = state.http.clone();
    let store = state.unites.clone();
    let audit_log_path = state.audit_log_path.clone();
    tokio::spawn(async move {
        audit::append(&record, &audit_log_path).await;
        unite::emit(record, http, store).await;
    });
}
