//! Moteur de règles symboliques (Policy-as-Code) — Étape 3 de la roadmap.
//!
//! Charge des règles déterministes depuis un fichier YAML (§2.2 du cahier
//! des charges), évaluées de façon **synchrone** sur le chemin critique de
//! la requête : comparaisons de chaînes + regex précompilées au chargement
//! (jamais par requête), pour rester largement sous les 5 ms visés.
//!
//! Rechargement à chaud par scrutation périodique du fichier (mtime), sans
//! redémarrage du proxy. Un fichier invalide au démarrage est fatal (même
//! logique que `config/providers.yaml`) ; un rechargement à chaud invalide
//! ne casse jamais le jeu de règles courant (log + conservation de
//! l'ancien jeu).

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{path::PathBuf, sync::Arc, time::SystemTime};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Autoriser,
    Bloquer,
    Alerter,
    Masquer,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct RawConditions {
    acteur: Option<String>,
    acteur_contains: Option<String>,
    contexte: Option<String>,
    contexte_contains: Option<String>,
    provider: Option<String>,
    mission: Option<String>,
    mission_contains: Option<String>,
    objectif: Option<String>,
    objectif_contains: Option<String>,
    action_contains: Option<String>,
    action_matches: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawRule {
    name: String,
    #[serde(default)]
    when: RawConditions,
    then: Vec<RuleAction>,
    #[serde(default)]
    replacement: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawRuleFile {
    #[serde(default)]
    rules: Vec<RawRule>,
}

const DEFAULT_REPLACEMENT: &str = "[MASQUE]";

/// Règle avec ses éventuelles regex précompilées.
#[derive(Debug, Clone)]
struct CompiledRule {
    name: String,
    when: RawConditions,
    action_matches_re: Option<Regex>,
    actions: Vec<RuleAction>,
    replacement: String,
}

impl CompiledRule {
    fn from_raw(raw: RawRule) -> Result<Self, regex::Error> {
        let action_matches_re = raw
            .when
            .action_matches
            .as_deref()
            .map(Regex::new)
            .transpose()?;
        Ok(CompiledRule {
            name: raw.name,
            when: raw.when,
            action_matches_re,
            actions: raw.then,
            replacement: raw
                .replacement
                .unwrap_or_else(|| DEFAULT_REPLACEMENT.to_string()),
        })
    }

    fn matches(&self, facts: &Facts) -> bool {
        let w = &self.when;
        if let Some(v) = &w.acteur {
            if v != facts.acteur {
                return false;
            }
        }
        if let Some(v) = &w.acteur_contains {
            if !contains_ci(facts.acteur, v) {
                return false;
            }
        }
        if let Some(v) = &w.contexte {
            if v != facts.contexte {
                return false;
            }
        }
        if let Some(v) = &w.contexte_contains {
            if !contains_ci(facts.contexte, v) {
                return false;
            }
        }
        if let Some(v) = &w.provider {
            if v != facts.provider {
                return false;
            }
        }
        if let Some(v) = &w.mission {
            if v != facts.mission {
                return false;
            }
        }
        if let Some(v) = &w.mission_contains {
            if !contains_ci(facts.mission, v) {
                return false;
            }
        }
        if let Some(v) = &w.objectif {
            if v != facts.objectif {
                return false;
            }
        }
        if let Some(v) = &w.objectif_contains {
            if !contains_ci(facts.objectif, v) {
                return false;
            }
        }
        if let Some(v) = &w.action_contains {
            if !contains_ci(facts.action, v) {
                return false;
            }
        }
        if let Some(re) = &self.action_matches_re {
            if !re.is_match(facts.action) {
                return false;
            }
        }
        true
    }

    fn summary_json(&self) -> serde_json::Value {
        json!({
            "name": self.name,
            "then": self.actions,
            "when": self.when,
        })
    }
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Les données de la requête sur lesquelles les règles sont évaluées —
/// exactement les axes déjà calculés de façon synchrone pour l'unité
/// d'observabilité (§2.1 du cahier des charges), rien de plus.
pub struct Facts<'a> {
    pub acteur: &'a str,
    pub contexte: &'a str,
    pub provider: &'a str,
    pub mission: &'a str,
    pub objectif: &'a str,
    pub action: &'a str,
}

#[derive(Debug, Default)]
pub struct Decision {
    pub blocked_by: Option<String>,
    pub allowed_by: Option<String>,
    pub alerts: Vec<String>,
    /// Noms des règles "masquer" qui ont matché (introspection/API).
    pub mask_rules: Vec<String>,
    /// (regex, remplacement) correspondants, pour l'application réelle du
    /// masquage — séparé de `mask_rules` car `Regex` n'est pas Serialize.
    masks: Vec<(Regex, String)>,
}

impl Decision {
    /// Résumé lisible, utilisé comme axe Risques de l'unité d'observabilité
    /// — remplace le placeholder statique "non_evalue" de l'Étape 2.
    pub fn summary(&self) -> String {
        if let Some(name) = &self.blocked_by {
            return format!("bloque par la regle '{name}'");
        }
        let mut parts = Vec::new();
        if let Some(name) = &self.allowed_by {
            parts.push(format!("autorise explicitement par '{name}'"));
        }
        if !self.alerts.is_empty() {
            parts.push(format!("alerte(s) : {}", self.alerts.join(", ")));
        }
        if !self.masks.is_empty() {
            parts.push(format!("{} masquage(s) applique(s)", self.masks.len()));
        }
        if parts.is_empty() {
            "aucune regle declenchee".to_string()
        } else {
            parts.join(" ; ")
        }
    }

    pub fn has_masks(&self) -> bool {
        !self.masks.is_empty()
    }

    /// Applique tous les masquages de la décision à un texte (utilisé à la
    /// fois sur l'axe Action pour l'observabilité et sur le corps de la
    /// requête avant transfert au fournisseur).
    pub fn apply_masks(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (re, replacement) in &self.masks {
            out = re.replace_all(&out, replacement.as_str()).into_owned();
        }
        out
    }
}

/// Sémantique "premier match décisif gagne" : `autoriser` et `bloquer`
/// arrêtent l'évaluation ; `alerter` et `masquer` s'accumulent et laissent
/// l'évaluation continuer (une requête peut déclencher plusieurs alertes
/// et plusieurs masquages avant, éventuellement, d'être bloquée).
fn eval(rules: &[CompiledRule], facts: &Facts) -> Decision {
    let mut decision = Decision::default();
    for rule in rules {
        if !rule.matches(facts) {
            continue;
        }
        if rule.actions.contains(&RuleAction::Autoriser) {
            decision.allowed_by = Some(rule.name.clone());
            break;
        }
        if rule.actions.contains(&RuleAction::Bloquer) {
            decision.blocked_by = Some(rule.name.clone());
            break;
        }
        if rule.actions.contains(&RuleAction::Alerter) {
            decision.alerts.push(rule.name.clone());
        }
        if rule.actions.contains(&RuleAction::Masquer) {
            if let Some(re) = &rule.action_matches_re {
                decision.masks.push((re.clone(), rule.replacement.clone()));
                decision.mask_rules.push(rule.name.clone());
            }
        }
    }
    decision
}

#[derive(Clone)]
pub struct RuleEngine {
    path: PathBuf,
    rules: Arc<RwLock<Vec<CompiledRule>>>,
    last_loaded_mtime: Arc<RwLock<Option<SystemTime>>>,
}

impl RuleEngine {
    /// Premier chargement : échec fatal si le fichier est invalide (même
    /// logique qu'au démarrage pour `config/providers.yaml`).
    pub fn load_initial(path: PathBuf) -> Self {
        let (rules, mtime) = Self::read(&path)
            .unwrap_or_else(|e| panic!("fichier de règles invalide {}: {e}", path.display()));
        println!(
            "Règles chargées depuis {} : {} règle(s)",
            path.display(),
            rules.len()
        );
        RuleEngine {
            path,
            rules: Arc::new(RwLock::new(rules)),
            last_loaded_mtime: Arc::new(RwLock::new(mtime)),
        }
    }

    fn read(path: &PathBuf) -> Result<(Vec<CompiledRule>, Option<SystemTime>), String> {
        let mtime = std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok());
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let file: RawRuleFile = serde_yaml::from_str(&raw).map_err(|e| e.to_string())?;
        let mut compiled = Vec::with_capacity(file.rules.len());
        for r in file.rules {
            compiled.push(CompiledRule::from_raw(r).map_err(|e| e.to_string())?);
        }
        Ok((compiled, mtime))
    }

    /// Évalue les règles courantes contre les faits fournis. Verrou en
    /// lecture le temps de l'évaluation seulement (pas de clone du jeu de
    /// règles complet) : la contention avec un rechargement en cours est
    /// négligeable et rarissime.
    pub async fn evaluate(&self, facts: &Facts<'_>) -> Decision {
        let guard = self.rules.read().await;
        eval(&guard, facts)
    }

    pub async fn summary(&self) -> serde_json::Value {
        let guard = self.rules.read().await;
        json!({
            "count": guard.len(),
            "rules": guard.iter().map(CompiledRule::summary_json).collect::<Vec<_>>(),
        })
    }

    /// Recharge si le fichier a changé depuis le dernier chargement réussi.
    async fn reload_if_changed(&self) {
        let current_mtime = std::fs::metadata(&self.path)
            .ok()
            .and_then(|m| m.modified().ok());
        {
            let last = self.last_loaded_mtime.read().await;
            if *last == current_mtime {
                return;
            }
        }
        match Self::read(&self.path) {
            Ok((rules, mtime)) => {
                let count = rules.len();
                *self.rules.write().await = rules;
                *self.last_loaded_mtime.write().await = mtime;
                println!(
                    "Règles rechargées depuis {} : {} règle(s)",
                    self.path.display(),
                    count
                );
            }
            Err(e) => {
                eprintln!(
                    "échec du rechargement des règles ({}) : {e} — jeu de règles précédent conservé",
                    self.path.display()
                );
            }
        }
    }

    /// Tâche de fond : scrute périodiquement le fichier pour le
    /// rechargement à chaud.
    pub fn spawn_watcher(&self, interval: std::time::Duration) {
        let engine = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await; // le premier tick est immédiat : on le consomme sans recharger
            loop {
                ticker.tick().await;
                engine.reload_if_changed().await;
            }
        });
    }
}
