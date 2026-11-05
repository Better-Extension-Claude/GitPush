# GitPush

Un bouton **↑ GitPush** collé sur la fenêtre de Claude desktop (Windows). Un clic ouvre la fenêtre GitPush pour la conversation affichée dans Claude, et pousse son projet sur GitHub sans passer par Claude.

## La fenêtre GitPush

- **Conversation** : celle affichée dans Claude est choisie d'office ; la liste permet d'en prendre une autre.
- **Dossier** : le dossier de la conversation, ou un des dossiers ajoutés à la conversation ou modifiés par Claude ; « Autre dossier… » ouvre l'explorateur Windows pour en choisir un autre (retenu pour la suite). Utile pour une conversation sans dossier de projet.
- **Dépôt** : un de tes dépôts GitHub, une adresse collée, ou **Nouveau** pour en créer un (privé ou public).
- **Branche** : une branche existante, ou **Nouveau** pour taper le nom d'une nouvelle branche (créée au push).
- **Fichiers** : arbre à cocher, pré-coché avec le dernier push, qu'il vienne de toi ou de Claude. Pour un projet jamais poussé avec GitPush (poussé par Claude, par le site GitHub…, même sans dossier `.git`), choisir le dépôt coche les fichiers déjà en ligne (« ↺ Fichiers du dépôt » pour le refaire).
- **Pousser** : chaque push envoie exactement les éléments cochés. Un fichier décoché disparaît du dépôt au commit suivant (il reste sur ton disque). **Forcer** apparaît si la branche distante a divergé.

## Le bouton

- Clic gauche : ouvre la fenêtre GitPush.
- Clic droit maintenu : déplace le bouton ; sa position est retenue. Clic droit sans bouger : retour à sa place par défaut.
- Il se cache pendant que tu déplaces Claude à la souris et revient au lâcher.
- La fenêtre GitPush se déplace par son en-tête et se ferme par sa croix (ou Échap) ; recliquer sur le bouton la ramène devant, même réduite.
- Il se ferme tout seul avec Claude.

## Connexion à GitHub

GitPush a son propre accès à GitHub, indépendant de Claude : Claude n'a pas besoin du connecteur GitHub.

Il passe par [GitHub CLI](https://cli.github.com) (`gh`). Le compte connecté s'affiche en haut à droite de la fenêtre :

- **Connecter GitHub** ouvre la page de connexion GitHub dans le navigateur ; GitPush affiche le code à y coller (déjà copié). Pour un autre compte que celui ouvert dans le navigateur, changer de compte sur cette page.
- Le menu du compte connecté permet de passer d'un compte à l'autre, d'en ajouter un ou de se déconnecter. Les push passent toujours par le compte actif.
- **Installer GitHub CLI** apparaît si `gh` manque.

Nécessite aussi `git` : la fenêtre propose **Installer Git** s'il manque.

## Installation

GitPush est une extension de **BetterClaude**, le gestionnaire d'extensions de Claude : il s'installe, se met à jour et se désinstalle uniquement par BetterClaude (onglet **Découverte**). BetterClaude le lance au début de chaque session ; l'activer ou le désactiver se fait dans **Mes extensions**.

Lancé seul, `GitPush.exe` ne s'installe pas lui-même : si son `extension.json` est à côté et que BetterClaude est installé, il demande à BetterClaude de l'installer (`BetterClaude.exe --add`) ; sinon, il indique de passer par BetterClaude.

Couleurs : celles du thème réglé dans BetterClaude (Réglages → Apparence), qui suit Claude par défaut.

Réglages et journaux : `%LOCALAPPDATA%\BetterExt\data\gitpush\` (ceux d'avant la 1.1, dans `%APPDATA%\GitPush`, sont repris d'office).

## Compiler

Rust, chaîne `stable-x86_64-pc-windows-gnu`. GitPush utilise `claude-band` (code partagé des extensions) et le SDK `BetterClaude/sdk`, attendus à côté du dossier `GitPush` :

```bash
bash app/build.sh
```

L'exe compilé est dans `dist/GitPush.exe`, avec son manifeste `dist/extension.json` ; le script l'installe ensuite dans BetterClaude (`BetterClaude.exe --add`), qui le relance s'il est activé.

## Dépannage

Lancer GitPush avec la variable d'environnement `GITPUSH_DEBUG=1` écrit un journal `debug.log` dans le dossier de données. Un bouton resté coincé sous Claude se relance seul (une fois par minute au plus) et le note dans `incidents.log`.
