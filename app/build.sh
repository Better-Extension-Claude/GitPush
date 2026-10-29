#!/usr/bin/env bash
# Compile GitPush (chaîne Rust GNU) et l'installe dans %LOCALAPPDATA%\GitPush.
# Le dossier de build est hors du dépôt : dlltool ne supporte pas le « & » du chemin.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$LOCALAPPDATA/GitPush/build"
cargo build --release
cp "$CARGO_TARGET_DIR/release/gitpush.exe" "$LOCALAPPDATA/GitPush/GitPush.exe"
echo "Installé : $LOCALAPPDATA/GitPush/GitPush.exe"
