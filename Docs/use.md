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

## Commandes Make

| Commande | Effet |
|---|---|
| `make dev-build` | Build des images Docker |
| `make dev-run` | Build + lancement des conteneurs en arrière-plan |
| `make dev-kill` | Arrêt et suppression des conteneurs |
| `make dev-shell` | Ouvre un shell dans le conteneur `proxy` (`SERVICE=admin` pour l'admin) |
