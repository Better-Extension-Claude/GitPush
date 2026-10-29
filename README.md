# GitPush (push-projet)

Mod Claude Code pour pousser le projet d'une conversation sur GitHub sans demander à Claude.

- `/push-projet` ouvre une fenêtre au-dessus de la zone de saisie : dépôt, branche, fichiers et dossiers à cocher, message de commit, bouton **Pousser**.
- Les fichiers sont pré-cochés avec le dernier push, qu'il vienne de toi ou de Claude (un `git push` lancé par Claude est détecté).
- Onglet **Dépôt & branche** : choisir un de tes dépôts GitHub, une branche existante ou nouvelle, créer/recréer un dépôt (privé ou public).
- `/push-rapide` pousse tout de suite avec la config du dernier push.

Chaque push envoie exactement les éléments cochés : un fichier décoché disparaît du dépôt au commit suivant (il reste sur ton disque).

Nécessite `git`, et `gh` connecté (`gh auth login`) pour lister ou créer des dépôts.

## App GitPush (bouton flottant, dossier `app/`)

Petite app Rust qui colle un bouton **↑ GitPush** sur la fenêtre de Claude desktop. Un clic ouvre un modal par-dessus Claude, déjà relié à la conversation où tu viens d'écrire (liste déroulante pour en choisir une autre) : dépôt, branche, arbre de fichiers pré-cochés selon le dernier push, message, **Pousser**. Le bouton se déplace par glisser-déposer. L'app se ferme toute seule quand Claude est fermé.

Compilation : `bash app/build.sh` (Rust chaîne `stable-x86_64-pc-windows-gnu`).

Démarrage avec Claude : ajouter dans `~/.claude/settings.json`

```json
"hooks": {
  "SessionStart": [
    { "hooks": [ { "type": "command", "command": "C:/Users/<toi>/AppData/Local/GitPush/GitPush.exe", "args": ["--launch"], "timeout": 10 } ] }
  ]
}
```

## Installation du mod

Dans un terminal Claude Code :

```
/plugin install push-projet --marketplace zomone/GitPush
```

Depuis un shell :

```
claude plugin install push-projet --marketplace zomone/GitPush
```

Dans l'app desktop : + à côté de la saisie → Plugins → Add marketplace `zomone/GitPush` → Add plugin.
