//! Moteur d'observabilité — méthode "Unité" (cf. Docs/cahier-des-charges.md §2.1).
//!
//! Chaque appel LLM relayé par le proxy est décomposé selon 9 axes
//! (Acteur, Contexte, Ressource, Logs, Risques, Relation, Réalisation,
//! Objectif, Mission), écrit en JSON structuré sur stdout et exporté en
//! OTLP/HTTP si `OTEL_EXPORTER_OTLP_ENDPOINT` est défini.
//!
//! L'émission est faite depuis une tâche asynchrone spawnée par l'appelant :
//! ce module n'ajoute donc aucune latence au chemin critique de la réponse
//! renvoyée au client.

use serde::Serialize;
use serde_json::json;
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Nombre maximum d'unités conservées en mémoire pour la vue Observabilité
/// de l'admin. Aucune persistance : un redémarrage du proxy vide l'historique.
pub const HISTORY_CAPACITY: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub struct LogsAxis {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub latency_ms: u128,
    pub request_bytes: usize,
}

/// Une "Unité" d'observabilité : la lecture structurée d'un appel LLM.
#[derive(Debug, Clone, Serialize)]
pub struct UniteRecord {
    pub request_id: String,
    pub timestamp_unix_ms: u128,

    /// Verbe + objet de l'intention réelle, extrait du dernier message
    /// utilisateur du prompt (voir `extract_action` dans main.rs). Déjà
    /// masqué (Étape 3 + Étape 4) au moment où l'unité est construite : ce
    /// qui apparaît ici est ce qui a été réellement envoyé au fournisseur,
    /// jamais le texte brut si une règle ou une détection PII a matché.
    pub action: String,
    pub acteur: String,
    pub contexte: String,
    pub ressource: String,
    pub logs: LogsAxis,
    pub risques: String,
    pub relation: String,
    pub realisation: String,
    pub objectif: String,
    pub mission: String,

    /// Étape 4 : au moins une donnée personnelle/secret détectée et
    /// masquée dans cet appel (indépendamment des règles de l'Étape 3).
    pub pii_masked: bool,
    /// Catégories détectées (ex. "EMAIL", "SECRET_API") — vide si
    /// `pii_masked` est faux.
    pub pii_categories: Vec<String>,
}

pub fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn new_request_id() -> String {
    Uuid::new_v4().simple().to_string()
}

/// Historique borné des dernières unités, consultable par l'admin via
/// `GET /internal/unites`. Partagé entre les tâches via `Arc<Mutex<_>>`.
#[derive(Clone)]
pub struct Store(Arc<Mutex<VecDeque<UniteRecord>>>);

impl Store {
    pub fn new() -> Self {
        Store(Arc::new(Mutex::new(VecDeque::with_capacity(HISTORY_CAPACITY))))
    }

    async fn push(&self, record: UniteRecord) {
        let mut guard = self.0.lock().await;
        if guard.len() == HISTORY_CAPACITY {
            guard.pop_front();
        }
        guard.push_back(record);
    }

    /// Les `limit` unités les plus récentes, les plus récentes en premier.
    pub async fn recent(&self, limit: usize) -> Vec<UniteRecord> {
        let guard = self.0.lock().await;
        guard.iter().rev().take(limit).cloned().collect()
    }
}

/// Écrit l'unité en JSON structuré sur stdout, l'ajoute à l'historique en
/// mémoire (pour la vue Observabilité de l'admin) puis, si configuré,
/// l'exporte en OTLP/HTTP (JSON) vers un collecteur OpenTelemetry.
pub async fn emit(record: UniteRecord, http: reqwest::Client, store: Store) {
    match serde_json::to_string(&record) {
        Ok(line) => println!("{line}"),
        Err(e) => eprintln!("échec de sérialisation de l'unité d'observabilité : {e}"),
    }

    store.push(record.clone()).await;

    let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") else {
        return;
    };

    if let Err(e) = export_otlp(&record, &endpoint, &http).await {
        eprintln!("export OpenTelemetry échoué vers {endpoint} : {e}");
    }
}

/// Construit et envoie un span OTLP/HTTP (encodage JSON du schéma
/// opentelemetry-proto) porteur des 9 axes en attributs. Implémentation
/// volontairement minimale (pas de SDK opentelemetry officiel) pour rester
/// simple et sans dépendance lourde (tonic/protoc).
async fn export_otlp(
    record: &UniteRecord,
    endpoint: &str,
    http: &reqwest::Client,
) -> Result<(), reqwest::Error> {
    let trace_id = Uuid::new_v4().simple().to_string();
    let span_id = Uuid::new_v4().simple().to_string()[..16].to_string();

    let start_ns = record.timestamp_unix_ms * 1_000_000;
    let end_ns = start_ns + (record.logs.latency_ms * 1_000_000);
    let status_code_otlp = if record.logs.status < 400 { 1 } else { 2 };

    let payload = json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [
                    { "key": "service.name", "value": { "stringValue": "proxyllm-proxy" } }
                ]
            },
            "scopeSpans": [{
                "scope": { "name": "proxyllm.observability" },
                "spans": [{
                    "traceId": trace_id,
                    "spanId": span_id,
                    "name": "llm.request",
                    "kind": 3,
                    "startTimeUnixNano": start_ns.to_string(),
                    "endTimeUnixNano": end_ns.to_string(),
                    "attributes": [
                        { "key": "proxyllm.request_id", "value": { "stringValue": record.request_id.clone() } },
                        { "key": "proxyllm.action", "value": { "stringValue": record.action.clone() } },
                        { "key": "proxyllm.acteur", "value": { "stringValue": record.acteur.clone() } },
                        { "key": "proxyllm.contexte", "value": { "stringValue": record.contexte.clone() } },
                        { "key": "proxyllm.ressource", "value": { "stringValue": record.ressource.clone() } },
                        { "key": "proxyllm.risques", "value": { "stringValue": record.risques.clone() } },
                        { "key": "proxyllm.relation", "value": { "stringValue": record.relation.clone() } },
                        { "key": "proxyllm.realisation", "value": { "stringValue": record.realisation.clone() } },
                        { "key": "proxyllm.objectif", "value": { "stringValue": record.objectif.clone() } },
                        { "key": "proxyllm.mission", "value": { "stringValue": record.mission.clone() } },
                        { "key": "http.method", "value": { "stringValue": record.logs.method.clone() } },
                        { "key": "http.target", "value": { "stringValue": record.logs.path.clone() } },
                        { "key": "http.status_code", "value": { "intValue": record.logs.status.to_string() } },
                        { "key": "proxyllm.pii_masked", "value": { "boolValue": record.pii_masked } },
                        { "key": "proxyllm.pii_categories", "value": { "stringValue": record.pii_categories.join(",") } }
                    ],
                    "status": { "code": status_code_otlp }
                }]
            }]
        }]
    });

    let url = format!("{}/v1/traces", endpoint.trim_end_matches('/'));
    http.post(url).json(&payload).send().await?;
    Ok(())
}
