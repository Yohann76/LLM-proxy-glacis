//! Auditabilité & conformité — Étape 5 de la roadmap.
//!
//! Deux volets :
//! 1. **Historisation persistante** des décisions : contrairement à
//!    l'historique en mémoire de l'Étape 2 (`unite::Store`, 200 entrées,
//!    perdu au redémarrage), chaque unité est aussi ajoutée à un journal
//!    JSONL sur disque (`data/audit.jsonl`, monté en volume) — une ligne
//!    JSON par appel, append-only, jamais réécrit. C'est la source de
//!    vérité pour le rapport de conformité, qui doit survivre à un
//!    redémarrage du conteneur.
//! 2. **Génération de rapport** ("1-Click Compliance Report", cf. cahier
//!    des charges §2.4) en JSON et en PDF, à partir de ce journal :
//!    volumétrie, décisions de blocage, alertes, détections PII — de quoi
//!    répondre aux exigences de traçabilité de l'EU AI Act.

use crate::unite::UniteRecord;
use printpdf::{BuiltinFont, Mm, PdfDocument};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};
use tokio::io::AsyncWriteExt;

/// Ajoute une unité au journal d'audit persistant. Appelé depuis la même
/// tâche asynchrone que l'émission des logs (§Étape 2) : hors du chemin
/// critique, aucun impact sur la latence de réponse.
pub async fn append(record: &UniteRecord, path: &Path) {
    if let Some(parent) = path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            eprintln!("impossible de créer {} pour le journal d'audit : {e}", parent.display());
            return;
        }
    }
    let Ok(line) = serde_json::to_string(record) else {
        eprintln!("échec de sérialisation d'une unité pour le journal d'audit");
        return;
    };
    match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
    {
        Ok(mut file) => {
            let _ = file.write_all(line.as_bytes()).await;
            let _ = file.write_all(b"\n").await;
        }
        Err(e) => eprintln!("échec d'écriture du journal d'audit ({}) : {e}", path.display()),
    }
}

/// Lit l'intégralité du journal d'audit. Fichier absent ou vide → liste
/// vide (pas d'erreur : le journal n'existe simplement pas encore).
pub fn read_all(path: &Path) -> Vec<UniteRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[derive(Debug, Serialize, Clone)]
pub struct EventSummary {
    pub timestamp_unix_ms: u128,
    pub request_id: String,
    pub acteur: String,
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct ComplianceReport {
    pub generated_at_unix_ms: u128,
    pub period_from_unix_ms: Option<u128>,
    pub period_to_unix_ms: Option<u128>,
    pub total_requests: usize,
    pub blocked_count: usize,
    pub alert_count: usize,
    pub pii_masked_count: usize,
    pub error_count: usize,
    pub by_provider: BTreeMap<String, usize>,
    pub by_mission: BTreeMap<String, usize>,
    pub pii_categories: BTreeMap<String, usize>,
    pub blocked_events: Vec<EventSummary>,
    pub alert_events: Vec<EventSummary>,
    pub pii_events: Vec<EventSummary>,
}

const MAX_EVENTS_LISTED: usize = 30;

/// Agrège le journal d'audit en rapport de conformité. Filtre optionnel sur
/// une période (`since`/`until`, timestamps unix en ms).
pub fn compute_report(
    records: &[UniteRecord],
    since: Option<u128>,
    until: Option<u128>,
) -> ComplianceReport {
    let filtered: Vec<&UniteRecord> = records
        .iter()
        .filter(|r| since.map_or(true, |s| r.timestamp_unix_ms >= s))
        .filter(|r| until.map_or(true, |u| r.timestamp_unix_ms <= u))
        .collect();

    let mut by_provider = BTreeMap::new();
    let mut by_mission = BTreeMap::new();
    let mut pii_categories = BTreeMap::new();
    let mut blocked_events = Vec::new();
    let mut alert_events = Vec::new();
    let mut pii_events = Vec::new();
    let mut blocked_count = 0;
    let mut alert_count = 0;
    let mut pii_masked_count = 0;
    let mut error_count = 0;

    for r in &filtered {
        let provider = r
            .ressource
            .split('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("inconnu")
            .to_string();
        *by_provider.entry(provider).or_insert(0) += 1;
        *by_mission.entry(r.mission.clone()).or_insert(0) += 1;

        if r.risques.starts_with("bloque par la regle") {
            blocked_count += 1;
            blocked_events.push(EventSummary {
                timestamp_unix_ms: r.timestamp_unix_ms,
                request_id: r.request_id.clone(),
                acteur: r.acteur.clone(),
                action: r.action.clone(),
                detail: r.risques.clone(),
            });
        }
        if r.risques.contains("alerte(s)") {
            alert_count += 1;
            alert_events.push(EventSummary {
                timestamp_unix_ms: r.timestamp_unix_ms,
                request_id: r.request_id.clone(),
                acteur: r.acteur.clone(),
                action: r.action.clone(),
                detail: r.risques.clone(),
            });
        }
        if r.pii_masked {
            pii_masked_count += 1;
            for cat in &r.pii_categories {
                *pii_categories.entry(cat.clone()).or_insert(0) += 1;
            }
            pii_events.push(EventSummary {
                timestamp_unix_ms: r.timestamp_unix_ms,
                request_id: r.request_id.clone(),
                acteur: r.acteur.clone(),
                action: r.action.clone(),
                detail: r.pii_categories.join(", "),
            });
        }
        if r.logs.status >= 500 || r.realisation.starts_with("erreur") {
            error_count += 1;
        }
    }

    for events in [&mut blocked_events, &mut alert_events, &mut pii_events] {
        events.sort_by(|a, b| b.timestamp_unix_ms.cmp(&a.timestamp_unix_ms));
        events.truncate(MAX_EVENTS_LISTED);
    }

    ComplianceReport {
        generated_at_unix_ms: crate::unite::now_unix_ms(),
        period_from_unix_ms: since,
        period_to_unix_ms: until,
        total_requests: filtered.len(),
        blocked_count,
        alert_count,
        pii_masked_count,
        error_count,
        by_provider,
        by_mission,
        pii_categories,
        blocked_events,
        alert_events,
        pii_events,
    }
}

/// Convertit un timestamp unix (ms) en date lisible UTC, sans dépendance
/// externe (algorithme classique civil_from_days de Howard Hinnant).
fn format_ts(unix_ms: u128) -> String {
    let secs = (unix_ms / 1000) as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Rend le rapport en PDF (une ou plusieurs pages A4, texte simple — pas de
/// mise en page élaborée, l'objectif est la traçabilité, pas le design).
pub fn render_pdf(report: &ComplianceReport) -> Vec<u8> {
    let (doc, page1, layer1) = PdfDocument::new(
        "ProxyLLM Glacis — Rapport de conformité",
        Mm(210.0),
        Mm(297.0),
        "Layer",
    );
    let font = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .expect("police PDF standard indisponible");
    let font_bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .expect("police PDF standard indisponible");

    let margin_left = Mm(18.0);
    let top = 280.0;
    let bottom_margin = 15.0;
    let mut y = top;
    let mut layer = doc.get_page(page1).get_layer(layer1);

    let new_page = |doc: &printpdf::PdfDocumentReference, y: &mut f32| {
        let (page, l) = doc.add_page(Mm(210.0), Mm(297.0), "Layer");
        *y = top;
        doc.get_page(page).get_layer(l)
    };

    macro_rules! line {
        ($text:expr, $size:expr, $bold:expr) => {{
            if y < bottom_margin {
                layer = new_page(&doc, &mut y);
            }
            layer.use_text($text, $size, margin_left, Mm(y), if $bold { &font_bold } else { &font });
            y -= if $size >= 14.0 { 8.0 } else { 5.5 };
        }};
    }

    line!("ProxyLLM Glacis — Rapport de conformité", 16.0, true);
    line!(&format!("Généré le {}", format_ts(report.generated_at_unix_ms)), 9.0, false);
    let period = match (report.period_from_unix_ms, report.period_to_unix_ms) {
        (None, None) => "Toute la période disponible dans le journal d'audit".to_string(),
        (from, until) => format!(
            "{} → {}",
            from.map(format_ts).unwrap_or_else(|| "début".to_string()),
            until.map(format_ts).unwrap_or_else(|| "maintenant".to_string())
        ),
    };
    line!(&format!("Période : {period}"), 9.0, false);
    y -= 4.0;

    line!("Résumé", 13.0, true);
    line!(&format!("Requêtes totales : {}", report.total_requests), 10.5, false);
    line!(&format!("Bloquées par une règle : {}", report.blocked_count), 10.5, false);
    line!(&format!("Alertes déclenchées : {}", report.alert_count), 10.5, false);
    line!(&format!("Appels avec PII/secrets masqués : {}", report.pii_masked_count), 10.5, false);
    line!(&format!("Erreurs fournisseur/serveur : {}", report.error_count), 10.5, false);
    y -= 4.0;

    line!("Répartition par fournisseur", 13.0, true);
    if report.by_provider.is_empty() {
        line!("(aucune donnée)", 10.0, false);
    }
    for (k, v) in &report.by_provider {
        line!(&format!("{k} : {v}"), 10.0, false);
    }
    y -= 4.0;

    line!("Répartition par mission", 13.0, true);
    if report.by_mission.is_empty() {
        line!("(aucune donnée)", 10.0, false);
    }
    for (k, v) in &report.by_mission {
        line!(&format!("{k} : {v}"), 10.0, false);
    }
    y -= 4.0;

    if !report.pii_categories.is_empty() {
        line!("Catégories PII/secrets détectées", 13.0, true);
        for (k, v) in &report.pii_categories {
            line!(&format!("{k} : {v}"), 10.0, false);
        }
        y -= 4.0;
    }

    line!(
        &format!("Événements bloqués (règles) — {} le(s) plus récent(s)", report.blocked_events.len()),
        13.0,
        true
    );
    if report.blocked_events.is_empty() {
        line!("(aucun)", 10.0, false);
    }
    for e in &report.blocked_events {
        line!(
            &format!("{} — {} — {}", format_ts(e.timestamp_unix_ms), truncate(&e.acteur, 20), truncate(&e.detail, 55)),
            9.0,
            false
        );
    }
    y -= 4.0;

    line!(
        &format!("Événements avec PII/secrets détectés — {} le(s) plus récent(s)", report.pii_events.len()),
        13.0,
        true
    );
    if report.pii_events.is_empty() {
        line!("(aucun)", 10.0, false);
    }
    for e in &report.pii_events {
        line!(
            &format!("{} — {} — {}", format_ts(e.timestamp_unix_ms), truncate(&e.acteur, 20), truncate(&e.detail, 55)),
            9.0,
            false
        );
    }

    doc.save_to_bytes().expect("échec de génération du PDF")
}
