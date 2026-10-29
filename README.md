# GitPush (push-projet)

Mod Claude Code pour pousser le projet d'une conversation sur GitHub sans demander à Claude.

- `/push-projet` ouvre une fenêtre au-dessus de la zone de saisie : dépôt, branche, fichiers et dossiers à cocher, message de commit, bouton **Pousser**.
- Les fichiers sont pré-cochés avec le dernier push, qu'il vienne de toi ou de Claude (un `git push` lancé par Claude est détecté).
- Onglet **Dépôt & branche** : choisir un de tes dépôts GitHub, une branche existante ou nouvelle, créer/recréer un dépôt (privé ou public).
- `/push-rapide` pousse tout de suite avec la config du dernier push.

Chaque push envoie exactement les éléments cochés : un fichier décoché disparaît du dépôt au commit suivant (il reste sur ton disque).

Nécessite `git`, et `gh` connecté (`gh auth login`) pour lister ou créer des dépôts.

## Installation

Dans un terminal Claude Code :

```
/plugin install push-projet --marketplace zomone/GitPush
```

Depuis un shell :

```
claude plugin install push-projet --marketplace zomone/GitPush
```

Dans l'app desktop : + à côté de la saisie → Plugins → Add marketplace `zomone/GitPush` → Add plugin.
