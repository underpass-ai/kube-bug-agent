#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if rg -n '\b(adapters|cli|application|ports|reqwest|rusqlite|kube|k8s_openapi|axum|tokio)::' src/domain src/backpressure/domain; then
    echo 'Domain must not depend on application, ports or adapters' >&2
    exit 1
fi
if rg -n '\b(adapters|cli|reqwest|rusqlite|kube|k8s_openapi|axum)::' src/application src/ports src/backpressure/application src/backpressure/ports; then
    echo 'Application and ports must not depend on adapters or CLI' >&2
    exit 1
fi
while IFS= read -r file; do
    count=$(rg -c '^\s*(pub(\([^)]*\))?\s+)?(struct|enum|trait)\s+' "$file" || true)
    if [[ ${count:-0} -gt 1 ]]; then
        echo "More than one primary type in $file" >&2
        exit 1
    fi
done < <(rg --files src -g '*.rs')
