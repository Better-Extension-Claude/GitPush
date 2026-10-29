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
echo "Prêt : dist/GitPush.exe"
