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

- [x] Extraction des 9 axes (Acteur, Contexte, Ressource, Logs, Risques, Relation, Réalisation, Objectif, Mission) par requête
- [x] Écriture des logs structurés en tâche asynchrone
- [x] Export OpenTelemetry

**Fait le 2026-09-14.** Module `proxy/src/unite.rs`. Chaque appel `/v1/*` est décomposé en une `UniteRecord` (les 9 axes) puis journalisé en JSON sur stdout et exporté en OTLP/HTTP (JSON, sans SDK opentelemetry officiel — implémentation manuelle légère pour rester simple et sans dépendance protoc/tonic) si `OTEL_EXPORTER_OTLP_ENDPOINT` est défini. L'émission tourne dans une tâche `tokio::spawn` : aucune attente ajoutée sur la réponse renvoyée au client.

**Mise à jour du 2026-09-14 (soir) : champ `action`.** Ajout d'un champ `action` à l'`UniteRecord`, extrait du dernier message `"role": "user"` du corps JSON (`messages[]`), tronqué à 80 caractères — l'intention réelle du prompt (verbe + objet, esprit Fourmi §1.1), pas juste `méthode + chemin`. Fallback sur `méthode + chemin` si non exploitable. ⚠️ Peut contenir des données sensibles saisies par l'utilisateur ; décision explicite de l'accepter en attendant le masquage PII (Étape 4) plutôt que de garder un label purement technique — validé avec l'utilisateur. Répercuté dans l'export OTLP (`proxyllm.action`), la vue Observabilité (colonne Action) et la vue Fourmi 3D (label du nœud Action).

Sources des axes (MVP, avant les étapes 3/6) :
- **Acteur** : en-tête `X-ProxyLLM-Actor`, sinon IP du pair (`ConnectInfo`).
- **Contexte** : en-tête `X-ProxyLLM-Context`, sinon `User-Agent`.
- **Ressource** : `fournisseur/modèle` (modèle extrait du champ `model` du body JSON si présent).
- **Logs** : méthode, chemin, statut, latence, taille de la requête.
- **Risques** : `non_evalue` en l'absence de règle déclenchée — calculé par le moteur de règles depuis l'Étape 3 (voir plus bas).
- **Relation** : en-tête `X-ProxyLLM-Session`, sinon `isolee`.
- **Réalisation** : succès/erreur + taille de la réponse (ou `stream` si en streaming).
- **Objectif** / **Mission** : en-têtes `X-ProxyLLM-Objective` / `X-ProxyLLM-Mission`, sinon `non_precise`.

Validé avec un fournisseur mock + un vrai collecteur OpenTelemetry (`otel/opentelemetry-collector`, exporter debug) sur le réseau Docker : span reçu avec les 9 axes en attributs.

## Étape 3 — Moteur de règles symboliques (Policy-as-Code)

- [x] Format des règles YAML/JSON
- [x] Chargement/rechargement à chaud des règles
- [x] Évaluation des règles sur le chemin synchrone (< 5 ms)
- [x] Actions : Bloquer / Alerter / Masquer / Autoriser

**Fait le 2026-09-14 (soir).** Module `proxy/src/rules.rs`. Règles chargées depuis `config/rules.yaml` (monté en volume, comme `providers.yaml`), rechargées automatiquement par scrutation du fichier (mtime) toutes les 2 s — pas de redémarrage requis. Premier chargement invalide = échec fatal (cohérence avec `providers.yaml`) ; un rechargement à chaud invalide log une erreur et conserve l'ancien jeu de règles.

Format d'une règle :
```yaml
rules:
  - name: "..."
    when: { acteur_contains: "...", action_matches: "regex", ... }   # ET logique entre les conditions
    then: [autoriser|bloquer|alerter|masquer]                        # une ou plusieurs actions
    replacement: "[TEXTE]"                                            # pour "masquer" uniquement
```
Conditions dans `when` : `acteur(_contains)`, `contexte(_contains)`, `provider`, `mission(_contains)`, `objectif(_contains)`, `action_contains`, `action_matches` (regex, précompilée au chargement — jamais par requête).

Évaluation synchrone, sur le chemin critique, **avant** tout appel réseau au fournisseur — c'est ce qui garde le surcoût sous les 5 ms visés (comparaisons de chaînes + regex précompilées, aucune I/O). Sémantique "premier match décisif gagne" en parcourant les règles dans l'ordre : `autoriser`/`bloquer` arrêtent l'évaluation (donc une règle `autoriser` placée avant une règle `bloquer` plus générale crée une exception) ; `alerter`/`masquer` s'accumulent sans arrêter.

- `bloquer` → réponse `403` immédiate, rien n'est transféré au fournisseur.
- `masquer` → substitution regex appliquée à la fois sur le corps transféré au fournisseur **et** sur l'axe Action de l'unité d'observabilité (sinon on masquerait l'observabilité sans masquer ce qui part réellement chez le tiers).
- `alerter` → pas de blocage, mais visible dans l'axe Risques et les logs.
- `autoriser` → arrête l'évaluation, aucune règle suivante (ex. un `bloquer` plus générique) ne s'applique.

L'axe **Risques** de l'unité d'observabilité (Étape 2) reflète maintenant le résultat réel de l'évaluation (ex. `"bloque par la regle 'X'"`, `"alerte(s) : Y"`, `"1 masquage(s) applique(s)"`, ou `"aucune regle declenchee"`) — ça referme la boucle demandée par l'utilisateur ("les risques doivent découler de l'action").

`config/rules.yaml` livré avec 4 règles d'exemple (une par action), reprenant l'exemple du cahier des charges (bloquer un `DROP TABLE`), plus un masquage d'emails, une alerte sur tentative de contournement de prompt, et une autorisation explicite par mission qui montre la priorité d'ordre.

Endpoint d'introspection `GET /internal/rules` (nom + conditions + actions de chaque règle chargée) pour vérifier un rechargement sans SSH.

Validé bout en bout avec un fournisseur mock : les 4 actions déclenchées séparément (bloquer → 403 sans toucher le fournisseur ; masquer → email effectivement remplacé dans le corps réellement envoyé ; alerter → passe mais visible dans Risques ; autoriser → contourne une règle de blocage plus générale placée après). Rechargement à chaud testé dans les deux sens (ajout d'une règle canari → blocage immédiat sans redémarrage ; retrait → déblocage immédiat) via le volume monté, sans toucher au conteneur. Latence totale mesurée (réseau loopback + règles + mock) : 2 à 5 ms — l'évaluation des règles elle-même en est une fraction négligeable.

- [x] Vue Règles dans l'admin (`rules.html`)

**Fait le 2026-09-14 (nuit).** Carte "Règles (Policy-as-Code)" du dashboard, plus `POST /internal/rules/test` côté proxy (relayé par `POST /api/rules/test`) pour un testeur de décision sans coût : formulaire de faits (Acteur/Contexte/Fournisseur/Mission/Objectif/Action) → verdict (bloqué/autorisé/alertes/masquages), sans appeler le LLM. La liste des règles chargées (nom/conditions/actions) vient de `GET /internal/rules`, déjà en place. `AXIS_META` de `fourmi.html` et le prompt système de `POST /api/analyze` ont été corrigés au passage pour ne plus faire deviner l'axe Risque par le LLM, puisqu'il est désormais une vraie valeur déterministe issue du moteur de règles.

## Étape 4 — Détection & masquage PII/secrets

- [x] Détection des données personnelles et secrets dans les requêtes sortantes
- [x] Masquage dynamique (remplacement par variables anonymes)
- [x] Réinjection des vraies valeurs dans la réponse

**Fait le 2026-09-14 (nuit).** Module `proxy/src/pii.rs`. Contrairement au moteur de règles (Étape 3, motifs écrits à la main dans `rules.yaml`), les catégories ici sont **intégrées et actives par défaut** — un filet de sécurité générique, pas une politique à configurer.

Catégories détectées : `EMAIL`, `TELEPHONE`, `IBAN` (validé par checksum mod-97), `CARTE_BANCAIRE` (validée par checksum de Luhn — évite les faux positifs sur n'importe quelle longue suite de chiffres), `SECRET_API` (préfixes connus : `sk-`, `AKIA`, `ghp_`, `xox[baprs]-`). Regex précompilées une seule fois (`OnceLock`), pas par requête — même philosophie que le moteur de règles pour rester sous le budget de 5 ms.

Fonctionnement :
1. Chaque occurrence détectée est remplacée par une **variable anonyme numérotée** (`[EMAIL_1]`, `[SECRET_API_1]`, ...) — appliqué à l'axe Action (observabilité) et au corps réellement transféré au fournisseur, après le masquage éventuel de l'Étape 3.
2. La correspondance placeholder → vraie valeur vit uniquement en mémoire locale le temps de la requête (jamais journalisée, jamais persistée).
3. Si le fournisseur reprend un placeholder dans sa réponse, il est **réinjecté** (vraie valeur restituée) avant de renvoyer la réponse au client.
4. Conséquence technique assumée : une requête avec réinjection ne peut plus être streamée en zero-copy (il faut bufferiser toute la réponse pour faire la substitution) — perte du streaming SSE token-par-token uniquement sur les appels concernés. Les appels sans PII détectée gardent le streaming intact.

Toggle serveur uniquement : `PROXY_PII_MASKING=off` dans `.env` désactive la détection. **Volontairement non désactivable par le client** (pas d'en-tête `X-ProxyLLM-*` prévu pour ça) — sinon n'importe quel appelant pourrait contourner une protection de conformité d'un simple en-tête.

Distinction importante avec l'Étape 3 : le masquage de règle (`rules.yaml`) est **statique et non réversible** (remplacement par un texte fixe, ex. `[EMAIL_MASQUE]`, aucune réinjection) — utile pour une rédaction permanente voulue par une politique. Le masquage PII de l'Étape 4 est **dynamique et réversible** — la valeur réelle revient dans la réponse. Les deux peuvent coexister sur un même appel sans conflit (numérotation de placeholders indépendante).

**Visibilité dans l'interface (à l'endroit le plus approprié — pas de nouvelle page) :** badge **🔒 PII** dans la liste de `fourmi.html`, à côté de chaque appel où une catégorie a été détectée (survol = catégories concernées). L'axe **Risques** inclut aussi la mention (`"... ; PII detectee : SECRET_API"`), et l'axe **Réalisation** note le nombre de valeurs réinjectées. L'axe **Action** affiche directement le texte masqué (ex. `voici ma cle [SECRET_API_1]`) — preuve visuelle immédiate que la valeur brute n'a jamais transité telle quelle.

Validé bout en bout avec un fournisseur mock qui journalise sur son propre stdout (jamais touché par la réinjection du proxy, donc preuve indépendante) : le fournisseur a bien reçu les placeholders (`[EMAIL_MASQUE]`, `[SECRET_API_1]`, `[IBAN_1]`, `[CARTE_BANCAIRE_1]`), jamais les vraies valeurs. Réinjection confirmée pour les catégories Étape 4 (secret, IBAN, carte) ; l'email resté masqué dans la réponse finale confirme le comportement non-réversible attendu de l'Étape 3. Faux positif évité sur un numéro à 16 chiffres ne passant pas Luhn. Latence mesurée avec les 3 catégories déclenchées dans un même appel : 3-4 ms.

## Étape 5 — Auditabilité & conformité AI Act

- [x] Historisation des décisions (règle appliquée, acteur, résultat)
- [x] Génération de rapport (1-Click Compliance Report) en PDF/JSON

**Fait le 2026-09-14 (nuit).** Module `proxy/src/audit.rs`.

**Historisation persistante** : contrairement à l'historique en mémoire de l'Étape 2 (`unite::Store`, 200 entrées, perdu au redémarrage), chaque unité est désormais aussi ajoutée à un journal `data/audit.jsonl` (une ligne JSON par appel, append-only), monté en volume (`./data:/app/data`, ajouté à `docker-compose.yml`). Écriture faite dans la même tâche asynchrone que le reste (§Étape 2) : hors chemin critique. Testé : après `docker compose restart proxy`, le rapport de conformité garde tout l'historique alors que la vue Fourmi 3D (mémoire) repart à zéro — c'est la distinction voulue entre observabilité temps réel et audit durable.

**Génération de rapport** : `GET /internal/compliance-report` (query `format=json|pdf`, `since`/`until` en timestamp unix ms optionnels) agrège le journal — volumétrie totale, décisions bloquées/alertes/PII masqués avec le détail de chaque événement (acteur, action, règle/catégorie), répartition par fournisseur et par mission. Le PDF (crate `printpdf`, police standard Helvetica, pagination manuelle simple) reprend le même contenu que le JSON, mis en forme en texte — dates converties en UTC lisible via un petit algorithme maison (civil_from_days de Howard Hinnant) pour éviter une dépendance chrono/time supplémentaire.

Validé bout en bout : 5 requêtes de test (1 bloquée, 1 alerte, 1 avec PII/secret détecté, 2 neutres) → rapport JSON et PDF cohérents entre eux, PDF vérifié lisible (texte extrait avec `pypdf`), persistance confirmée après redémarrage du conteneur.

## Étape 6 — FinOps & administration

- [x] Clés API virtuelles
- [x] Suivi de consommation de tokens par acteur/équipe
- [x] Quotas, budgets, alertes de surconsommation
- [x] Vue chargeback

**Fait le 2026-09-14 (nuit).** Module `proxy/src/finops.rs`. Fichier `config/virtual_keys.yaml` (même schéma hot-reload ~2s que `rules.yaml`).

**Comportement par défaut inchangé.** Fichier vide (`keys: []`, livré tel quel) = proxy ouvert, exactement comme avant cette étape. Dès qu'au moins une clé est définie, le proxy exige `Authorization: Bearer <clé>` sur tout `/v1/*` — 401 sinon. Décision volontaire pour ne rien casser tant que l'admin n'active pas explicitement le contrôle d'accès.

**Identité** : la clé résolue remplace l'axe Acteur dérivé de l'IP/en-tête (plus fiable — authentifié, pas juste déclaré). Le bearer du client n'atteint jamais le vrai fournisseur (déjà exclu des en-têtes forwardés, remplacé par la vraie clé provider comme avant).

**Quota** : `quota_tokens` optionnel par clé, vérifié en mémoire (rapide, budget < 5 ms) sur la consommation connue *avant* l'appel — cohérence à terme, pas une garantie stricte anti-rafale sur des requêtes concurrentes (limite documentée, acceptable pour du suivi FinOps). Dépassé → `403`. Alerte de surconsommation dès 80 % du quota, remontée dans l'axe Risques (`"alerte surconsommation : X/Y tokens"`), sans bloquer.

**Suivi des tokens** : extrait du champ `usage` de la réponse fournisseur (format OpenAI). Compromis assumé : ça nécessite de lire toute la réponse, donc de renoncer au streaming zero-copy — uniquement pour les appels authentifiés par une clé virtuelle (comme la réinjection PII de l'Étape 4, dont c'est la même contrainte). Sans clé virtuelle configurée, aucun impact : le streaming reste intact.

**Persistance sans nouveau mécanisme** : les compteurs de consommation ne sont pas sauvegardés séparément — ils sont réhydratés au démarrage depuis le journal d'audit de l'Étape 5 (déjà persistant), en resommant les tokens par clé virtuelle. Testé : après redémarrage du conteneur, la consommation cumulée est retrouvée à l'identique.

**Chargeback** : `GET /internal/chargeback` — une ligne par clé configurée (nom, tokens consommés, requêtes, quota, % utilisé, coût estimé si `cost_per_1k_tokens` renseigné — un taux interne défini par l'admin, pas le vrai tarif du fournisseur). Clés brutes **jamais exposées**, toujours masquées (`****xxxx`) — y compris dans cet endpoint interne, puisque proxy et admin restent sans authentification propre.

Validé bout en bout avec un mock retournant un vrai champ `usage` : sans clé → 401 ; clé invalide → 401 ; bonne clé → 200 + acteur = nom de la clé ; quota de 30 tokens consommé sur 2 appels (15 chacun) → 3e appel bloqué (403, "30/30 tokens") ; chargeback exact (50 % puis 100 %, coût estimé correct) ; persistance confirmée après redémarrage.

## Étape 7 — Fallback / Failover

- [ ] Détection de panne/latence d'un fournisseur
- [ ] Bascule automatique vers un fournisseur/modèle équivalent
- [ ] Configuration des correspondances de modèles

## Étape 8 — Interface admin (dashboard)

- [x] Vue Règles (éditeur Policy-as-Code) — liste + testeur de décision, `rules.html` (cf. Étape 3)
- [x] Vue Observabilité (exploration des séquences par la grille Unité) — fusionnée dans la vue Fourmi 3D, voir plus bas
- [x] Vue Compliance (génération/téléchargement des rapports) — `compliance.html` (cf. Étape 5)
- [x] Vue FinOps (coûts, quotas, alertes) — `finops.html` (cf. Étape 6)
- [ ] Vue Fournisseurs (config LLM + fallback)
- [ ] Gestion des accès (clés API virtuelles)

**Vue Observabilité faite le 2026-09-14, supprimée le 2026-09-14 (nuit).** `admin/static/observability.html` (tableau des dernières unités) faisait doublon avec la liste latérale de la vue Fourmi 3D (mêmes données, via `GET /api/observability`). Retirée du dashboard et du disque à la demande de l'utilisateur ; seule la vue Fourmi 3D subsiste comme point d'entrée Observabilité.

- [x] Vue Fourmi 3D (WebGL) — chaque appel représenté comme un graphe selon la méthode Fourmi (`Docs/Fourmi.md`)

**Fait le 2026-09-14.** `admin/static/fourmi.html` (carte "Fourmi 3D" du dashboard — seul point d'entrée Observabilité de l'admin). Bibliothèque `3d-force-graph@1.80.0` (WebGL/Three.js, CDN jsdelivr). Liste des derniers appels à gauche (via `GET /api/observability`), clic → rendu 3D du graphe Fourmi de cet appel.

Mapping unité → Fourmi (sous-ensemble fidèle à Fourmi.md §8, boucle 1) :
- Action = méthode + chemin ; Acteur/Ressource/Contexte/Risque/Livrable/Objectif/Mission/Logs = axes correspondants de l'unité.
- Lien (bleu foncé) approximé par l'axe Relation de l'unité (`X-ProxyLLM-Session` ou `isolee`).
- Arêtes rendues : `réalise`, `permet` (Ressource et Lien → Action), `influence`, `menace`, `produit`, `contribue à`, `donne sens à`, `génère`, `documente`.

**Non couvert (nécessite une agrégation sur plusieurs appels, pas encore implémentée côté proxy)** — affiché explicitement dans un bandeau sur la page :
- **Interaction** (distincte du Lien) — pas de suivi de flux daté par acteur/session.
- **Égrégore** — nécessiterait une détection de motif récurrent sur plusieurs missions/contextes, avec une `norme_prescrite` qui reste d'appréciation humaine (cf. Fourmi.md §3.6, §6).
- **Acteur induit** — le moteur de règles (Étape 3) existe désormais, mais rien n'y détecte spécifiquement les effets de bord sur des tiers non participants ; resterait à écrire une règle/heuristique dédiée.

- [x] Analyse sémantique à la demande (bouton "Analyser avec le LLM")

**Fait le 2026-09-14 (soir).** Par défaut, Acteur/Contexte/Ressource/Risque/Objectif/Mission restent dérivés de métadonnées techniques (en-têtes, IP, config) — pas du sens du prompt. Sur demande explicite (bouton dans `fourmi.html`), `POST /api/analyze` (admin) fait relire l'action par le LLM lui-même (réutilise `/v1/chat/completions` du proxy, même fournisseur/modèle que l'appel d'origine) avec un prompt système dédié qui demande une décomposition Fourmi en JSON. Le résultat met à jour le graphe 3D et s'affiche dans un panneau dédié.

Décision (validée avec l'utilisateur, cf. §"Prochaine étape" précédente) : pas d'analyse automatique sur chaque appel — ça doublerait systématiquement le coût facturé. Uniquement à la demande, appel par appel. Testé de bout en bout avec un fournisseur mock imitant une vraie réponse `chat.completion` : extraction JSON correcte depuis `choices[0].message.content` (y compris nettoyage des balises markdown ```json``` si le LLM les ajoute), et gestion propre des erreurs (clé manquante, JSON non exploitable).

**Correction du 2026-09-14 (nuit) : clé API du client, pas de `.env`.** Initialement, l'analyse retombait sur la clé serveur (`api_key_env`) faute de mieux. Ajout d'un champ "Clé API" dans `fourmi.html` (identique à celui de `test.html`), envoyé via `X-ProxyLLM-Api-Key` et jamais lu depuis l'environnement du conteneur admin. Les deux pages partagent la valeur via `localStorage` du navigateur (jamais persistée côté serveur). Sans clé saisie, erreur explicite plutôt qu'un fallback silencieux sur `.env`. Vérifié bout en bout (comparaison de l'`Authorization` reçu par un mock, avec/sans clé cliente).

---

**Prochaine étape à prioriser : Étape 3 (moteur de règles) ou Étape 4 (masquage PII), selon ce que tu veux poser en premier — à discuter.**
