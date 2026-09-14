# Roadmap — ProxyLLM Glacis

Suivi de l'avancement par étape. Coché = fait, décoché = à faire.

## Étape 0 — Infrastructure

- [x] Dockerfile proxy (multi-stage, Rust → debian-slim)
- [x] Dockerfile admin (multi-stage, Rust → debian-slim)
- [x] docker-compose.yml (services `proxy` + `admin`, ports exotiques 45321/45322)
- [x] Makefile (`dev-build`, `dev-run`, `dev-kill`, `dev-shell`)
- [x] Squelette `proxy` : serveur axum, endpoint `/healthz`
- [x] Squelette `admin` : serveur axum, dashboard statique (`/healthz` + sections à venir)

**État actuel : squelette fonctionnel, aucune logique métier implémentée.**

## Étape 1 — Passthrough proxy compatible OpenAI API

- [x] Endpoint générique `/v1/*` (chat/completions et tout autre endpoint) en drop-in replacement
- [x] Configuration des fournisseurs LLM + tokens côté proxy (`config/providers.yaml`, monté en volume, clés via `.env`)
- [x] Transfert de la requête au fournisseur configuré et relai de la réponse (méthode, path+query, body, headers, streaming)

**Fait le 2026-09-14.** Sélection du fournisseur via l'en-tête `X-ProxyLLM-Provider` (sinon `default_provider`). La clé `Authorization` envoyée par le client est ignorée : le proxy injecte systématiquement la clé configurée côté serveur (`api_key_env`). Validé avec un fournisseur mock sur le réseau Docker (GET, POST + body JSON, en-têtes).

- [x] Interface de test dans l'admin (`/test.html` + route serveur `POST /api/test` qui relaie vers le proxy) pour tester l'API sans terminal.
- [x] Champ clé API optionnel dans l'interface de test (en-tête `X-ProxyLLM-Api-Key`, surcharge `.env` pour l'appel en cours, sans persistance).

## Étape 2 — Moteur d'observabilité (méthode Unité)

- [ ] Extraction des 9 axes (Acteur, Contexte, Ressource, Logs, Risques, Relation, Réalisation, Objectif, Mission) par requête
- [ ] Écriture des logs structurés en tâche asynchrone
- [ ] Export OpenTelemetry

## Étape 3 — Moteur de règles symboliques (Policy-as-Code)

- [ ] Format des règles YAML/JSON
- [ ] Chargement/rechargement à chaud des règles
- [ ] Évaluation des règles sur le chemin synchrone (< 5 ms)
- [ ] Actions : Bloquer / Alerter / Masquer / Autoriser

## Étape 4 — Détection & masquage PII/secrets

- [ ] Détection des données personnelles et secrets dans les requêtes sortantes
- [ ] Masquage dynamique (remplacement par variables anonymes)
- [ ] Réinjection des vraies valeurs dans la réponse

## Étape 5 — Auditabilité & conformité AI Act

- [ ] Historisation des décisions (règle appliquée, acteur, résultat)
- [ ] Génération de rapport (1-Click Compliance Report) en PDF/JSON

## Étape 6 — FinOps & administration

- [ ] Clés API virtuelles
- [ ] Suivi de consommation de tokens par acteur/équipe
- [ ] Quotas, budgets, alertes de surconsommation
- [ ] Vue chargeback

## Étape 7 — Fallback / Failover

- [ ] Détection de panne/latence d'un fournisseur
- [ ] Bascule automatique vers un fournisseur/modèle équivalent
- [ ] Configuration des correspondances de modèles

## Étape 8 — Interface admin (dashboard)

- [ ] Vue Règles (éditeur Policy-as-Code)
- [ ] Vue Observabilité (exploration des séquences par la grille Unité)
- [ ] Vue Compliance (génération/téléchargement des rapports)
- [ ] Vue FinOps (coûts, quotas, alertes)
- [ ] Vue Fournisseurs (config LLM + fallback)
- [ ] Gestion des accès (clés API virtuelles)

---

**Prochaine étape à prioriser : Étape 2 (observabilité) ou Étape 3 (moteur de règles), selon ce que tu veux poser en premier — à discuter.**
