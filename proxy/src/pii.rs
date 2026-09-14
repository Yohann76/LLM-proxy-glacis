//! Détection & masquage dynamique des données personnelles et secrets —
//! Étape 4 de la roadmap.
//!
//! Contrairement au moteur de règles (Étape 3, motifs écrits à la main par
//! l'admin dans `rules.yaml`), les catégories ici sont intégrées et actives
//! par défaut : c'est un filet de sécurité générique, pas une politique
//! métier à écrire. Chaque occurrence détectée est remplacée par une
//! **variable anonyme** numérotée (ex. `[EMAIL_1]`, `[EMAIL_2]`) ; la
//! correspondance placeholder → valeur réelle n'est jamais persistée nulle
//! part — elle vit le temps de la requête, dans une `HashMap` locale, pour
//! réinjecter les vraies valeurs dans la réponse du fournisseur avant de la
//! renvoyer au client (cf. `reinject`).
//!
//! Détection best-effort par regex (+ checksum quand c'en existe un
//! standard, IBAN et carte bancaire) : ni exhaustive, ni infaillible — un
//! filet de sécurité raisonnable, pas une garantie absolue.

use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

fn secret_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(?:sk-[A-Za-z0-9]{16,}|AKIA[0-9A-Z]{16}|ghp_[A-Za-z0-9]{20,}|xox[baprs]-[A-Za-z0-9-]{10,})\b")
            .expect("regex secret invalide")
    })
}

fn email_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[\w.+-]+@[\w-]+\.[\w.-]+").expect("regex email invalide"))
}

fn iban_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b").expect("regex IBAN invalide"))
}

fn carte_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b\d(?:[ -]?\d){12,18}\b").expect("regex carte bancaire invalide")
    })
}

fn telephone_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:\+33[\s.-]?|0)[1-9](?:[\s.-]?\d{2}){4}").expect("regex téléphone invalide")
    })
}

/// Validation Luhn (mod 10) — réduit les faux positifs de `carte_re`, qui
/// sinon matcherait n'importe quelle longue séquence de chiffres.
fn luhn_valid(matched: &str) -> bool {
    let digits: Vec<u32> = matched.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    let mut double = false;
    for &d in digits.iter().rev() {
        let mut d = d;
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    sum % 10 == 0
}

/// Validation IBAN par mod 97 (norme ISO 7064) — réduit les faux positifs
/// de `iban_re`.
fn iban_valid(matched: &str) -> bool {
    let cleaned: String = matched.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if cleaned.len() < 15 || cleaned.len() > 34 {
        return false;
    }
    let (head, tail) = cleaned.split_at(4);
    let rearranged = format!("{tail}{head}");

    let mut remainder: u64 = 0;
    for c in rearranged.chars() {
        let value = if c.is_ascii_digit() {
            c.to_digit(10).unwrap() as u64
        } else if c.is_ascii_uppercase() {
            (c as u64) - ('A' as u64) + 10
        } else {
            return false;
        };
        let digits = if value >= 10 { 2 } else { 1 };
        remainder = (remainder * 10u64.pow(digits) + value) % 97;
    }
    remainder == 1
}

#[derive(Debug, Default)]
pub struct MaskResult {
    pub text: String,
    /// placeholder (ex. "[EMAIL_1]") → valeur réelle. Jamais sérialisé,
    /// jamais journalisé : vit le temps de la requête pour `reinject`.
    pub mapping: HashMap<String, String>,
    /// Catégories effectivement détectées (pour l'observabilité).
    pub categories: Vec<String>,
}

/// Détecte et masque les PII/secrets dans un texte. Catégories traitées
/// des plus spécifiques aux plus génériques (un secret API ou un IBAN
/// masqué en premier ne pollue pas la détection de carte bancaire qui suit
/// sur le même texte).
pub fn mask(text: &str) -> MaskResult {
    let mut out = text.to_string();
    let mut mapping = HashMap::new();
    let mut categories = Vec::new();
    let mut counters: HashMap<&'static str, usize> = HashMap::new();

    out = mask_category(&out, "SECRET_API", secret_re(), &mut mapping, &mut counters, &mut categories, |_| true);
    out = mask_category(&out, "EMAIL", email_re(), &mut mapping, &mut counters, &mut categories, |_| true);
    out = mask_category(&out, "IBAN", iban_re(), &mut mapping, &mut counters, &mut categories, iban_valid);
    out = mask_category(&out, "CARTE_BANCAIRE", carte_re(), &mut mapping, &mut counters, &mut categories, luhn_valid);
    out = mask_category(&out, "TELEPHONE", telephone_re(), &mut mapping, &mut counters, &mut categories, |_| true);

    MaskResult { text: out, mapping, categories }
}

#[allow(clippy::too_many_arguments)]
fn mask_category(
    text: &str,
    category: &'static str,
    re: &Regex,
    mapping: &mut HashMap<String, String>,
    counters: &mut HashMap<&'static str, usize>,
    categories: &mut Vec<String>,
    validate: impl Fn(&str) -> bool,
) -> String {
    let mut hit = false;
    let replaced = re.replace_all(text, |caps: &regex::Captures| {
        let matched = caps.get(0).unwrap().as_str();
        if !validate(matched) {
            return matched.to_string();
        }
        let counter = counters.entry(category).or_insert(0);
        *counter += 1;
        let placeholder = format!("[{category}_{counter}]");
        mapping.insert(placeholder.clone(), matched.to_string());
        hit = true;
        placeholder
    });
    if hit {
        categories.push(category.to_string());
    }
    replaced.into_owned()
}

/// Remplace les variables anonymes (`[EMAIL_1]`, ...) par les vraies
/// valeurs dans la réponse du fournisseur, si celui-ci les a reprises
/// telles quelles dans sa sortie.
pub fn reinject(text: &str, mapping: &HashMap<String, String>) -> String {
    let mut out = text.to_string();
    for (placeholder, original) in mapping {
        out = out.replace(placeholder.as_str(), original.as_str());
    }
    out
}
