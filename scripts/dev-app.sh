#!/usr/bin/env bash

set -u

cache_root="${PROMPTRUNNER_CACHE_DIR:-${XDG_CACHE_HOME:-${TMPDIR:-/tmp}/PromptRunner}/rust-target}"
export CARGO_TARGET_DIR="$cache_root"

cleanup_build_cache() {
  local target_dir="$CARGO_TARGET_DIR"
  local target_mb=0

  if [ -d "$target_dir" ]; then
    target_mb="$(du -sm "$target_dir" 2>/dev/null | awk '{print $1}')"
    target_mb="${target_mb:-0}"
    if [ "$target_mb" -ge 2048 ]; then
      local incremental_dir="$target_dir/debug/incremental"
      if [ -d "$incremental_dir" ]; then
        echo "PromptRunner: очищаю только временный incremental-кэш (${target_mb} MB)"
        rm -rf "$incremental_dir"
      fi
    fi
  fi
}

# Убираем только старый временный кэш до нового запуска; текущая сборка сохраняется.
cleanup_build_cache

./node_modules/.bin/tauri dev "$@"
