# The hosted relay

Marquee's sync relay (`mq-sync-server`) at `https://momotus.normalisdead.mom`, on a small Linux server behind a Cloudflare tunnel, with the link host's static pages on the same name from a Cloudflare Worker. The plan and its reasons: `plans/accounts-social-sync.md`. Every command below runs on the Mac; `MQ_RELAY_SSH` is the server's SSH target, which no file in this repository names.

| File | What |
|---|---|
| `momotus-relay.service` | the relay, loopback `127.0.0.1:8787`, user `momotus`, data in `/var/lib/momotus`, the identity file through `LoadCredential` |
| `momotus-backup.service`, `.timer` | `backup.py` nightly at 04:20, 14 sets kept |
| `backup.py` | SQLite's online backup, integrity check, the roster journal, `SHA256SUMS` |
| `identity.py` | creates the identity file (a new key, never printed) and merges the provider values into it |
| `deploy.sh` | build, backup, preflight on a copy, switch, health, rollback |
| `pull-backup.sh`, `app.marquee.relay-backup.plist` | the Mac's daily pull of the newest set, 7 kept |

Checks: `bun test deploy/relay`, `uv run python -I deploy/relay/backup.py --self-test`, `uv run python -I deploy/relay/identity.py --self-test`, `shellcheck deploy/relay/*.sh`.

## First time

The SSH account runs `sudo` without a password for these commands (`deploy.sh`, `identity.py`, the backup pull); a narrower rule than full `NOPASSWD` is the owner's choice (the backup pull's line is below).

```sh
# The service user, and the identity file with a fresh key (the script prints only its fingerprint).
ssh "$MQ_RELAY_SSH" 'sudo useradd --system --no-create-home --shell /usr/sbin/nologin momotus'
ssh "$MQ_RELAY_SSH" 'sudo python3 -I - create' < deploy/relay/identity.py
# The owner, in his own terminal, copies the key into his password manager:
#   ssh <server> sudo cat /etc/momotus/identity.json
# Then the relay itself (also installs and enables the units and the backup timer):
MQ_RELAY_SSH=... deploy/relay/deploy.sh
```

`deploy.sh` builds the checked-out commit, which must be on the trunk with no uncommitted change under `crates` or here, as a static `x86_64-unknown-linux-musl` binary (`cargo zigbuild`, its own target dir under `~/.cargo-targets/`). The relay has no schema migration: a commit that changes `SCHEMA_VERSION` fails the preflight on a copy of the database, and nothing switches until a migration exists.

## Cloudflare

1. Read and save the tunnel's config (`cf tunnels config get <tunnel id>`) outside the repository; list the zone's DNS records; read the zone's security settings.
2. `cf tunnels config update` with the read config plus `{"hostname": "momotus.normalisdead.mom", "service": "http://127.0.0.1:8787"}` first; the catch-all stays last.
3. A proxied `CNAME momotus` to `<tunnel id>.cfargotunnel.com`.
4. The link host:

```sh
RELAY_URL=https://momotus.normalisdead.mom PROVIDERS=google,facebook \
ANDROID_PACKAGES=app.marquee.android,app.marquee.android.walk \
ANDROID_SIGNING_SHA256=<the signing certificate's SHA-256> bun apps/linkhost/build.ts
# wrangler, not cf deploy: cf deploy ignores wrangler.jsonc and makes a new project
(cd apps/linkhost && bunx wrangler@4.45.0 deploy --dry-run && bunx wrangler@4.45.0 deploy)
```

Rollback: the saved tunnel config back, the CNAME deleted, the Worker deleted.

## Sign-in providers

The owner's console values go into `.env.accounts.local` at the repository root (gitignored). They are piped to the server, never printed:

```sh
ssh "$MQ_RELAY_SSH" 'sudo python3 -I /opt/momotus/identity.py merge' < .env.accounts.local
ssh "$MQ_RELAY_SSH" 'sudo systemctl restart momotus-relay'
curl -s https://momotus.normalisdead.mom/v1/relay   # providers: ["google","facebook"]
```

`merge` keeps the key (it refuses a file without one), keeps the old file as `identity.json.prev`, and prints the key's fingerprint, which must not change.

## Apps against the hosted relay

The hosted values are never committed: builds read them from `.env.hosted.local` at the repository root (gitignored, nothing secret in it):

```sh
MARQUEE_SYNC_SERVER=https://momotus.normalisdead.mom
MARQUEE_LINK_HOST=momotus.normalisdead.mom
MARQUEE_GOOGLE_WEB_CLIENT_ID=<the web client id>
```

```sh
set -a; . ./.env.hosted.local; set +a
# Android (the core first, with the link host; add -Pmarquee.applicationIdSuffix=.walk for a walk build)
(cd apps/android && MARQUEE_LINK_HOST=$MARQUEE_LINK_HOST scripts/build-core.sh && ./gradlew assembleRelease \
  -Pmarquee.syncServer=$MARQUEE_SYNC_SERVER -Pmarquee.linkHost=$MARQUEE_LINK_HOST -Pmarquee.googleWebClientId=$MARQUEE_GOOGLE_WEB_CLIENT_ID)
# Apple, in a fresh shell (the line above exported MARQUEE_LINK_HOST, which an Apple core
# must not carry: no associated domains without the paid Apple program): the sync service only
env -u MARQUEE_LINK_HOST xcodebuild ... MARQUEE_SYNC_SERVER=$MARQUEE_SYNC_SERVER
# Web: apps/web/.env.production.local holds VITE_MARQUEE_SYNC_SERVER; mq-bridge built with MARQUEE_LINK_HOST
```

A walk build (`-Pmarquee.applicationIdSuffix=.walk`) installs beside the owner's app; its home-screen shortcut still opens `app.marquee.android` (`res/xml/shortcuts.xml` names the package), so a walk never uses the shortcut.

The relay runs with `--trust-proxy`: a budgeted call to it on the server itself (a `curl` to `127.0.0.1:8787/v1/pairings`) needs a `cf-connecting-ip` header, else it answers 400 `noClientAddress`; `/v1/relay` is not budgeted.

Release note for the per-addon sync: a profile whose addon list is older than the recommended-addons step gets the newer recommended addons, on, on each device after its first pull, and they then sync; turning one off sticks on every device.

The web app signs in only from `http://127.0.0.1:<port>`: the relay sends brokered returns only to loopback, the app scheme and the link host's `/delete`.

## Backups on the Mac

```sh
mkdir -p ~/.local/share/momotus
cp deploy/relay/pull-backup.sh ~/.local/share/momotus/pull-backup.sh
sed -e "s|__SCRIPT__|$HOME/.local/share/momotus/pull-backup.sh|" -e "s|__SSH__|$MQ_RELAY_SSH|" \
  deploy/relay/app.marquee.relay-backup.plist > ~/Library/LaunchAgents/app.marquee.relay-backup.plist
launchctl bootstrap "gui/$(id -u)" ~/Library/LaunchAgents/app.marquee.relay-backup.plist
```

The pull reads the server's backups with `sudo -n`. The narrowest rule for the account it uses is one sudoers line, `<user> ALL=(root) NOPASSWD: /usr/bin/ls -1 /var/lib/momotus/backups, /usr/bin/cat /var/lib/momotus/backups/*`, added by the owner if he wants the pull to run without full sudo.

It writes `~/.local/share/momotus/relay-backups/pull.log` and shows a notification when the last good pull or the public health check is more than 48 hours old. The folder is excluded from Time Machine.

## Restore

1. `sudo systemctl stop momotus-relay`.
2. Remove `/var/lib/momotus/relay.sqlite-wal` and `relay.sqlite-shm`: beside a restored database they corrupt it. Never copy `relay.sqlite` while the relay runs; restore only from `backup.py` sets (check them with `shasum -a 256 -c <stamp>.SHA256SUMS`).
3. The set's `.sqlite` becomes `relay.sqlite`. The journal: the server's own `relay.roster.jsonl` when its disk survived (it is a superset), else the set's. Owner `momotus`, mode 0600.
4. One start with `--new-generation`, so devices send back what the backup lost: a runtime drop-in (`sudo mkdir -p /run/systemd/system/momotus-relay.service.d`, then this file as `restore.conf` in it; copy the `ExecStart` line from the installed unit and add the flag, so it matches the unit as it is now):

   ```ini
   [Service]
   ExecStart=
   ExecStart=/opt/momotus/bin/mq-sync-server --db /var/lib/momotus/relay.sqlite --listen 127.0.0.1:8787 --trust-proxy --public-host momotus.normalisdead.mom --identity %d/identity.json --new-generation
   ```

   `sudo systemctl daemon-reload && sudo systemctl start momotus-relay`; `/v1/relay` shows a new `generation`.
5. Remove that drop-in folder and `sudo systemctl daemon-reload` (not `systemctl revert`). The next start keeps the stored generation.
6. For one hour devices may re-create accounts made after the backup by replay (20 per client address). If devices that matter missed the hour, start once more with the flag: it opens a new hour.

A restore from the Mac's copy after the server's disk is lost brings back accounts deleted after that copy was pulled: the server's journal, which held their deletion, is gone with the disk.
