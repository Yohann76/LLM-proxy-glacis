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
- Aucun contrôle métier pour l'instant (pas de règles, pas de masquage PII, pas de logs d'audit) : c'est un simple relais transparent — voir `roadmap.md` pour la suite.

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
