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
- Aucun contrôle métier pour l'instant (pas de règles, pas de masquage PII) : c'est un simple relais transparent — voir `roadmap.md` pour la suite.

## Observabilité (méthode Unité)

Chaque appel `/v1/*` est journalisé en JSON structuré (`docker logs proxyllm-proxy`), avec les 9 axes (Acteur, Contexte, Ressource, Logs, Risques, Relation, Réalisation, Objectif, Mission) **plus un champ `action`** : le dernier message `"role": "user"` du corps JSON (`messages[]`, format OpenAI chat), tronqué à 80 caractères — c'est l'intention réelle envoyée au LLM, pas juste la forme technique de l'appel HTTP. Fallback sur `méthode + chemin` si le corps n'est pas exploitable (GET, format non conversationnel).

> ⚠️ Le champ `action` peut donc contenir du texte saisi par l'utilisateur, potentiellement sensible. Aucun masquage PII n'existe encore (Étape 4) : c'est un compromis assumé, visible dans les logs, l'export OTLP, et la vue Fourmi 3D de l'admin — exposée sans authentification (cf. avertissement plus haut).

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
- **Étages Interaction et Égrégore non représentés** : ils demandent une agrégation sur plusieurs appels (fréquence, motifs récurrents, `norme_prescrite`) que le proxy ne calcule pas encore. **Acteur induit** non plus (nécessite le moteur de règles, Étape 3). Un bandeau sur la page le rappelle.
- Survoler un nœud affiche son contenu complet ; les libellés longs sont tronqués dans la liste.

**Bouton "Analyser avec le LLM"** : par défaut, Acteur/Contexte/Ressource/Risque/Objectif/Mission viennent de métadonnées techniques (en-têtes, IP, config), pas du sens du prompt. Ce bouton fait relire l'action par le LLM lui-même (même fournisseur/modèle que l'appel d'origine, réutilise `/v1/chat/completions`) pour en déduire ces 6 axes naturellement, à partir du sens réel de l'action — pas seulement de sa forme technique. Résultat affiché dans un panneau en bas à droite et injecté dans le graphe 3D (survoler les nœuds pour voir les nouvelles valeurs).

**Clé API du champ dédié, jamais de `.env`** : un champ "Clé API" dans la barre d'outils de `fourmi.html` envoie la clé du client (pas celle du serveur) à chaque analyse, via `X-ProxyLLM-Api-Key` — exactement comme le champ équivalent de `test.html`. Les deux pages partagent la même clé via le `localStorage` du navigateur (jamais envoyée au serveur admin en dehors de l'appel, jamais écrite dans `.env`) : tape-la une fois dans `test.html`, elle réapparaît automatiquement dans `fourmi.html`. Sans clé saisie, l'analyse échoue avec une erreur explicite plutôt que de retomber silencieusement sur une clé de `.env`. Vérifié bout en bout avec un fournisseur mock (comparaison de l'en-tête `Authorization` reçu).

⚠️ **Déclenché uniquement à la demande, jamais automatiquement** : ça double le nombre d'appels facturés au fournisseur pour l'appel analysé (un appel d'analyse en plus de l'appel d'origine). C'est un choix explicite pour ne pas doubler systématiquement les coûts de tous les appels.

## Interface de test (dans l'admin)

Page dédiée : **http://162.19.241.44:45322/test.html** (aussi accessible depuis la carte "Test API" du dashboard).

- Formulaire : fournisseur, méthode, chemin, corps JSON, **clé API optionnelle**.
- Le bouton "Envoyer via le proxy" appelle `POST /api/test` sur l'admin, qui relaie côté serveur vers le proxy (pas de CORS à gérer) et affiche le statut HTTP, la latence et le corps de la réponse.
- Clé API : si le champ est rempli, elle surcharge (via l'en-tête `X-ProxyLLM-Api-Key`) la clé de `.env` pour cet appel uniquement — pratique pour tester sans éditer `.env` ni redémarrer les conteneurs. Si le champ est vide, le proxy retombe sur la clé configurée côté serveur (`api_key_env`).
- ⚠️ La page est servie en HTTP simple (pas de TLS) sur une IP publique : une clé saisie dans ce champ transite en clair sur le réseau. À réserver à des clés de test / jetables tant qu'il n'y a pas de HTTPS devant l'admin.

## Commandes Make

| Commande | Effet |
|---|---|
| `make dev-build` | Build des images Docker |
| `make dev-run` | Build + lancement des conteneurs en arrière-plan |
| `make dev-kill` | Arrêt et suppression des conteneurs |
| `make dev-shell` | Ouvre un shell dans le conteneur `proxy` (`SERVICE=admin` pour l'admin) |
