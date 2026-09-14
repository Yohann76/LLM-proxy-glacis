# Format Fourmi — Spécification canonique v0.2

## Document de référence unique

*Ce document remplace et consolide la v0.1. Il se suffit à lui-même : aucune lecture préalable n'est nécessaire.*

> **Objectif dans ProxyLLM Glacis :** Visualisation WebGL 3D de l'observabilité des séquences LLM (méthode Unité, cf. `Docs/cahier-des-charges.md` §2.1 et `Docs/roadmap.md` Étape 2), d'après un prompt donné. Ce document est la spécification du format de graphe ("Fourmi") sur laquelle s'appuiera cette visualisation.

---

## 0. Ce que fait le format

Le format Fourmi représente **une action élémentaire comme un système complet**.

Ce n'est pas un diagramme de tâche. Une tâche répond à « qu'est-ce qui est fait ? ». Une Fourmi répond en plus à : par qui, avec quoi, dans quel cadre, sous quelle menace, pour produire quoi, au service de quoi, en laissant quelle trace, en nourrissant quel lien — et sous la prescription de quelle entité collective que personne n'a décidée.

L'unité minimale est toujours :

> **Action centrale + composants périphériques**

L'action est au centre parce qu'elle est le point de passage entre l'intention et le résultat.

**Ce que le format apporte par rapport à un diagramme classique :** il rend visibles trois objets que les représentations habituelles perdent — le capital relationnel consommé et produit par l'action, les acteurs qui subissent l'action sans y participer, et la norme collective qui contraint l'action sans figurer dans aucune décision.

---

## 1. Les treize composants

Dix composants de base, trois niveaux relationnels, une variante d'acteur.

### 1.1 Action — rouge `#f44e3b`

Le centre du schéma. **La formulation commence toujours par un verbe.**

- Bon : « Produire 1 post FB », « Construire la matrice RACI », « Migrer le projet pilote »
- Mauvais : « Post Facebook », « RACI », « Migration »

Une Fourmi = une action. Un processus se représente par une série de Fourmis, jamais par une Fourmi géante.

### 1.2 Acteur — noir `#000000`

Celui qui réalise ou contribue à l'action. Plusieurs acteurs sont possibles.

Arêtes : `Acteur → réalise → Action`, `Acteur → utilise → Ressource`

Exemples : chef de projet, formateur, community manager, équipe plateforme, assistant IA, prestataire.

### 1.3 Acteur induit — noir `#000000`, labels `["Acteur", "Induit"]`

**Variante introduite en v0.2.** Celui qui subit l'action sans en être partie prenante, et qui produit des effets par retrait ou résistance passive.

Arêtes : `Action → affecte → Acteur induit`, puis `Acteur induit → érode → Lien` ou `→ résiste à → Action`

Exemples : l'abonné sur-sollicité qui se désabonne, l'expert rétrogradé en *Informé* par un RACI, l'auteur d'un pipeline réécrit sans lui, le riverain d'un chantier, l'utilisateur d'un outil qu'il n'a pas choisi.

**Règle de détection :** dans presque toute action de structuration (RACI, procédure, migration, réorganisation), quelqu'un perd une influence informelle sans être consulté. Cette personne est un acteur induit. Elle ne proteste jamais.

### 1.4 Ressource — gris `#999999`

Ce qui rend l'action possible : outil, budget, planning, template, donnée, prompt, support.

Arêtes : `Ressource → permet → Action`

### 1.5 Contexte — bleu cyan `#73d8ff`

Ce qui explique **pourquoi l'action a lieu maintenant**. Ce n'est pas un décor : c'est la situation qui rend l'action pertinente.

Arête : `Contexte → influence → Action`

Exemples : « Arbitrages qui remontent trop tard », « Coûts et dépendance fournisseur », « Bassin de vie déplacé de 250 km ».

### 1.6 Risque — orange `#fb9e00`

Ce qui menace l'action ou son résultat. Un bon risque est concret et rattaché à un objet précis.

**Deux cibles distinctes, à ne pas confondre :**

| Cible | Arête | Effet |
|---|---|---|
| L'action | `Risque → menace → Action` | Le livrable est dégradé ou absent |
| Le lien | `Risque → érode → Lien` | Le livrable sort parfaitement, le capital relationnel baisse |

Le second type est presque toujours absent des registres de risques classiques. C'est celui qui coûte le plus cher, parce que personne ne le surveille.

### 1.7 Réalisation / Livrable — vert `#68bc00`

Ce que l'action produit, sous forme vérifiable.

Arêtes : `Action → produit → Livrable`, `Livrable → contribue à → Objectif`

### 1.8 Objectif — jaune `#fcdc00`

L'effet attendu. Distinct du livrable : le livrable est ce qu'on remet, l'objectif est ce qui doit changer.

Arêtes : `Livrable → contribue à → Objectif`, `Objectif → vise → Transformation`

### 1.9 Mission / Transformation — violet `#7b64ff`

Le niveau de sens, ou le changement d'état recherché.

Arête : `Mission → donne sens à → Objectif`

### 1.10 Logs / Mémoire — brun `#8B5A2B`

Les traces laissées par l'action : lien, version, date, compte rendu, décision, **métriques**.

Arêtes : `Action → génère → Logs`, `Logs → documente → Action`, `Logs → mesure → Objectif`, `Logs → révèle → Égrégore`

**Règle absolue v0.2 : une métrique est un Log, jamais une relation.** En v0.1, les indicateurs d'engagement étaient colorés en bleu relationnel, ce qui créait trois sources de vérité concurrentes pour la mesure de l'objectif. C'est corrigé.

### 1.11 Interaction — bleu moyen `#2e75d4`

**Premier étage relationnel.** Le flux : un échange daté, un contact effectif.

Arêtes : `Action → engage → Interaction`, `Interaction → nourrit → Lien`

Exemples : une publication adressée à une audience, un atelier de négociation des rôles, une revue conjointe, un premier contact.

### 1.12 Lien — bleu foncé `#0b3d91`

**Deuxième étage relationnel.** Le stock : ce qui reste quand plus rien ne se passe.

Arêtes : `Interaction → nourrit → Lien`, `Lien → permet → Action`, `Lien → constitue → Égrégore`

Exemples : confiance d'une audience, légitimité de qui tranche, entraide de proximité, crédibilité d'une équipe.

### 1.13 Égrégore — indigo `#3b1f7a`, bordure pointillée

**Troisième étage relationnel.** L'entité collective émergente. Voir §3.

---

## 2. La composante Relations en trois étages

C'est l'apport central de la v0.2. En v0.1, « Relations » était un nœud unique et terminal : l'action l'influençait, et rien n'en repartait. Trois défauts en découlaient — le nœud était occupé par les interlocuteurs et les métriques plutôt que par la relation, le capital relationnel préexistant n'était jamais déclaré, et aucune boucle ne se refermait.

| Étage | Nature | Question | Temporalité | Se détruit par |
|---|---|---|---|---|
| **Interaction** | Flux, événement daté | Que s'est-il passé entre eux ? | Ponctuelle | Rien — l'événement est passé |
| **Lien** | Stock, capital accumulé | Que reste-t-il quand rien ne se passe ? | Cumulative avec dépréciation | Non-usage, trahison, rupture |
| **Égrégore** | Entité émergente autonome | Qu'est-ce qui s'est mis à exister par-dessus eux ? | Longue, avec inertie propre | Perte de croyance collective uniquement |

**Règle de production :** l'Interaction nourrit le Lien ; l'agrégat de Liens constitue l'Égrégore ; l'Égrégore prescrit aux Acteurs.

**Trois erreurs de niveau fréquentes :**

- Colorier un interlocuteur en bleu. « L'audience » est un Acteur, pas une relation.
- Colorier une métrique en bleu. « Likes et partages » sont des Logs.
- Modéliser un Lien sans l'Interaction qui le nourrit. Un stock sans flux est un lien fantôme.

---

## 3. L'égrégore

### 3.1 Définition opératoire

> **Un égrégore est une entité collective émergente, constituée par l'agrégation durable de liens, qui acquiert une capacité prescriptive autonome sur les acteurs qui l'ont produite.**

### 3.2 Trois critères cumulatifs

1. **Agrégation** — il résulte de plusieurs liens, jamais d'un seul
2. **Persistance** — il survit au départ de n'importe lequel de ses membres
3. **Prescription** — il impose une norme que personne n'a décidée et que chacun applique

### 3.3 Le test discriminant

> **Si l'entité disparaît quand on retire son animateur, ce n'est pas un égrégore — c'est un lien à dépendance porteur maximale.**

**Valides :** une communauté de marque, une culture d'équipe, une réputation professionnelle, une audience qui attend son rendez-vous hebdomadaire, un standard de fait, l'esprit d'une promotion de formation, le réflexe d'un outil qu'on a quitté.

**Invalides :** un groupe WhatsApp (ressource), un comité de pilotage (acteur collectif décisionnel), une liste de diffusion (ressource), une audience ciblée (acteur).

### 3.4 Les six états

| État | Signe observable | Enjeu de pilotage |
|---|---|---|
| **Naissant** | Des attentes se forment sans être formulées | Décider de le nourrir ou non |
| **Consolidé** | La norme est citée spontanément par les membres | Entretien régulier requis |
| **Autonome** | Il prescrit plus qu'il ne reçoit | Il faut négocier avec lui |
| **Dérivant** | Sa norme diverge de la situation réelle | Recadrage ou rupture |
| **Dormant** | Stock intact, flux nul | Réactivable à coût faible |
| **Dissous** | Plus personne n'y croit | Deuil, pas de relance |

### 3.5 Le paradoxe égrégorique

> **L'égrégore finit par commander les actions qui l'ont créé.**

Le community manager qui a bâti une communauté publie désormais pour satisfaire une attente qu'il a lui-même fabriquée. L'organisation qui a standardisé ses pipelines juge sa nouvelle forge à l'aune d'une norme qu'elle a construite sans la décider.

La question de pilotage n'est donc pas « comment nourrir l'égrégore » mais **« à quelles conditions accepte-t-on d'être prescrit par lui »**.

Une Fourmi comportant un égrégore en état *autonome* ou *dérivant* doit expliciter cette contrainte, sous peine de représenter comme libre une action qui ne l'est plus.

### 3.6 Règle d'agency

> **Si `autonomie ≥ 7`, l'égrégore est doublement labellisé `["Égrégore", "Acteur"]` et peut porter une arête `réalise → Action`.**

À ce seuil, l'entité collective agit : elle produit des actions que personne n'a décidées individuellement. Ne pas la modéliser comme acteur revient à attribuer à des individus des comportements qui leur sont prescrits.

---

## 4. Charte couleur officielle

La charte doit rester stable : c'est ce qui rend le format lisible d'un coup d'œil.

| Composant | Couleur | Code | Sens |
|---|---|---|---|
| Acteurs | Noir | `#000000` | Responsabilité, décision, action humaine |
| Ressources | Gris | `#999999` | Moyens, outils, supports |
| Actions | Rouge | `#f44e3b` | Mouvement, exécution, transformation concrète |
| Risques | Orange | `#fb9e00` | Menace, alerte, vigilance |
| Réalisations / Livrables | Vert | `#68bc00` | Production, résultat, valeur livrée |
| Objectifs | Jaune | `#fcdc00` | Cible, intention, effet recherché |
| Mission / Transformation | Violet | `#7b64ff` | Sens, transition, changement d'état |
| **Interaction** | **Bleu moyen** | **`#2e75d4`** | **Échange daté, contact, sollicitation** |
| **Lien** | **Bleu foncé** | **`#0b3d91`** | **Capital relationnel accumulé** |
| **Égrégore** | **Indigo** | **`#3b1f7a`** | **Entité collective émergente, prescriptive** |
| Contexte / Environnement | Bleu cyan | `#73d8ff` | Situation, cadre, milieu |
| Logs / Mémoire | Brun sépia | `#8B5A2B` | Trace, preuve, historique, capitalisation |

**Convention visuelle de l'égrégore :** bordure blanche épaisse ou pointillée, rayon supérieur aux autres nœuds. L'égrégore n'a pas le même statut ontologique que les autres composants — il n'est pas un objet du système, il est **un effet du système devenu cause**.

---

## 5. Liste contrôlée des relations

Limiter les verbes est ce qui empêche le format de dériver vers le schéma libre. **Aucun verbe hors de cette liste.**

| Verbe | De | Vers | Usage |
|---|---|---|---|
| `réalise` | Acteur | Action | L'acteur fait ou contribue à l'action |
| `utilise` | Acteur | Ressource | L'acteur mobilise un outil ou support |
| `permet` | Ressource | Action | La ressource rend l'action possible |
| `permet` | **Lien** | **Action** | **Le capital relationnel rend l'action possible** |
| `influence` | Contexte | Action | Le contexte oriente ou conditionne l'action |
| `menace` | Risque | Action | Le risque peut dégrader l'action |
| `érode` | Risque / Acteur induit | **Lien** | **Le stock se déprécie ou se casse** |
| `produit` | Action | Livrable | L'action génère un résultat concret |
| `contribue à` | Livrable | Objectif | Le livrable aide à atteindre l'objectif |
| `donne sens à` | Mission | Objectif | La mission justifie l'objectif |
| `vise` | Objectif | Transformation | L'objectif cherche un changement |
| `génère` | Action | Logs | L'action laisse une trace |
| `documente` | **Logs** | **Action** | **Boucle de capitalisation** |
| `mesure` | Logs | Objectif | Les traces évaluent l'effet |
| `engage` | **Action** | **Interaction** | **L'action ouvre un contact effectif** |
| `nourrit` | **Interaction** | **Lien** | **Le flux alimente le stock** |
| `constitue` | **Lien** | **Égrégore** | **Les liens agrégés forment l'entité** |
| `prescrit` | **Égrégore** | **Acteur** | **L'entité impose une norme non décidée** |
| `révèle` | **Logs** | **Égrégore** | **L'égrégore ne s'observe que par ses traces** |
| `affecte` | **Action** | **Acteur induit** | **L'action touche un non-partie-prenante** |
| `résiste à` | **Acteur induit** | **Action** | **Effet produit par résistance passive** |

*Les lignes en gras sont les ajouts v0.2.*

**Verbe retiré :** `mesure` ne part plus jamais des Relations. Uniquement des Logs.

---

## 6. Propriétés typées

Le champ `properties`, vide en v0.1, porte les grandeurs relationnelles. C'est ce qui transforme le schéma en instrument de pilotage.

### Sur un nœud `Lien`

```
stock                : 0-10
etat                 : actif | dormant | dette | rompu
entretien            : charge récurrente nécessaire au maintien
dependance_porteur   : 0-10 — part du lien attachée à une personne
portabilite          : ce qui subsiste si le contexte change de lieu ou de structure
reversibilite        : faible | moyenne | forte
```

`dependance_porteur` est la variable décisive : elle mesure ce qui disparaît le jour du départ d'une personne. C'est la seule métrique du format qui chiffre un risque humain jamais inscrit au registre.

### Sur un nœud `Égrégore`

```
intensite         : 0-10
autonomie         : 0-10   (≥ 7 → double label Acteur, voir §3.6)
etat              : naissant | consolide | autonome | derivant | dormant | dissous
norme_prescrite   : la règle non décidée que chacun applique
portee            : dyadique | triadique | reticulaire | champ
```

**Un égrégore sans `norme_prescrite` renseignée est un décor, pas un égrégore.** Si la norme ne peut pas être formulée en une phrase, l'objet ne mérite pas le nœud.

### Sur un nœud `Interaction`

```
frequence      : ponctuelle | cyclique | continue
initiative     : sortante | entrante | reciproque
consentement   : consenti | subi
```

`frequence` est le levier le plus fréquent : une interaction ponctuelle ne compense jamais l'érosion continue d'un stock.

---

## 7. Les trois boucles canoniques

Toute Fourmi complète comporte au moins une boucle fermée. Un graphe sans retour n'est pas un système.

### Boucle 1 — Capitalisation (courte, rapide)

```
Action → génère → Logs → documente → Action
```

Ce que l'action apprend revient à l'action suivante.

### Boucle 2 — Capital relationnel (moyenne)

```
Action → engage → Interaction → nourrit → Lien → permet → Action
```

Le lien est simultanément **produit** par l'action et **condition** de l'action suivante. Une Fourmi isolée en montre un arc ; une série en montre un capital qui croît ou s'épuise.

### Boucle 3 — Égrégorique (longue, lente, prescriptive)

```
Lien → constitue → Égrégore → prescrit → Acteur → réalise → Action → engage → Interaction → nourrit → Lien
```

Invisible à l'échelle d'une action, décisive à l'échelle d'une année.

---

## 8. Structure logique canonique

```
Acteur + Ressource + Contexte + Risque + Lien
              ↓
           Action
              ↓
        Livrable → Objectif → Mission / Transformation
```

Avec les trois boucles :

```
1.  Action → Logs → Action
2.  Action → Interaction → Lien → Action
3.  Lien → Égrégore → Acteur → Action → Interaction → Lien
```

**Question centrale du format :**

> Qui fait quoi, avec quoi, dans quel contexte, sous quels risques, avec quel capital relationnel préexistant, pour produire quoi, au service de quel objectif, en laissant quelles traces, en nourrissant quel lien — et sous la prescription de quelle entité collective que personne n'a décidée ?

---

## 9. Méthode d'extraction : du texte à la Fourmi

### 9.1 Table de correspondance

Énoncé source : *« Je dois publier un post Facebook pour annoncer une nouvelle formation et obtenir des inscriptions. »*

| Élément détecté | Composant |
|---|---|
| publier un post Facebook | **Action** |
| chargé de communication | **Acteur** |
| Facebook | **Ressource** |
| annoncer une nouvelle formation | **Contexte** |
| post publié | **Livrable** |
| obtenir des inscriptions | **Objectif** |
| communiquer | **Mission** |
| mauvais ciblage, mauvais horaire | **Risques** (sur l'action) |
| lien du post, métriques | **Logs** — seule source de mesure |
| l'audience | **Acteur** (c'est un interlocuteur) |
| le fait de s'adresser à elle | **Interaction** |
| la confiance accumulée | **Lien** |
| ce que l'audience attend désormais | **Égrégore** |

### 9.2 Les sept questions d'extraction

Les quatre premières relèvent du format de base, les trois dernières sont propres à la v0.2 et sont celles qu'on oublie systématiquement.

1. Quel verbe décrit exactement ce qui est fait ?
2. Qui le fait, avec quoi, et pourquoi maintenant ?
3. Qu'est-ce qui sort, et à quoi ça sert ?
4. Qu'est-ce qui menace le résultat ?
5. **Quel capital relationnel cette action consomme-t-elle avant d'en produire ?**
6. **Que reste-t-il de cette relation si l'action ne se répète pas pendant six mois ?**
7. **Quelle norme collective, que personne n'a décidée, contraint déjà cette action ?**

### 9.3 Ordre de construction recommandé

1. L'action, formulée avec un verbe
2. La chaîne verticale : Livrable → Objectif → Mission
3. Les entrants : Acteurs, Ressource, Contexte
4. Les risques sur l'action
5. Les Logs et la boucle de capitalisation
6. **Les trois étages relationnels, de l'Interaction vers l'Égrégore**
7. **L'acteur induit et le risque qui érode le stock** — presque toujours découverts en dernier, et souvent les plus intéressants

---

## 10. Règles de qualité

### Les seize règles

1. L'action est concrète et formulée avec un verbe.
2. L'acteur est identifiable et nommé.
3. La ressource est réellement mobilisable.
4. Le contexte explique pourquoi l'action a lieu maintenant.
5. Les risques sont rattachés à un objet précis.
6. Le livrable est vérifiable.
7. L'objectif exprime un effet, pas un objet.
8. La mission donne du sens.
9. Les logs permettent de retrouver ou prouver l'action.
10. Les relations montrent les interactions produites ou mobilisées.
11. **Le nœud « Relations » n'existe plus seul** : toute relation est Interaction, Lien ou Égrégore.
12. **Aucune métrique n'est une relation** : les indicateurs sont des Logs.
13. **Toute Fourmi complète contient au moins une boucle fermée.**
14. **Le stock relationnel consommé est déclaré** : si une arête `Lien → permet → Action` existe, elle est explicite.
15. **L'égrégore n'est jamais un livrable** : il préexiste ou se constitue à travers plusieurs Fourmis.
16. **Un égrégore modélisé porte toujours sa `norme_prescrite`.**

### Anti-patterns

| Anti-pattern | Symptôme | Correction |
|---|---|---|
| L'égrégore décoratif | Nœud indigo sans `norme_prescrite` | Le nommer ou le supprimer |
| L'égrégore fabriqué | Créé par une seule action | C'est un Lien, pas un égrégore |
| La métrique relationnelle | Nœud bleu contenant des chiffres | Le passer en Logs |
| Le lien fantôme | Lien sans Interaction qui le nourrit | Ajouter le flux ou retirer le stock |
| L'audience-relation | Un interlocuteur colorié en bleu | C'est un Acteur |
| Le graphe en arbre | Aucune arête retour | Implémenter au minimum la boucle 1 |
| Le risque unique | Tous les risques pointent vers l'Action | En chercher au moins un qui érode le Lien |
| La Fourmi obèse | Plus d'une action au centre | Scinder en plusieurs Fourmis |

---

## 11. Exemple commenté

**Action :** « Migrer le projet pilote de bout en bout » (contexte : migration GitLab → Forgejo)

| Composant | Contenu |
|---|---|
| Action | Migrer le projet pilote de bout en bout |
| Acteurs | Équipe plateforme Forgejo · Développeurs du projet pilote |
| Ressource | Runners Forgejo + cartographie des dépendances |
| Contexte | Coûts et dépendance fournisseur |
| Risques sur l'action | Dépendance non cartographiée · Secrets ou droits mal recréés |
| Livrable | Pilote validé de bout en bout |
| Objectif | Cycle complet sans dépendre de GitLab |
| Mission | Reprendre la maîtrise de la forge |
| Logs | Procédure, écarts et preuves |
| **Interaction** | Revue conjointe dev × plateforme — `frequence: cyclique` |
| **Lien** | Confiance des équipes dans Forgejo — `stock: 3`, `dependance_porteur: 8`, `reversibilite: faible` |
| **Acteur induit** | Auteurs des pipelines historiques — `effet: contournements maintenus hors forge` |
| **Risque relationnel** | Un pipeline rouge qui circule — `cible: le stock, pas l'action` |
| **Égrégore** | Le réflexe GitLab — `norme_prescrite: une chaîne n'est fiable que si elle se comporte comme GitLab` |

**Ce que le schéma fait apparaître et que le plan projet ne contient pas :**

- Le livrable est atteint bien avant l'objectif. Le pilote sera vert ; les autres équipes n'iront pas pour autant.
- Le risque qui coûte le plus n'est pas au registre : un pipeline rouge légitime, corrigé en deux heures, détruit plus d'adhésion qu'il ne coûte de temps.
- Les « pipelines historiques mal documentés » ne sont pas un problème technique mais des personnes dont l'expertise n'est pas sollicitée.
- L'objectif affiché « conserver une expérience proche de GitLab » est en réalité une norme collective, pas un critère technique — donc tout écart sera lu comme un défaut, même quand il est neutre ou meilleur.
- La seule arête qui nourrit le stock est la revue conjointe. Si le pilote est mené par l'équipe plateforme seule et présenté une fois terminé, il reste un livrable vert et un capital nul.

---

## 12. Limites connues — chantier v0.3

Trois choses que le format ne capte toujours pas :

- **La représentation asymétrique.** Un acteur agit selon la relation qu'il *croit* avoir. Le format modélise le lien réel, jamais sa représentation divergente d'un côté à l'autre — or c'est souvent la représentation qui gouverne l'action.
- **L'égrégore hostile.** Les collectifs de résistance constitués par agrégation d'acteurs induits sont représentables mais aucune règle de traitement n'existe.
- **La composition inter-Fourmis.** Liens et Égrégores sont par nature transverses à plusieurs actions. Il faudra un identifiant stable partagé entre schémas plutôt qu'une duplication de nœuds.
- **La double appartenance égrégorique.** Un acteur soumis à deux égrégores aux normes contradictoires — cas fréquent en transformation organisationnelle — n'est pas traité.

---

## Annexe — Squelette JSON minimal

Structure attendue par les outils de type arrows.app. Le détail technique complet (styles, gabarit de positions, validation géométrique) fait l'objet du document *Fourmi v0.2 — Guide technique de construction*.

```json
{
  "style": { "...": "voir guide technique" },
  "nodes": [
    {
      "id": "n0",
      "position": { "x": 1120, "y": 720 },
      "caption": "Verbe + objet de l'action",
      "style": { "node-color": "#f44e3b", "caption-color": "#ffffff", "caption-font-weight": "bold" },
      "labels": ["Action"],
      "properties": {}
    }
  ],
  "relationships": [
    { "id": "r0", "type": "réalise", "style": {}, "properties": {}, "fromId": "n1", "toId": "n0" }
  ]
}
```

---

*Format Fourmi — spécification canonique v0.2*
*Composante Relations en trois étages : Interaction · Lien · Égrégore*
