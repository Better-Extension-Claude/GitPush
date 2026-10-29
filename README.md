# GitPush

Un bouton **↑ GitPush** collé sur la fenêtre de Claude desktop (Windows). Un clic ouvre la fenêtre GitPush pour la conversation affichée dans Claude, et pousse son projet sur GitHub sans passer par Claude.

## La fenêtre GitPush

- **Conversation** : celle affichée dans Claude est choisie d'office ; la liste permet d'en prendre une autre.
- **Dépôt** : un de tes dépôts GitHub, une adresse collée, ou un nouveau dépôt (privé ou public).
- **Branche** : une branche existante ou un nouveau nom.
- **Fichiers** : arbre à cocher, pré-coché avec le dernier push, qu'il vienne de toi ou de Claude.
- **Pousser** : chaque push envoie exactement les éléments cochés. Un fichier décoché disparaît du dépôt au commit suivant (il reste sur ton disque). **Forcer** apparaît si la branche distante a divergé.

## Le bouton

- Clic gauche : ouvre la fenêtre GitPush.
- Clic droit maintenu : déplace le bouton ; sa position est retenue.
- Il se cache pendant que tu déplaces Claude à la souris et revient au lâcher.
- « Replacer le bouton » et « Quitter GitPush » sont en haut de la fenêtre.
- Il se ferme tout seul avec Claude.

## Connexion à GitHub

GitPush a son propre accès à GitHub, indépendant de Claude : Claude n'a pas besoin du connecteur GitHub.

Il passe par [GitHub CLI](https://cli.github.com) (`gh`). Le compte connecté s'affiche en haut à droite de la fenêtre :

- **Connecter GitHub** ouvre une console puis le navigateur pour te connecter, et règle git pour que les push passent par ce compte. GitPush détecte la connexion tout seul.
- Cliquer sur le compte connecté permet d'en changer.
- **Installer GitHub CLI** apparaît si `gh` manque.

Nécessite aussi `git`.

## Installation

Compiler et installer dans `%LOCALAPPDATA%\GitPush\GitPush.exe` (Rust, chaîne `stable-x86_64-pc-windows-gnu`) :

```bash
bash app/build.sh
```

Démarrer avec Claude : ajouter dans `~/.claude/settings.json`

```json
"hooks": {
  "SessionStart": [
    { "hooks": [ { "type": "command", "command": "C:/Users/<toi>/AppData/Local/GitPush/GitPush.exe", "args": ["--launch"], "timeout": 10 } ] }
  ]
}
```

## Dépannage

Lancer GitPush avec la variable d'environnement `GITPUSH_DEBUG=1` écrit un journal dans `%LOCALAPPDATA%\GitPush\debug.log`.
