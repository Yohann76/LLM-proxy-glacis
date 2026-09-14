# ProxyLLM Glacis — Observability & Compliance Data

## Objectif

ProxyLLM-glacis est un proxy qui intercepte les requêtes vers les LLM afin de :

- **Observer** les séquences d'appels LLM selon la méthode **Unité / Action / Risque**.
- Assurer une **Compliance** rendant le proxy utilisable en entreprise : masquage des données personnelles, filtrage de mots-clés sensibles, pour éviter tout transfert imprévu de données vers les États-Unis.

Les utilisateurs configurent eux-mêmes leurs fournisseurs LLM et leurs tokens d'API ; les coûts de token restent à leur charge.

## 1. Fonctionnalités & Moteur d'IA symbolique (valeur différenciante)

- **Règles symboliques dynamiques (Policy-as-Code)** : les administrateurs définissent des règles déterministes en YAML/JSON (ex. *Si l'utilisateur est un dev Junior et que l'Action = "DROP TABLE", ALORS Bloquer et Alerter*).
- **Détection et assainissement des données sensibles en temps réel (PII & secrets)** : au-delà du blocage pur, un mode de masquage dynamique (anonymisation/rédaction) remplace les tokens sensibles par des variables anonymes avant l'envoi au LLM, puis réinjecte les vraies valeurs dans la réponse.
- **Moteur d'auditabilité & rapports AI Act ("1-Click Compliance Report")** : génération automatique de rapports PDF/JSON certifiant la conformité des flux d'IA de l'entreprise aux exigences de l'EU AI Act (explicabilité, traçabilité, maîtrise des risques).

## 2. Architecture & Intégration (simplicité d'adoption)

- **Compatibilité native OpenAI API / OpenTelemetry** : le proxy agit en *drop-in replacement* — il suffit de changer l'URL de base (`OPENAI_BASE_URL`) dans les IDE (Cursor, VS Code) ou SDK, sans modifier le code métier.
- **Exécution asynchrone pour la télémétrie** : l'analyse du graphe de décision et l'écriture des logs d'audit s'exécutent en tâche de fond, garantissant une latence quasi nulle (< 5 ms) sur la réponse renvoyée au développeur.

## 3. Gestion FinOps & Administration (besoins DSI)

- **Quotas & budgets par équipe/utilisateur (chargeback)** : suivi détaillé des coûts de tokens par clé API virtuelle, avec plafonds mensuels et alertes de surconsommation.
- **Fallback / Failover automatique** : en cas de panne ou de latence excessive d'un fournisseur LLM (ex. OpenAI), la gateway bascule automatiquement la requête vers un modèle équivalent (ex. Mistral sur OVHcloud).

## Observability 

Analyser les flux LLM avec la méthode:
1 unité -> Acteur, Contexte, Ressource, Logs, Risques, Relation, Realisation-livrable, Objectif, Mission 

## Technologies

- Rust
- Docker
- Make (`make dev-run`, `make dev-kill`, `make dev-build`, …)
