//! Clés API virtuelles & FinOps — Étape 6 de la roadmap.
//!
//! Contrairement aux fournisseurs (`providers.yaml`) et aux règles
//! (`rules.yaml`), les clés virtuelles ne sont pas une politique de
//! contenu : ce sont des identités authentifiées pour le suivi de
//! consommation et le chargeback.
//!
//! **Comportement par défaut (aucune clé configurée) : le proxy reste
//! ouvert**, exactement comme avant cette étape — pour ne rien casser tant
//! que l'admin n'a pas explicitement activé le contrôle d'accès. Dès
//! qu'une clé au moins est définie dans `config/virtual_keys.yaml`, le
//! proxy exige une clé valide (`Authorization: Bearer <clé>`) pour tout
//! appel `/v1/*` — 401 sinon.

use crate::unite::{TokenUsage, UniteRecord};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::SystemTime};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VirtualKeyConfig {
    pub key: String,
    pub name: String,
    /// Budget cumulé en tokens. `None` = pas de limite.
    #[serde(default)]
    pub quota_tokens: Option<u64>,
    /// Taux indicatif pour l'estimation de chargeback (défini par l'admin,
    /// pas le vrai tarif du fournisseur — cf. Docs/use.md).
    #[serde(default)]
    pub cost_per_1k_tokens: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawKeysFile {
    #[serde(default)]
    keys: Vec<VirtualKeyConfig>,
}

/// Masque une clé pour l'affichage admin : jamais la valeur brute, même
/// dans une interface interne — le proxy et l'admin sont exposés sans
/// authentification propre (cf. avertissements des étapes précédentes).
fn mask_key(key: &str) -> String {
    if key.chars().count() <= 4 {
        "****".to_string()
    } else {
        let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        format!("****{tail}")
    }
}

#[derive(Debug, Serialize)]
pub struct ChargebackEntry {
    pub name: String,
    pub key_masked: String,
    pub quota_tokens: Option<u64>,
    pub tokens_used: u64,
    pub requests: u64,
    pub percent_used: Option<f64>,
    pub estimated_cost: Option<f64>,
    pub quota_exceeded: bool,
    pub near_quota: bool,
}

const NEAR_QUOTA_RATIO: f64 = 0.8;

#[derive(Clone)]
pub struct VirtualKeyStore {
    path: PathBuf,
    keys: Arc<RwLock<Vec<VirtualKeyConfig>>>,
    last_loaded_mtime: Arc<RwLock<Option<SystemTime>>>,
    /// clé brute → tokens cumulés consommés.
    consumption: Arc<RwLock<HashMap<String, u64>>>,
    /// clé brute → nombre de requêtes.
    request_counts: Arc<RwLock<HashMap<String, u64>>>,
}

impl VirtualKeyStore {
    /// Premier chargement : un fichier absent équivaut à "aucune clé"
    /// (proxy ouvert), ce n'est pas une erreur. Un fichier présent mais
    /// invalide, en revanche, est fatal — même logique que les autres
    /// fichiers de config.
    pub fn load_initial(path: PathBuf) -> Self {
        let keys = Self::read(&path)
            .unwrap_or_else(|e| panic!("fichier de clés virtuelles invalide {}: {e}", path.display()));
        let mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
        if keys.is_empty() {
            println!("Clés virtuelles : aucune configurée dans {} — proxy ouvert (pas d'authentification requise)", path.display());
        } else {
            println!(
                "Clés virtuelles chargées depuis {} : {} clé(s) — authentification requise sur /v1/*",
                path.display(),
                keys.len()
            );
        }
        VirtualKeyStore {
            path,
            keys: Arc::new(RwLock::new(keys)),
            last_loaded_mtime: Arc::new(RwLock::new(mtime)),
            consumption: Arc::new(RwLock::new(HashMap::new())),
            request_counts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn read(path: &PathBuf) -> Result<Vec<VirtualKeyConfig>, String> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let file: RawKeysFile = serde_yaml::from_str(&raw).map_err(|e| e.to_string())?;
        Ok(file.keys)
    }

    /// Réhydrate les compteurs de consommation depuis le journal d'audit
    /// persistant (Étape 5) au démarrage — pas de mécanisme de persistance
    /// séparé, on réutilise celui qui existe déjà.
    pub async fn rehydrate_from_audit(&self, records: &[UniteRecord]) {
        let mut consumption = self.consumption.write().await;
        let mut counts = self.request_counts.write().await;
        let mut total = 0u64;
        for r in records {
            if let Some(vk) = &r.virtual_key {
                *consumption.entry(vk.clone()).or_insert(0) += r.tokens.total_tokens;
                *counts.entry(vk.clone()).or_insert(0) += 1;
                total += 1;
            }
        }
        if total > 0 {
            println!("FinOps : compteurs de consommation réhydratés depuis le journal d'audit ({total} appel(s) avec clé virtuelle)");
        }
    }

    pub async fn is_enforced(&self) -> bool {
        !self.keys.read().await.is_empty()
    }

    pub async fn resolve(&self, bearer: &str) -> Option<VirtualKeyConfig> {
        self.keys.read().await.iter().find(|k| k.key == bearer).cloned()
    }

    pub async fn consumption_of(&self, key: &str) -> u64 {
        *self.consumption.read().await.get(key).unwrap_or(&0)
    }

    pub async fn record_usage(&self, key: &str, tokens: u64) {
        *self.consumption.write().await.entry(key.to_string()).or_insert(0) += tokens;
        *self.request_counts.write().await.entry(key.to_string()).or_insert(0) += 1;
    }

    async fn reload_if_changed(&self) {
        let current_mtime = std::fs::metadata(&self.path).ok().and_then(|m| m.modified().ok());
        {
            let last = self.last_loaded_mtime.read().await;
            if *last == current_mtime {
                return;
            }
        }
        match Self::read(&self.path) {
            Ok(keys) => {
                let count = keys.len();
                *self.keys.write().await = keys;
                *self.last_loaded_mtime.write().await = current_mtime;
                println!(
                    "Clés virtuelles rechargées depuis {} : {} clé(s)",
                    self.path.display(),
                    count
                );
            }
            Err(e) => {
                eprintln!(
                    "échec du rechargement des clés virtuelles ({}) : {e} — jeu précédent conservé",
                    self.path.display()
                );
            }
        }
    }

    pub fn spawn_watcher(&self, interval: std::time::Duration) {
        let store = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                store.reload_if_changed().await;
            }
        });
    }

    /// Rapport chargeback : une ligne par clé configurée, valeurs brutes
    /// des clés jamais exposées (masquées).
    pub async fn chargeback_report(&self) -> Vec<ChargebackEntry> {
        let keys = self.keys.read().await;
        let consumption = self.consumption.read().await;
        let counts = self.request_counts.read().await;
        keys.iter()
            .map(|k| {
                let used = *consumption.get(&k.key).unwrap_or(&0);
                let requests = *counts.get(&k.key).unwrap_or(&0);
                let percent_used = k.quota_tokens.map(|q| {
                    if q == 0 {
                        100.0
                    } else {
                        (used as f64 / q as f64) * 100.0
                    }
                });
                ChargebackEntry {
                    name: k.name.clone(),
                    key_masked: mask_key(&k.key),
                    quota_tokens: k.quota_tokens,
                    tokens_used: used,
                    requests,
                    percent_used,
                    estimated_cost: k
                        .cost_per_1k_tokens
                        .map(|rate| (used as f64 / 1000.0) * rate),
                    quota_exceeded: k.quota_tokens.is_some_and(|q| used >= q),
                    near_quota: k
                        .quota_tokens
                        .is_some_and(|q| q > 0 && used < q && (used as f64) >= (q as f64) * NEAR_QUOTA_RATIO),
                }
            })
            .collect()
    }
}

pub fn extract_usage(bytes: &[u8]) -> Option<TokenUsage> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let usage = value.get("usage")?;
    Some(TokenUsage {
        prompt_tokens: usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        completion_tokens: usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        total_tokens: usage.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
    })
}
