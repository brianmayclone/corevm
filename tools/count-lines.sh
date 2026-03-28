#!/usr/bin/env bash
#
# Zählt Quelltextzeilen in apps/, libs/ und tools/
# Aufgeschlüsselt pro Produkt (Unterordner) und gesamt.
# Ausgeschlossen: node_modules, target, dist, build, .map-Dateien
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIRS=("apps" "libs" "tools")

# Quelltextdateien-Erweiterungen
EXTENSIONS="rs,ts,tsx,js,jsx,css,html,sh,toml,yml,yaml,sql,svelte,vue"

# Farben
BOLD='\033[1m'
CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
MAGENTA='\033[0;35m'
RESET='\033[0m'

# Alle Quelltextzeilen in einem Verzeichnis zählen
count_source_lines() {
    local dir="$1"
    local total=0

    IFS=',' read -ra EXT_ARRAY <<< "$EXTENSIONS"
    for ext in "${EXT_ARRAY[@]}"; do
        local lines
        lines=$(find "$dir" \
            \( -name node_modules -o -name target -o -name dist -o -name build -o -name .git -o -name vendor -o -name pkg \) -prune \
            -o -name "*.${ext}" -type f -print0 2>/dev/null \
            | xargs -0 cat 2>/dev/null \
            | wc -l)
        total=$((total + lines))
    done

    echo "$total"
}

# Quelltextdateien in einem Verzeichnis zählen
count_source_files() {
    local dir="$1"
    local total=0

    IFS=',' read -ra EXT_ARRAY <<< "$EXTENSIONS"
    for ext in "${EXT_ARRAY[@]}"; do
        local count
        count=$(find "$dir" \
            \( -name node_modules -o -name target -o -name dist -o -name build -o -name .git -o -name vendor -o -name pkg \) -prune \
            -o -name "*.${ext}" -type f -print 2>/dev/null \
            | wc -l)
        total=$((total + count))
    done

    echo "$total"
}

printf "\n${BOLD}══════════════════════════════════════════════════════════════════${RESET}\n"
printf "${BOLD}  CoreVM — Quelltextzeilen-Statistik${RESET}\n"
printf "${BOLD}══════════════════════════════════════════════════════════════════${RESET}\n\n"

grand_total_lines=0
grand_total_files=0

for folder in "${DIRS[@]}"; do
    folder_path="${REPO_ROOT}/${folder}"
    if [[ ! -d "$folder_path" ]]; then
        continue
    fi

    printf "${CYAN}${BOLD}📁 %s/${RESET}\n" "$folder"

    folder_total_lines=0
    folder_total_files=0

    # Unterordner (Produkte) auflisten
    for product_path in "${folder_path}"/*/; do
        [[ -d "$product_path" ]] || continue
        product_name=$(basename "$product_path")

        lines=$(count_source_lines "$product_path")
        files=$(count_source_files "$product_path")

        if [[ "$files" -gt 0 ]]; then
            printf "  ${GREEN}${BOLD}%-20s${RESET}  %'10d Zeilen  (%'d Dateien)\n" "$product_name" "$lines" "$files"
            folder_total_lines=$((folder_total_lines + lines))
            folder_total_files=$((folder_total_files + files))
        fi
    done

    # Direkte Dateien im Ordner (z.B. tools/*.sh)
    direct_lines=0
    direct_files=0
    IFS=',' read -ra EXT_ARRAY <<< "$EXTENSIONS"
    for ext in "${EXT_ARRAY[@]}"; do
        local_files=$(find "$folder_path" -maxdepth 1 -name "*.${ext}" -type f 2>/dev/null | wc -l)
        if [[ "$local_files" -gt 0 ]]; then
            local_lines=$(find "$folder_path" -maxdepth 1 -name "*.${ext}" -type f -print0 | xargs -0 cat 2>/dev/null | wc -l)
            direct_lines=$((direct_lines + local_lines))
            direct_files=$((direct_files + local_files))
        fi
    done

    if [[ "$direct_files" -gt 0 ]]; then
        printf "  ${GREEN}${BOLD}%-20s${RESET}  %'10d Zeilen  (%'d Dateien)\n" "(direkt)" "$direct_lines" "$direct_files"
        folder_total_lines=$((folder_total_lines + direct_lines))
        folder_total_files=$((folder_total_files + direct_files))
    fi

    printf "  ${YELLOW}──────────────────────────────────────────────────────${RESET}\n"
    printf "  ${YELLOW}${BOLD}%-20s  %'10d Zeilen  (%'d Dateien)${RESET}\n" "Summe ${folder}/" "$folder_total_lines" "$folder_total_files"
    printf "\n"

    grand_total_lines=$((grand_total_lines + folder_total_lines))
    grand_total_files=$((grand_total_files + folder_total_files))
done

printf "${BOLD}══════════════════════════════════════════════════════════════════${RESET}\n"
printf "  ${MAGENTA}${BOLD}GESAMTSUMME:  %'10d Zeilen  in  %'d Dateien${RESET}\n" "$grand_total_lines" "$grand_total_files"
printf "${BOLD}══════════════════════════════════════════════════════════════════${RESET}\n\n"
