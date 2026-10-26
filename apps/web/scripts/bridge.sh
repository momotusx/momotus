#!/usr/bin/env bash
# Runs the dev bridge (mq-bridge) the web app talks to in development.
# The Trakt app (client id and redirect URI, read by `option_env!`) comes from
# .env.trakt.local at the repository root when this machine has it
# (gitignored). Without the file the core has no Trakt app (notConfigured), as
# in CI or a fresh clone. The TMDB read token (MARQUEE_TMDB_READ_TOKEN) comes
# from .env.tmdb.local the same way; without it the title page has no TMDB score.
# The torrent engine is built in, as in the apps' full builds; MQ_TORRENT=0
# leaves it out. Arguments go to the bridge.
# Only pages from the bridge's own origin and the dev server's
# (http://127.0.0.1:5173 or http://localhost:5173) may open its socket; a dev
# server on another port is named in MQ_BRIDGE_ORIGINS (comma-separated origins).
set -euo pipefail

root="$(cd "$(dirname "$0")/../../.." && pwd)"
for env in "$root/.env.trakt.local" "$root/.env.tmdb.local"; do
  if [[ -f "$env" ]]; then
    set -a
    source "$env"
    set +a
  fi
done
cd "$root"
if [[ "${MQ_TORRENT:-1}" == "0" ]]; then
  exec cargo run -p mq-bridge -- "$@"
fi
exec cargo run -p mq-bridge --features torrent -- "$@"
