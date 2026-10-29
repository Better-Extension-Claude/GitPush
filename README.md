# GitPush

Un bouton **↑ GitPush** collé sur la fenêtre de Claude desktop (Windows). Un clic ouvre la fenêtre GitPush pour la conversation affichée dans Claude, et pousse son projet sur GitHub sans passer par Claude.

## La fenêtre GitPush

- **Conversation** : celle affichée dans Claude est choisie d'office ; la liste permet d'en prendre une autre.
- **Dossier** : le dossier de la conversation, ou un des dossiers ajoutés à la conversation ou modifiés par Claude ; « Autre dossier… » pour en choisir un autre (retenu pour la suite). Utile pour une conversation sans dossier de projet.
- **Dépôt** : un de tes dépôts GitHub, une adresse collée, ou un nouveau dépôt (privé ou public).
- **Branche** : une branche existante ou un nouveau nom.
- **Fichiers** : arbre à cocher, pré-coché avec le dernier push, qu'il vienne de toi ou de Claude. Pour un projet jamais poussé avec GitPush (poussé par Claude, par le site GitHub…, même sans dossier `.git`), choisir le dépôt coche les fichiers déjà en ligne (« ↺ Fichiers du dépôt » pour le refaire).
- **Pousser** : chaque push envoie exactement les éléments cochés. Un fichier décoché disparaît du dépôt au commit suivant (il reste sur ton disque). **Forcer** apparaît si la branche distante a divergé.

## Le bouton

- Clic gauche : ouvre la fenêtre GitPush.
- Clic droit maintenu : déplace le bouton ; sa position est retenue.
- Il se cache pendant que tu déplaces Claude à la souris et revient au lâcher.
- « Replacer le bouton » est en haut de la fenêtre GitPush. Celle-ci se déplace par son en-tête et se ferme par sa croix (ou Échap) ; recliquer sur le bouton la ramène devant, même réduite.
- Il se ferme tout seul avec Claude.

## Connexion à GitHub

GitPush a son propre accès à GitHub, indépendant de Claude : Claude n'a pas besoin du connecteur GitHub.

Il passe par [GitHub CLI](https://cli.github.com) (`gh`). Le compte connecté s'affiche en haut à droite de la fenêtre :

- **Connecter GitHub** ouvre la page de connexion GitHub dans le navigateur ; GitPush affiche le code à y coller (déjà copié). Pour un autre compte que celui ouvert dans le navigateur, changer de compte sur cette page.
- Le menu du compte connecté permet de passer d'un compte à l'autre, d'en ajouter un ou de se déconnecter. Les push passent toujours par le compte actif.
- **Installer GitHub CLI** apparaît si `gh` manque.

Nécessite aussi `git` : la fenêtre propose **Installer Git** s'il manque.

## Installation

1. Télécharger `GitPush.exe` dans la dernière [release](../../releases/latest).
2. Le lancer : c'est l'installateur.
   - **Dossier** : où copier GitPush (par défaut `%LOCALAPPDATA%\Programs\GitPush`).
   - **Démarrer avec Claude** : ajoute le hook dans `~/.claude/settings.json`, avec le bon chemin ; le reste du fichier est conservé, et une copie de l'ancien est gardée à côté (`settings.json.gitpush.bak`).
   - **Prérequis** : Git et GitHub CLI, avec un bouton **Installer** (winget) pour ceux qui manquent.
3. **Installer** : le bouton apparaît sur Claude.

Le fichier téléchargé peut ensuite être supprimé. Windows peut afficher un avertissement SmartScreen (l'exe n'est pas signé) : « Informations complémentaires » → « Exécuter quand même ».

Mettre à jour : quand une nouvelle version sort, la fenêtre GitPush affiche **Mettre à jour (vX)** en haut à droite ; le bouton reste là tant qu'on ne l'utilise pas. Un clic télécharge la nouvelle version, ferme la fenêtre, remplace l'exe (hook conservé) et relance le bouton. Depuis une version antérieure à 1.0.2 : lancer le nouveau `GitPush.exe` et cliquer sur **Mettre à jour**. Désinstaller : lancer `GitPush.exe --install` puis **Désinstaller** (retire aussi le hook).

Installation sans fenêtre : `GitPush.exe --install-to "<dossier>"`.

## Compiler

Rust, chaîne `stable-x86_64-pc-windows-gnu` :

```bash
bash app/build.sh
```

L'exe compilé est dans `dist/GitPush.exe`.

## Dépannage

Lancer GitPush avec la variable d'environnement `GITPUSH_DEBUG=1` écrit un journal dans `%LOCALAPPDATA%\GitPush\debug.log`.
