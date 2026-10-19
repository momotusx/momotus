#!/usr/bin/env bash
# Runs the dev bridge (mq-bridge) the web app talks to in development.
# The Trakt app (client id and redirect URI, read by `option_env!`) comes from
# .env.trakt.local at the repository root when this machine has it
# (gitignored). Without the file the core has no Trakt app (notConfigured), as
# in CI or a fresh clone. Arguments go to the bridge.
set -euo pipefail

root="$(cd "$(dirname "$0")/../../.." && pwd)"
if [[ -f "$root/.env.trakt.local" ]]; then
  set -a
  source "$root/.env.trakt.local"
  set +a
fi
cd "$root"
exec cargo run -p mq-bridge -- "$@"
