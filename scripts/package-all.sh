#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
output_dir="${OUTPUT_DIR:-${project_dir}/release}"
workflow_file="package-windows.yml"
artifact_name="promptrunner-windows"
windows_file_name="PromptRunner-windows-x64-setup.exe"
mac_app_name="PromptRunner.app"

fail() {
  printf 'Ошибка: %s\n' "$*" >&2
  exit 1
}

command -v npm >/dev/null || fail 'Не найден npm.'
command -v gh >/dev/null || fail 'Не найден GitHub CLI (gh).'
command -v shasum >/dev/null || fail 'Не найдена утилита shasum.'
[[ "$(uname -s)" == "Darwin" ]] || fail 'Этот скрипт запускается на macOS: .app собирается локально, а .exe — на Windows GitHub runner.'

cd "${project_dir}"

# Windows runner получает исходники из GitHub. Проверяем, что обе платформы
# собирают один и тот же уже опубликованный коммит.
[[ -z "$(git status --porcelain)" ]] || fail 'Есть незакоммиченные изменения. Закоммитьте и отправьте их в GitHub, затем повторите запуск.'
branch="$(git branch --show-current)"
[[ -n "${branch}" ]] || fail 'Нужна активная Git-ветка, а не detached HEAD.'
local_commit="$(git rev-parse HEAD)"
remote_commit="$(git ls-remote --heads origin "refs/heads/${branch}" | awk '{print $1}')"
[[ -n "${remote_commit}" ]] || fail "Ветка ${branch} ещё не опубликована в origin."
[[ "${local_commit}" == "${remote_commit}" ]] || fail 'Текущий коммит не отправлен в GitHub. Выполните git push и повторите запуск.'
gh auth status >/dev/null 2>&1 || fail 'GitHub CLI не авторизован. Выполните gh auth login.'

mkdir -p "${output_dir}"
rm -rf "${output_dir:?}/${mac_app_name}"
rm -f "${output_dir:?}/${windows_file_name}" "${output_dir:?}/SHA256SUMS"

printf 'Собираю macOS-приложение…\n'
npm run tauri build -- --bundles app --config src-tauri/tauri.package.conf.json

mac_app_source="${project_dir}/src-tauri/target/release/bundle/macos/${mac_app_name}"
[[ -d "${mac_app_source}" ]] || fail "Tauri не создал ${mac_app_source}."
ditto "${mac_app_source}" "${output_dir}/${mac_app_name}"

build_id="$(date -u +%Y%m%dT%H%M%SZ)-$$"
run_title="Windows package (${build_id})"
printf 'Запускаю Windows-сборку в GitHub Actions…\n'
gh workflow run "${workflow_file}" --ref "${branch}" -f "build_id=${build_id}"

run_id=''
for _ in {1..30}; do
  run_id="$(gh run list --workflow "${workflow_file}" --branch "${branch}" --event workflow_dispatch --limit 30 --json databaseId,displayTitle --jq ".[] | select(.displayTitle == \"${run_title}\") | .databaseId" | head -n 1)"
  [[ -n "${run_id}" ]] && break
  sleep 2
done
[[ -n "${run_id}" ]] || fail 'Не удалось найти запущенную Windows-сборку в GitHub Actions.'

gh run watch "${run_id}" --exit-status

download_dir="$(mktemp -d "${output_dir}/.windows-artifact.XXXXXX")"
trap 'rm -rf "${download_dir}"' EXIT
gh run download "${run_id}" --name "${artifact_name}" --dir "${download_dir}"
windows_installer="$(find "${download_dir}" -type f -name '*.exe' -print -quit)"
[[ -n "${windows_installer}" ]] || fail 'В артефакте Windows-сборки не найден .exe-файл.'
cp "${windows_installer}" "${output_dir}/${windows_file_name}"

# .app — это каталог, а не один файл. Поэтому здесь хешируется исполняемый
# файл внутри bundle; .exe хешируется целиком.
mac_executable="$(find "${output_dir}/${mac_app_name}/Contents/MacOS" -maxdepth 1 -type f -perm -111 -print -quit)"
[[ -n "${mac_executable}" ]] || fail 'Внутри .app не найден исполняемый файл.'
(
  cd "${output_dir}"
  shasum -a 256 "${mac_executable#${output_dir}/}" "${windows_file_name}"
) > "${output_dir}/SHA256SUMS"

printf '\nГотово:\n  %s\n  %s\n  %s\n' \
  "${output_dir}/${mac_app_name}" \
  "${output_dir}/${windows_file_name}" \
  "${output_dir}/SHA256SUMS"
