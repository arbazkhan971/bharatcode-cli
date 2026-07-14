#!/usr/bin/env bash
set -euo pipefail

COMMITTED="crates/bharatcode-server/ui/desktop/openapi.json"
GENERATED="$(mktemp)"
trap 'rm -f "$GENERATED"' EXIT

echo "🔍 Checking OpenAPI schema is up-to-date..."

if [ ! -f "$COMMITTED" ]; then
  echo "❌ Committed schema not found at $COMMITTED"
  exit 1
fi

BHARATCODE_OPENAPI_OUTPUT="$GENERATED" \
  cargo run --quiet -p bharatcode-server --bin generate_schema >/dev/null

if ! diff -u --ignore-space-change "$COMMITTED" "$GENERATED"; then
  echo ""
  echo "❌ OpenAPI schema is out of date!"
  echo ""
  echo "The schema generated from the server routes differs from the committed copy."
  echo "This usually means API types were added or modified without updating the schema."
  echo ""
  echo "Run 'just generate-openapi', then commit $COMMITTED."
  exit 1
fi

echo "✅ OpenAPI schema is up-to-date"
