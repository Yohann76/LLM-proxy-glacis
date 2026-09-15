# Utilisation — Adresses & ports

## Démarrage

```
make dev-run
```

(crée `.env` depuis `.env.example` si absent, build et lance les conteneurs)

## Services

| Service | Adresse | Port hôte | Port conteneur | Healthcheck |
|---|---|---|---|---|
| Proxy | http://162.19.241.44:45321 | 45321 | 8080 | http://162.19.241.44:45321/healthz |
| Admin | http://162.19.241.44:45322 | 45322 | 8081 | http://162.19.241.44:45322/healthz |


http://162.19.241.44:45322/

Ports définis dans `.env` (`PROXY_PORT`, `ADMIN_PORT`), modifiables si besoin. Services bindés sur toutes les interfaces (`0.0.0.0`) et donc accessibles via l'IP de la machine, pas seulement en local.

> ⚠️ L'IP `162.19.241.44` est celle de la machine au moment de la rédaction (`hostname -I`) — à vérifier si elle change (DHCP, autre réseau). `make dev-run` affiche l'IP à jour à chaque lancement.

## Passthrough proxy (`/v1/*`)

Le proxy relaie toute requête `/v1/...` vers un fournisseur LLM configuré dans `config/providers.yaml` (monté en volume, éditable sans rebuild — redémarrer le conteneur `proxy` pour appliquer un changement).

1. Renseigner la clé du fournisseur dans `.env` (ex. `OPENAI_API_KEY=...`), puis `make dev-run`.
2. Appeler le proxy comme l'API OpenAI, en changeant juste l'URL de base :

```
curl http://162.19.241.44:45321/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"salut"}]}'
```

- Fournisseur utilisé : `default_provider` du fichier de config, ou celui indiqué via l'en-tête `X-ProxyLLM-Provider: mistral`.
- La clé `Authorization` envoyée par le client est ignorée — le proxy injecte toujours la clé configurée côté serveur.
- Depuis l'Étape 3, chaque requête passe par le moteur de règles avant transfert (voir section dédiée ci-dessous) : elle peut être bloquée ou son contenu masqué. Pas encore de détection PII automatique générique (Étape 4) — seulement ce que les règles explicitement configurées couvrent.

## Moteur de règles symboliques (Policy-as-Code)

Fichier : **`config/rules.yaml`** (monté en volume, éditable sans rebuild — rechargé automatiquement en ~2 s, pas besoin de redémarrer le conteneur `proxy`).

```yaml
rules:
  - name: "Bloquer les instructions destructrices en base de données"
    when:
      action_matches: '(?i)\b(DROP\s+TABLE|DELETE\s+FROM|TRUNCATE)\b'
    then: [bloquer, alerter]
```

- Conditions dans `when` (toutes doivent matcher — ET logique) : `acteur`/`acteur_contains`, `contexte`/`contexte_contains`, `provider`, `mission`/`mission_contains`, `objectif`/`objectif_contains`, `action_contains`, `action_matches` (regex).
- Actions dans `then` : `autoriser`, `bloquer`, `alerter`, `masquer` (une ou plusieurs). Évaluées **dans l'ordre du fichier**, "premier match décisif gagne" : `autoriser`/`bloquer` arrêtent l'évaluation (place une règle `autoriser` **avant** une règle `bloquer` plus générale pour créer une exception) ; `alerter`/`masquer` s'accumulent sans arrêter.
- `bloquer` → `403` immédiat, rien n'est envoyé au fournisseur.
- `masquer` → réutilise `action_matches` comme motif, remplacé par `replacement` (défaut `[MASQUE]`) — appliqué **à la fois** au corps réellement transféré au fournisseur et à l'axe Action de l'observabilité.
- `alerter` → ne bloque pas, mais visible dans l'axe **Risques** (et les logs/OTLP/Fourmi 3D).
- 4 règles d'exemple livrées par défaut (une par action) — à adapter ou remplacer selon tes besoins.
- ⚠️ Évaluation **synchrone**, avant l'appel réseau au fournisseur (chemin critique, budget < 5 ms) — pas de logique lourde ou d'appel externe dans une règle.

## Détection & masquage PII/secrets (intégré)

Contrairement aux règles ci-dessus (motifs à écrire à la main), cette détection est **active par défaut**, sans configuration : catégories `EMAIL`, `TELEPHONE`, `IBAN` (checksum mod-97), `CARTE_BANCAIRE` (checksum de Luhn), `SECRET_API` (`sk-`, `AKIA`, `ghp_`, `xox...-`).

- Chaque occurrence → variable anonyme numérotée (`[EMAIL_1]`, `[SECRET_API_1]`...) envoyée au fournisseur à la place de la vraie valeur.
- Si le fournisseur reprend le placeholder dans sa réponse, la vraie valeur est **réinjectée** avant de renvoyer la réponse au client — la seule catégorie de masquage de ce projet qui soit réversible (celles de `rules.yaml`, Étape 3, sont permanentes, pas de réinjection).
- Conséquence : une réponse avec réinjection est bufferisée entièrement (pas de streaming SSE token-par-token) — seulement pour les appels où une PII a été détectée, le reste garde le streaming.
- Désactivable uniquement côté serveur : `PROXY_PII_MASKING=off` dans `.env`. Jamais par un en-tête client (sinon n'importe qui pourrait contourner la protection).
- **Visible dans l'interface** : badge **🔒 PII** dans la liste de `fourmi.html` (survol = catégories détectées) ; aussi dans l'axe Risques (`"PII detectee : SECRET_API"`) et l'axe Action (qui affiche directement le texte masqué).

### Vue Règles (dans l'admin)

Page dédiée : **http://162.19.241.44:45322/rules.html** (carte "Règles (Policy-as-Code)" du dashboard).

- **Liste des règles chargées** : nom, conditions (`when`), actions (`then`) — reflète `config/rules.yaml` tel qu'actuellement chargé côté proxy (bouton "Rafraîchir" après une modif, le rechargement à chaud prenant jusqu'à ~2 s).
- **Testeur de décision** : formulaire (Acteur, Contexte, Fournisseur, Mission, Objectif, Action) qui évalue les faits saisis contre le jeu de règles courant et affiche le verdict — **sans faire de vraie requête LLM**, donc sans coût ni appel fournisseur. Pratique pour vérifier qu'une règle fait ce qu'on croit avant de l'exposer à du vrai trafic.
- Backend : `GET /internal/rules` et `POST /internal/rules/test` côté proxy, relayés par l'admin (`GET /api/rules`, `POST /api/rules/test`) — même schéma que les autres vues.

## Auditabilité & rapport de conformité

Journal d'audit persistant : **`data/audit.jsonl`** (monté en volume, `./data:/app/data`) — une ligne JSON par appel, jamais écrasée. Contrairement à l'historique en mémoire de la vue Fourmi 3D (200 entrées, perdu au redémarrage), ce journal **survit aux redémarrages du conteneur** : c'est la source du rapport de conformité.

### Vue Compliance (dans l'admin)

Page dédiée : **http://162.19.241.44:45322/compliance.html** (carte "Compliance" du dashboard).

- **Cartes de synthèse** : requêtes totales, bloquées, alertes, PII/secrets masqués, erreurs.
- **Répartition** par fournisseur, par mission, par catégorie PII détectée.
- **Tables d'événements** : appels bloqués, appels avec PII détectée, alertes — acteur, action, règle/catégorie, horodatage.
- **Filtre de période** optionnel (Depuis/Jusqu'à) — sans filtre, tout le journal disponible.
- **Téléchargement JSON ou PDF** en un clic ("1-Click Compliance Report") — le PDF reprend le même contenu, mis en page en texte simple (pas de design élaboré, l'objectif est la traçabilité).
- Backend : `GET /internal/compliance-report?format=json|pdf&since=...&until=...` côté proxy, relayé tel quel par l'admin (`GET /api/compliance-report`) — le PDF n'est jamais rechargé en mémoire, juste transmis en bytes.

## Clés API virtuelles & FinOps

Fichier : **`config/virtual_keys.yaml`** (monté en volume, rechargé à chaud en ~2 s — même mécanisme que `rules.yaml`).

```yaml
keys:
  - key: "vk-marketing-team-a1b2c3"
    name: "Équipe Marketing"
    quota_tokens: 500000
    cost_per_1k_tokens: 0.002   # optionnel — TON tarif interne, pas le vrai prix du fournisseur
```

- **`keys: []` (défaut livré)** : le proxy reste **ouvert**, comme avant cette fonctionnalité — aucune authentification requise.
- **Dès qu'une clé est définie** : le proxy exige `Authorization: Bearer <clé>` sur tout appel `/v1/*` — `401` sans clé valide.
- La clé résolue devient l'axe **Acteur** de l'observabilité (remplace l'IP/en-tête — c'est une identité authentifiée, plus fiable).
- **Quota** (`quota_tokens`, optionnel) : dépassé → `403`. Alerte dès 80 % du quota (visible dans l'axe Risques), sans bloquer.
- **Suivi des tokens** : extrait de la réponse du fournisseur (`usage.total_tokens`). ⚠️ Nécessite de bufferiser la réponse (perte du streaming zero-copy) — uniquement pour les appels authentifiés par une clé virtuelle, comme la réinjection PII de l'Étape 4.
- **Persistant** : les compteurs sont réhydratés au démarrage depuis `data/audit.jsonl` (pas de mécanisme de stockage séparé) — survivent aux redémarrages du conteneur.
- Les clés brutes ne sont **jamais exposées** par l'admin ou les endpoints `/internal/*` — toujours masquées (`****xxxx`).

### Vue FinOps (dans l'admin)

Page dédiée : **http://162.19.241.44:45322/finops.html** (carte "FinOps" du dashboard).

- Bandeau d'état : proxy ouvert (aucune clé) ou fermé (authentification requise).
- Une carte par clé virtuelle : nom, clé masquée, tokens consommés, quota, barre de progression (bleu → orange proche du quota → rouge dépassé), nombre de requêtes, coût estimé.
- Backend : `GET /internal/chargeback` côté proxy, relayé par l'admin (`GET /api/chargeback`).

### Tester l'authentification depuis l'admin

`test.html` a un champ **"Clé virtuelle"** (distinct du champ "Clé API" existant) — envoyé en `Authorization: Bearer <valeur>` vers le proxy, pour tester `config/virtual_keys.yaml` sans terminal. Sauvegardé dans le `localStorage` du navigateur (séparément de la clé API), jamais dans `.env`.

## Observabilité (méthode Unité)

Chaque appel `/v1/*` est journalisé en JSON structuré (`docker logs proxyllm-proxy`), avec les 9 axes (Acteur, Contexte, Ressource, Logs, Risques, Relation, Réalisation, Objectif, Mission) **plus un champ `action`** : le dernier message `"role": "user"` du corps JSON (`messages[]`, format OpenAI chat), tronqué à 80 caractères — c'est l'intention réelle envoyée au LLM, pas juste la forme technique de l'appel HTTP. Fallback sur `méthode + chemin` si le corps n'est pas exploitable (GET, format non conversationnel).

> ⚠️ Le champ `action` peut contenir du texte saisi par l'utilisateur — mais depuis l'Étape 4, tout ce qui matche une catégorie PII/secret connue y est déjà masqué (voir section dédiée plus haut). Ce qui reste en clair est ce qu'aucune règle (Étape 3) ni détection PII (Étape 4) n'a reconnu comme sensible : la protection est réelle mais pas exhaustive. Visible dans les logs, l'export OTLP, et la vue Fourmi 3D de l'admin — exposée sans authentification (cf. avertissement plus haut).

Pour enrichir les axes, envoyer des en-têtes optionnels sur la requête :

| En-tête | Axe renseigné | Défaut si absent |
|---|---|---|
| `X-ProxyLLM-Actor` | Acteur | IP du client |
| `X-ProxyLLM-Context` | Contexte | `User-Agent` |
| `X-ProxyLLM-Session` | Relation | `isolee` |
| `X-ProxyLLM-Objective` | Objectif | `non_precise` |
| `X-ProxyLLM-Mission` | Mission | `non_precise` |

Export OpenTelemetry (OTLP/HTTP) vers un collecteur externe : définir `OTEL_EXPORTER_OTLP_ENDPOINT` dans `.env` (ex. `http://otel-collector:4318`), puis `make dev-run`. Laisser vide pour désactiver l'export (les logs stdout restent actifs dans tous les cas).

### Vue Fourmi 3D (dans l'admin) — seul point d'entrée Observabilité

Page dédiée : **http://162.19.241.44:45322/fourmi.html** (carte "Fourmi 3D" du dashboard).

> Il existait une vue tableau séparée (`observability.html`) : supprimée le 2026-09-14 car redondante avec la liste latérale ci-dessous (mêmes données, `GET /api/observability`).

- Chaque appel LLM est traduit en graphe 3D navigable (WebGL, `3d-force-graph` via CDN) selon la méthode Fourmi décrite dans `Docs/Fourmi.md`.
- Liste des derniers appels à gauche (heure, statut, action, mission) ; cliquer sur un appel affiche son graphe : Action (rouge, centre) reliée à Acteur, Ressource, Contexte, Risque, Livrable, Objectif, Mission, Logs et Lien, avec les couleurs et verbes de la charte officielle.
- Alimentée par l'historique en mémoire du proxy (200 dernières unités, `GET /internal/unites`), relayé par l'admin (`GET /api/observability`). Aucune donnée sensible dans les métadonnées de base (pas de corps de requête/réponse — voir le champ `action` séparément ci-dessus). **Non persisté** : redémarrer le conteneur `proxy` vide l'historique.
- **Étages Interaction et Égrégore non représentés** : ils demandent une agrégation sur plusieurs appels (fréquence, motifs récurrents, `norme_prescrite`) que le proxy ne calcule pas encore. **Acteur induit** non plus. Un bandeau sur la page le rappelle.
- Survoler un nœud affiche son contenu complet ; les libellés longs sont tronqués dans la liste.
- **Badge 🔒 PII** sur les appels où l'Étape 4 a détecté et masqué une donnée personnelle/secret (survol pour voir les catégories).

**Bouton "Analyser avec le LLM"** : par défaut, Acteur/Contexte/Ressource/Risque/Objectif/Mission viennent de métadonnées techniques (en-têtes, IP, config), pas du sens du prompt. Ce bouton fait relire l'action par le LLM lui-même (même fournisseur/modèle que l'appel d'origine, réutilise `/v1/chat/completions`) pour en déduire ces 6 axes naturellement, à partir du sens réel de l'action — pas seulement de sa forme technique. Résultat affiché dans un panneau en bas à droite et injecté dans le graphe 3D (survoler les nœuds pour voir les nouvelles valeurs).

**Clé API du champ dédié, jamais de `.env`** : un champ "Clé API" dans la barre d'outils de `fourmi.html` envoie la clé du client (pas celle du serveur) à chaque analyse, via `X-ProxyLLM-Api-Key` — exactement comme le champ équivalent de `test.html`. Les deux pages partagent la même clé via le `localStorage` du navigateur (jamais envoyée au serveur admin en dehors de l'appel, jamais écrite dans `.env`) : tape-la une fois dans `test.html`, elle réapparaît automatiquement dans `fourmi.html`. Sans clé saisie, l'analyse échoue avec une erreur explicite plutôt que de retomber silencieusement sur une clé de `.env`. Vérifié bout en bout avec un fournisseur mock (comparaison de l'en-tête `Authorization` reçu).

⚠️ **Déclenché uniquement à la demande, jamais automatiquement** : ça double le nombre d'appels facturés au fournisseur pour l'appel analysé (un appel d'analyse en plus de l'appel d'origine). C'est un choix explicite pour ne pas doubler systématiquement les coûts de tous les appels.

## Interface de test — Chat (dans l'admin)

Page dédiée : **http://162.19.241.44:45322/test.html** (aussi accessible depuis la carte "Test API" du dashboard).

Interface de chat (bulles utilisateur/assistant), plutôt qu'un formulaire JSON à remplir à la main :

- Barre de réglages : Fournisseur, Modèle, Clé API — partagée avec `fourmi.html` via le `localStorage` du navigateur.
- Chaque message envoyé accumule l'historique de la conversation (`messages[]`, format OpenAI chat) et l'envoie en entier à chaque tour, pour un vrai contexte multi-tours.
- Sous chaque réponse : statut HTTP, latence, modèle, et **le nombre de tokens** (`usage.total_tokens`, détaillé prompt+completion si fourni par le fournisseur) — rien n'est perdu par rapport à l'ancien formulaire.
- Lien "voir la réponse brute" sous chaque réponse pour retrouver le JSON complet si besoin de débogage.
- "Nouvelle conversation" réinitialise l'historique.
- Toujours `POST /api/test` côté admin en coulisse (relais serveur vers le proxy, pas de CORS) — aucun changement côté backend.
- ⚠️ Page servie en HTTP simple (pas de TLS) sur une IP publique : la clé transite en clair sur le réseau. À réserver à des clés de test / jetables tant qu'il n'y a pas de HTTPS devant l'admin.

## Commandes Make

| Commande | Effet |
|---|---|
| `make dev-build` | Build des images Docker |
| `make dev-run` | Build + lancement des conteneurs en arrière-plan |
| `make dev-kill` | Arrêt et suppression des conteneurs |
| `make dev-shell` | Ouvre un shell dans le conteneur `proxy` (`SERVICE=admin` pour l'admin) |
