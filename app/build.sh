#!/usr/bin/env bash
# Compile GitPush (chaîne Rust GNU) et dépose l'exe dans dist/GitPush.exe.
# Lancer cet exe ouvre l'installateur intégré (dossier, démarrage avec Claude, prérequis).
# Le dossier de build est hors du dépôt : dlltool ne supporte pas le « & » du chemin.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$LOCALAPPDATA/GitPush/build"
cargo build --release
mkdir -p ../dist
cp "$CARGO_TARGET_DIR/release/gitpush.exe" ../dist/GitPush.exe
# Manifeste ExtraClaude : dist/ s'ajoute tel quel comme extension (« Ajouter une extension… »).
version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cat > ../dist/extension.json <<JSON
{
  "id": "gitpush",
  "name": "GitPush",
  "version": "$version",
  "description": "Envoyez votre projet sur GitHub en un clic, sans passer par Claude.",
  "exe": "GitPush.exe",
  "start": ["--button"]
}
JSON
echo "Prêt : dist/GitPush.exe et dist/extension.json"
# Installe la nouvelle version dans ExtraClaude (et la relance si elle est activée).
# Sorties détournées : le bouton relancé hériterait sinon de celles du script, qui ne
# rendrait jamais la main.
"$LOCALAPPDATA/Programs/ExtraClaude/ExtraClaude.exe" --add "$(cygpath -w "$PWD/../dist/extension.json")" </dev/null >/dev/null 2>&1 \
  && echo "Installée dans ExtraClaude." || echo "Installation dans ExtraClaude impossible."
