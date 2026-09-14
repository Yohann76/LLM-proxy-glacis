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

- [ ] Endpoint `/v1/chat/completions` (et équivalents) en drop-in replacement
- [ ] Configuration des fournisseurs LLM + tokens côté proxy (fichier de config)
- [ ] Transfert de la requête au fournisseur configuré et relai de la réponse

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

**Prochaine étape à prioriser : Étape 1 (passthrough proxy) ou Étape 3 (moteur de règles), selon ce que tu veux poser en premier — à discuter.**
