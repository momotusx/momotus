# The hosted relay

Marquee's sync relay (`mq-sync-server`) at `https://momotus.normalisdead.mom`, on a small Linux server behind a Cloudflare tunnel, with the link host's static pages on the same name from a Cloudflare Worker. The plan and its reasons: `plans/accounts-social-sync.md`. Every command below runs on the Mac; `MQ_RELAY_SSH` is the server's SSH target, which no file in this repository names.

| File | What |
|---|---|
| `momotus.env` | Momotus's instance: name (service user, `/opt`, `/var/lib` and `/etc` folders, unit names), public host, ports, flags, credential files |
| `relay.service` | the relay unit's template: loopback `127.0.0.1:<PORT>`, user `<NAME>`, data in `/var/lib/<NAME>`, the identity file through `LoadCredential`; installed as `<NAME>-relay.service` |
| `backup.service`, `.timer` | templates of `<NAME>-backup.*`: `backup.py` nightly at 04:20, 14 sets kept |
| `render.sh` | renders the three units of an instance (`render.sh momotus.env OUT_DIR` to read them) |
| `backup.py` | SQLite's online backup, integrity check, the roster journal, `SHA256SUMS` |
| `identity.py` | creates the identity file (a new key, never printed) and merges the provider values into it |
| `deploy.sh` | build, backup, preflight on a copy, switch, health, rollback, for one instance file (`momotus.env` by default) |
| `pull-backup.sh`, `app.momotus.relay-backup.plist` | the Mac's daily pull of the newest set, 7 kept |

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

`deploy.sh` builds the checked-out commit, which must be on GitHub's `main` (the hosted terms page points users at the published source), with no uncommitted change under `crates` or here, as a static `x86_64-unknown-linux-musl` binary (`cargo zigbuild`, its own target dir under `~/.cargo-targets/`). The relay has no schema migration: a commit that changes `SCHEMA_VERSION` fails the preflight on a copy of the database, and nothing switches until a migration exists.

## Another instance

Another app's relay runs beside Momotus's from the same files, under its own name: its own service user, database, journal, identity file, port, units and backups, so a deploy of one never touches the other. Its instance file lives in that app's repository (nothing secret in it) and names `NAME`, `TITLE`, `HOST`, `PORT`, `PREFLIGHT_PORT` (both free on the server), `FLAGS` (for example `--escrow --quota-mib 200`) and `CREDENTIALS` (files in `/etc/<NAME>/` the flags read besides `identity.json`, often none).

```sh
ssh "$MQ_RELAY_SSH" 'sudo useradd --system --no-create-home --shell /usr/sbin/nologin <NAME>'
# With --escrow the file also gets an escrow key; the owner copies it into his password manager.
ssh "$MQ_RELAY_SSH" 'sudo python3 -I - create <NAME> <HOST> --escrow' < deploy/relay/identity.py
# Flags that must stay out of every repository, as one root-only line (optional):
ssh "$MQ_RELAY_SSH" 'sudo install -m 0600 /dev/stdin /etc/<NAME>/relay.env' <<< 'RELAY_FLAGS=--invite <code>'
MQ_RELAY_SSH=... deploy/relay/deploy.sh <path to the instance file>
```

`--escrow` makes the relay keep one recovery kit secret per account (`crates/mq-sync-server/src/escrow.rs`), so its operator can open every account: the app's privacy page must say so. The escrow key in the identity file opens every kit in the database and in every backup of it; it never leaves the server and the password manager.

## Rekey

For the day the escrow key, or a copy of the database, may have leaked.

What rekey does: every escrowed kit is sealed again under a new escrow key, and the escrow generation goes up. What it cannot do by itself: the kit secrets stay the same, and whoever holds the old key and any copy of the database (the server's 14 backups and the Mac's 7 included) holds them. A kit is a roster member: every key rotation is sealed to it, so its secret opens the account until the kit leaves the roster. That is why the generation goes up. Each account's devices see their escrowed kit as stale on their next escrow check (`mq_sync::escrow::ensure`, which the app runs after its sync rounds). They then make a new kit, put it in escrow, and remove the old kit, which rotates the account key. An account none of whose devices syncs again keeps its old kit until one does: say so to anyone asking after a leak.

The relay must be stopped (`rekey` refuses a database another process holds open). The command runs as the service user, the identity files coming in as credentials, as the preflight does:

```sh
ssh "$MQ_RELAY_SSH" 'sudo python3 -I /opt/<NAME>/identity.py escrow-key <NAME>'   # writes identity.json.next
ssh "$MQ_RELAY_SSH" 'sudo systemctl stop <NAME>-relay && sudo systemd-run --wait --pipe --collect -p User=<NAME> -p Group=<NAME> \
  -p LoadCredential=from.json:/etc/<NAME>/identity.json -p LoadCredential=to.json:/etc/<NAME>/identity.json.next \
  -p ReadWritePaths=/var/lib/<NAME> sh -c '"'"'exec /opt/<NAME>/bin/mq-sync-server rekey --db /var/lib/<NAME>/relay.sqlite \
  --identity "$CREDENTIALS_DIRECTORY/from.json" --to "$CREDENTIALS_DIRECTORY/to.json"'"'"' \
  && sudo mv /etc/<NAME>/identity.json /etc/<NAME>/identity.json.prev && sudo mv /etc/<NAME>/identity.json.next /etc/<NAME>/identity.json'
ssh "$MQ_RELAY_SSH" 'sudo systemctl start <NAME>-relay'
```

The second line swaps the files only when `rekey` printed `escrow_rekeyed`. The owner replaces the key in his password manager, then `identity.json.prev` is deleted. `rekey` refuses a database whose escrow key is not the `--identity` file's, and a relay started on the old file after it refuses to start. `identity.py escrow-key` prints the new key's fingerprint in the relay's form, the one the database's `meta` table keeps as `escrow_key`.

## Cloudflare

1. Read and save the tunnel's config (`cf tunnels config get <tunnel id>`) outside the repository; list the zone's DNS records; read the zone's security settings.
2. `cf tunnels config update` with the read config plus `{"hostname": "momotus.normalisdead.mom", "service": "http://127.0.0.1:8787"}` first; the catch-all stays last.
3. A proxied `CNAME momotus` to `<tunnel id>.cfargotunnel.com`.
4. The link host:

```sh
RELAY_URL=https://momotus.normalisdead.mom PROVIDERS=google,facebook \
ANDROID_PACKAGES=app.momotus.android,app.momotus.android.walk,app.marquee.android \
ANDROID_SIGNING_SHA256=<the signing certificate's SHA-256> bun apps/linkhost/build.ts
# wrangler, not cf deploy: cf deploy ignores wrangler.jsonc and makes a new project
(cd apps/linkhost && bunx wrangler@4.45.0 deploy --dry-run && bunx wrangler@4.45.0 deploy)
```

`app.marquee.android` is the Android app from before the Momotus ids (an install from before 2026-10-26): it stays in the list, so its passkeys and `/pair` links keep working, until no device runs it. The relay also still takes its sign-in return, `marquee://auth`, beside `momotus://auth`.

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

A walk build (`-Pmarquee.applicationIdSuffix=.walk`) installs beside the owner's app; its home-screen shortcut opens the walk build itself (`shortcut_package`, set per variant in `app/build.gradle.kts`).

The relay runs with `--trust-proxy`: a budgeted call to it on the server itself (a `curl` to `127.0.0.1:8787/v1/pairings`) needs a `cf-connecting-ip` header, else it answers 400 `noClientAddress`; `/v1/relay` is not budgeted.

Release note for the per-addon sync: a profile whose addon list is older than the recommended-addons step gets the newer recommended addons, on, on each device after its first pull, and they then sync; turning one off sticks on every device.

The web app signs in only from `http://127.0.0.1:<port>`: the relay sends brokered returns only to loopback, the app scheme and the link host's `/delete`.

## Backups on the Mac

```sh
NAME=momotus HEALTH=https://momotus.normalisdead.mom/v1/relay   # another instance: its NAME and public host
mkdir -p ~/.local/share/$NAME
cp deploy/relay/pull-backup.sh ~/.local/share/$NAME/pull-backup.sh
sed -e "s|__SCRIPT__|$HOME/.local/share/$NAME/pull-backup.sh|" -e "s|__SSH__|$MQ_RELAY_SSH|" \
  -e "s|__NAME__|$NAME|" -e "s|__HEALTH__|$HEALTH|" \
  deploy/relay/app.momotus.relay-backup.plist > ~/Library/LaunchAgents/app.$NAME.relay-backup.plist
launchctl bootstrap "gui/$(id -u)" ~/Library/LaunchAgents/app.$NAME.relay-backup.plist
```

Each instance pulls its own backups (`/var/lib/<NAME>/backups`) into its own folder (`~/.local/share/<NAME>/relay-backups`), with the same 48-hour notification.

A Mac that installed it before the rename runs it as `app.marquee.relay-backup`: `launchctl bootout "gui/$(id -u)/app.marquee.relay-backup"` and delete `~/Library/LaunchAgents/app.marquee.relay-backup.plist` first, or both pull each day.

The pull reads the server's backups with `sudo -n`. The narrowest rule for the account it uses is one sudoers line, `<user> ALL=(root) NOPASSWD: /usr/bin/ls -1 /var/lib/momotus/backups, /usr/bin/cat /var/lib/momotus/backups/*`, added by the owner if he wants the pull to run without full sudo.

It writes `~/.local/share/momotus/relay-backups/pull.log` and shows a notification when the last good pull or the public health check is more than 48 hours old. The folder is excluded from Time Machine.

## Restore

Momotus's instance below; another one has its own `<NAME>` in every path and unit name. An escrow instance (`--escrow`) needs its identity file restored too: without it the restored database opens no escrowed kit, and the relay refuses to start on another escrow key. After the restore, a signed-in device of each account checks its escrow (`escrow::ensure`): a kit the backup holds but the replayed roster dropped, or one put after the backup, shows as gone, and the device puts a new one. For the hour after the restart (`--new-generation`), a replay may drop the old escrowed kit (that history passed the rule once); after it, nothing a device sends may. Each such drop is logged as `escrow_dropped` with the account id (`journalctl -u <NAME>-relay | grep escrow_dropped`): the drill checks it, and one nobody expects is worth a look.

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
