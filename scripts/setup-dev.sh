#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cache="${XDG_CACHE_HOME:-$HOME/.cache}/gitpanel"
target="$cache/target"

mkdir -p "$target"

if [ -L "$root/target" ]; then
    current=$(readlink "$root/target" || true)
    if [ "$current" != "$target" ]; then
        ln -sfn "$target" "$root/target"
    fi
elif [ -e "$root/target" ]; then
    echo "target/ já existe e não é um symlink; mantendo como está." >&2
else
    ln -s "$target" "$root/target"
fi

if ! command -v mold >/dev/null 2>&1; then
    echo "Aviso: mold não encontrado; o linker padrão será usado." >&2
fi

echo "target -> $target"
