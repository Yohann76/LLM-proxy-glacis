# Cahier des charges — ProxyLLM Glacis

## 1. Présentation du projet

### 1.1 Contexte

Les entreprises adoptent massivement les LLM (via API OpenAI, Mistral, etc.) sans disposer de visibilité ni de contrôle sur les échanges de données qui transitent par ces outils (IDE type Cursor/VS Code, SDK internes, agents…). Deux risques en découlent : l'absence d'observabilité des usages, et la fuite potentielle de données sensibles vers des fournisseurs hébergés hors UE (États-Unis notamment), en contradiction avec les exigences de conformité (RGPD, EU AI Act).

### 1.2 Objectif

Construire **ProxyLLM Glacis**, un proxy qui s'intercale entre les clients (IDE, SDK, agents) et les fournisseurs LLM pour :

- **Observer** chaque séquence d'appel LLM de façon structurée.
- **Garantir la compliance** des flux (masquage de données personnelles, filtrage de mots-clés, blocage de transferts non maîtrisés).
- **Piloter les coûts et la disponibilité** (FinOps, fallback multi-fournisseurs).

### 1.3 Périmètre

- Le proxy est **agnostique du fournisseur** : chaque utilisateur/entreprise configure ses propres fournisseurs LLM et ses tokens d'API.
- Les coûts de consommation de tokens restent à la charge de l'utilisateur final ; ProxyLLM ne facture pas l'usage des modèles.
- Le produit cible les équipes techniques (DSI, plateforme, sécurité) qui doivent encadrer l'usage des LLM par leurs développeurs.

---

## 2. Fonctionnalités détaillées

### 2.1 Moteur d'observabilité — méthode « Unité »

Chaque appel LLM est décomposé et journalisé selon une grille de lecture structurée (1 unité d'analyse) :

- Acteur
- Contexte
- Ressource
- Logs
- Risques
- Relation
- Réalisation / Livrable
- Objectif
- Mission

Cette grille alimente le graphe de décision utilisé par le moteur de règles et les rapports d'audit.

### 2.2 Moteur de règles symboliques (Policy-as-Code)

- Règles déterministes définies par les administrateurs en **YAML/JSON**.
- Syntaxe conditionnelle du type : `SI <condition sur Acteur/Action/Ressource/Risque> ALORS <Bloquer | Alerter | Masquer | Autoriser>`.
- Exemple : *Si l'utilisateur est un dev Junior et que l'Action = "DROP TABLE", ALORS Bloquer et Alerter*.
- Les règles sont versionnées, rechargeables à chaud, et applicables par périmètre (équipe, projet, clé API virtuelle).

### 2.3 Détection & assainissement des données sensibles (PII & secrets)

- Détection en temps réel des données personnelles (noms, emails, IBAN, etc.) et des secrets (clés API, tokens, mots de passe) dans les requêtes sortantes.
- Deux modes d'action, configurables par règle :
  - **Blocage** pur de la requête.
  - **Masquage dynamique** (anonymisation/rédaction) : les tokens sensibles sont remplacés par des variables anonymes avant l'envoi au LLM, puis les vraies valeurs sont **réinjectées dans la réponse** au retour.
- Objectif : éviter tout transfert imprévu de données vers des fournisseurs hébergés aux États-Unis, sans bloquer systématiquement l'usage du LLM.

### 2.4 Auditabilité & conformité EU AI Act

- Génération automatique de **rapports de conformité** (PDF/JSON) en un clic (« 1-Click Compliance Report »).
- Les rapports couvrent les exigences clés de l'EU AI Act : explicabilité, traçabilité des décisions, maîtrise des risques.
- Basés sur les logs structurés produits par le moteur d'observabilité (§2.1) et l'historique des règles appliquées (§2.2).

### 2.5 FinOps & administration

- **Quotas et budgets** définis par équipe ou par utilisateur, associés à des clés API virtuelles.
- Suivi fin de la consommation de tokens (coût par requête, par acteur, par projet).
- **Alertes de surconsommation** et plafonds mensuels configurables.
- Vue de **chargeback** pour répartir les coûts réels entre équipes.

### 2.6 Fallback / Failover automatique

- Détection de panne ou de latence excessive d'un fournisseur LLM primaire.
- Bascule automatique et transparente vers un modèle équivalent chez un fournisseur secondaire (ex. OpenAI → Mistral sur OVHcloud).
- Règles de correspondance modèle primaire/secondaire configurables par l'administrateur.

---

## 3. Architecture & intégration

### 3.1 Principe : drop-in replacement

- Le proxy expose une API **compatible OpenAI API** (mêmes endpoints/format).
- Intégration côté client : changement de la variable `OPENAI_BASE_URL` uniquement (IDE — Cursor, VS Code —, SDK).
- Aucune modification du code métier des applications clientes.

### 3.2 Flux d'une requête

```
Client (IDE / SDK)
   │  requête (OPENAI_BASE_URL → ProxyLLM)
   ▼
ProxyLLM Glacis
   ├─► Moteur de règles symboliques (Policy-as-Code)     ─┐
   ├─► Détection & masquage PII/secrets                   ├─ chemin synchrone (< 5 ms ajoutés)
   ├─► Décision : Bloquer / Alerter / Masquer / Autoriser ─┘
   │
   ├─► [si autorisé] Transfert au fournisseur LLM configuré
   │        │
   │        ▼
   │   Réponse du fournisseur
   │        │
   │   Réinjection des valeurs réelles (si masquage appliqué)
   ▼
Réponse renvoyée au client

   (en parallèle, hors chemin critique)
   └─► Tâche asynchrone : construction du graphe de décision,
       écriture des logs d'audit, mise à jour FinOps/quotas
```

### 3.3 Traitement asynchrone

- Seules les étapes indispensables à la décision (règles + détection PII) sont **synchrones**, pour garantir une latence ajoutée quasi nulle (**< 5 ms**).
- L'analyse approfondie (graphe de décision, enrichissement des logs, agrégation FinOps, génération de rapports) est déportée en **tâche de fond**.

### 3.4 Compatibilité télémétrie

- Export des traces au format **OpenTelemetry**, pour intégration dans les stacks d'observabilité déjà en place chez le client (Grafana, Datadog, etc.).

---

## 4. Interface

> Cette section spécifie les interfaces nécessaires pour exploiter les fonctionnalités ci-dessus ; leur implémentation précise reste à valider avec le porteur du projet.

### 4.1 Interface d'administration (dashboard web)

- **Gestion des règles** : éditeur des règles Policy-as-Code (YAML/JSON), activation/désactivation, versionning.
- **Vue Observabilité** : exploration des séquences d'appels LLM selon la grille Unité (Acteur, Contexte, Ressource, Logs, Risques, Relation, Réalisation, Objectif, Mission), avec filtres et recherche.
- **Vue Compliance** : génération et téléchargement des rapports AI Act (1-Click Compliance Report).
- **Vue FinOps** : suivi des coûts par clé API virtuelle/équipe/utilisateur, configuration des quotas et alertes.
- **Gestion des fournisseurs** : configuration des fournisseurs LLM, tokens d'API, règles de fallback/failover.
- **Gestion des accès** : création des clés API virtuelles, rattachement à des acteurs/équipes.

### 4.2 Interface développeur

- **Aucune interface dédiée requise côté usage courant** : intégration transparente via `OPENAI_BASE_URL` (§3.1).
- Messages d'erreur/alerte explicites renvoyés dans la réponse API en cas de blocage par une règle, pour rester lisible dans l'IDE/SDK du développeur.

### 4.3 Interface d'exploitation (CLI / Make)

- Commandes de cycle de vie du service en local/dev :
  - `make dev-run` : démarrage de l'environnement de développement.
  - `make dev-kill` : arrêt de l'environnement.
  - `make dev-build` : build du proxy.

---

## 5. Technologies

| Brique | Choix | Rôle |
|---|---|---|
| Langage du proxy | **Rust** | Performance et sûreté mémoire pour un composant sur le chemin critique des requêtes (latence < 5 ms) |
| Conteneurisation | **Docker** | Packaging et déploiement du proxy et de ses dépendances |
| Orchestration dev | **Make** | Commandes standardisées de développement (`dev-run`, `dev-kill`, `dev-build`) |
| Télémétrie | **OpenTelemetry** | Export des traces/logs vers les outils d'observabilité existants |
| Format des règles | **YAML / JSON** | Définition des règles Policy-as-Code |
| API | **Compatible OpenAI API** | Interopérabilité drop-in avec les IDE/SDK existants |

---

## 6. Exigences non fonctionnelles

- **Latence** : surcoût de traitement synchrone inférieur à 5 ms par requête.
- **Sécurité** : les tokens des fournisseurs LLM et les données réidentifiées (masquage) doivent être stockés/chiffrés de façon à ne jamais transiter en clair vers un tiers non autorisé.
- **Traçabilité** : chaque décision (blocage, masquage, autorisation) doit être journalisée et rattachée à une règle identifiable, pour satisfaire les exigences d'auditabilité de l'EU AI Act.
- **Disponibilité** : le mécanisme de fallback/failover ne doit pas dégrader la disponibilité perçue par le développeur.
- **Portabilité** : déploiement via Docker sur l'infrastructure du client (on-premise ou cloud européen), sans dépendance imposée à un hébergeur.

---

## 7. Glossaire

- **Unité** : grille d'analyse d'un appel LLM (Acteur, Contexte, Ressource, Logs, Risques, Relation, Réalisation, Objectif, Mission).
- **Policy-as-Code** : définition des règles de gouvernance sous forme de fichiers de configuration versionnés (YAML/JSON) plutôt qu'en code applicatif.
- **PII** : Personally Identifiable Information — données à caractère personnel.
- **Chargeback** : répartition des coûts réels de consommation entre équipes/utilisateurs.
- **Drop-in replacement** : composant substituable à un autre sans modification du code appelant, via simple changement de configuration (ici, l'URL de base de l'API).
