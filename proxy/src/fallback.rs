//! Fallback / Failover entre fournisseurs — Étape 7 de la roadmap.
//!
//! Détecte une panne ou une lenteur excessive du fournisseur primaire et
//! bascule automatiquement vers un fournisseur/modèle équivalent, selon des
//! correspondances configurées dans `config/fallback.yaml` (même schéma de
//! rechargement à chaud que `rules.yaml`).
//!
//! **Ce qui compte comme "panne/latence"** : une erreur réseau, une erreur
//! serveur (5xx) du fournisseur, ou un délai dépassé **avant même de
//! recevoir les en-têtes de réponse**. Le délai ne borne volontairement PAS
//! la lecture du corps de la réponse (streaming), pour ne pas interrompre
//! une complétion longue mais qui répond normalement — seul le temps
//! d'attente de la première réponse est surveillé.

use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, time::{Duration, SystemTime}};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FallbackChain {
    pub primary_provider: String,
    /// `None` = s'applique à n'importe quel modèle de ce fournisseur.
    #[serde(default)]
    pub primary_model: Option<String>,
    pub fallback_provider: String,
    /// `None` = conserve le même nom de modèle chez le fournisseur de repli.
    #[serde(default)]
    pub fallback_model: Option<String>,
}

fn default_timeout_ms() -> u64 {
    10_000
}

#[derive(Debug, Clone, Deserialize)]
struct RawFallbackFile {
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
    #[serde(default)]
    chains: Vec<FallbackChain>,
}

impl Default for RawFallbackFile {
    fn default() -> Self {
        RawFallbackFile { timeout_ms: default_timeout_ms(), chains: Vec::new() }
    }
}

#[derive(Clone)]
pub struct FallbackStore {
    path: PathBuf,
    timeout_ms: Arc<RwLock<u64>>,
    chains: Arc<RwLock<Vec<FallbackChain>>>,
    last_loaded_mtime: Arc<RwLock<Option<SystemTime>>>,
}

impl FallbackStore {
    /// Un fichier absent équivaut à "aucun fallback configuré" (pas une
    /// erreur) ; un fichier présent mais invalide est fatal, comme les
    /// autres fichiers de config.
    pub fn load_initial(path: PathBuf) -> Self {
        let (timeout_ms, chains) = Self::read(&path)
            .unwrap_or_else(|e| panic!("fichier de fallback invalide {}: {e}", path.display()));
        let mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
        println!(
            "Fallback : {} correspondance(s) chargée(s) depuis {} (timeout {timeout_ms} ms)",
            chains.len(),
            path.display()
        );
        FallbackStore {
            path,
            timeout_ms: Arc::new(RwLock::new(timeout_ms)),
            chains: Arc::new(RwLock::new(chains)),
            last_loaded_mtime: Arc::new(RwLock::new(mtime)),
        }
    }

    fn read(path: &PathBuf) -> Result<(u64, Vec<FallbackChain>), String> {
        if !path.exists() {
            return Ok((default_timeout_ms(), Vec::new()));
        }
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let file: RawFallbackFile = serde_yaml::from_str(&raw).map_err(|e| e.to_string())?;
        Ok((file.timeout_ms, file.chains))
    }

    pub async fn timeout(&self) -> Duration {
        Duration::from_millis(*self.timeout_ms.read().await)
    }

    /// Cherche une correspondance pour `provider` (+ `model` si la règle en
    /// exige un précis). Une règle sans `primary_model` matche n'importe
    /// quel modèle de ce fournisseur.
    pub async fn resolve(&self, provider: &str, model: Option<&str>) -> Option<FallbackChain> {
        self.chains
            .read()
            .await
            .iter()
            .find(|c| {
                c.primary_provider == provider
                    && match (&c.primary_model, model) {
                        (None, _) => true,
                        (Some(m), Some(actual)) => m == actual,
                        (Some(_), None) => false,
                    }
            })
            .cloned()
    }

    pub async fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "timeout_ms": *self.timeout_ms.read().await,
            "chains": *self.chains.read().await,
        })
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
            Ok((timeout_ms, chains)) => {
                let count = chains.len();
                *self.timeout_ms.write().await = timeout_ms;
                *self.chains.write().await = chains;
                *self.last_loaded_mtime.write().await = current_mtime;
                println!(
                    "Fallback rechargé depuis {} : {count} correspondance(s), timeout {timeout_ms} ms",
                    self.path.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "échec du rechargement du fallback ({}) : {e} — configuration précédente conservée",
                    self.path.display()
                );
            }
        }
    }

    pub fn spawn_watcher(&self, interval: Duration) {
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
}

/// Remplace (ou ajoute) le champ `"model"` d'un corps JSON. Utilisé pour
/// adapter la requête au modèle du fournisseur de repli. Best-effort :
/// retourne le corps inchangé si ce n'est pas du JSON exploitable.
pub fn substitute_model(body: &[u8], new_model: &str) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.to_vec();
    };
    let Some(obj) = value.as_object_mut() else {
        return body.to_vec();
    };
    obj.insert("model".to_string(), serde_json::Value::String(new_model.to_string()));
    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}
