"""Error and crash reports for the nightly check (plans/error-reports.md section 9).

On the server, as the service user, read-only (deploy.sh installs this file into /opt/momotus):
    sudo -n -u momotus /usr/bin/python3 -I /opt/momotus/reports.py summary SINCE_MS
        one JSON line with the server's totals, then one per report group received since SINCE_MS

On the Mac, in the nightly check:
    MQ_RELAY_SSH=<ssh target> uv run python -I deploy/relay/reports.py nightly
        writes ~/.local/share/momotus/reports/<date>.md: new groups first, then groups that more than
        doubled, then the rest, walk and debug builds apart. A night with nothing new still writes the
        file with its counts; a failed SSH call writes a file that says so and exits 1.

    python3 -I reports.py --self-test     this file's own rules, on a scratch folder

Reports carry no identifier; `batches` (distinct numbers the relay drew per request) bounds how many
devices a group touched. Notes that go into plans/ cite only grp, kind, where and counts, never a
message or a body. Standard library only.
"""

from __future__ import annotations

import datetime as dt
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path

DB = Path("/var/lib/momotus/reports.sqlite")
REMOTE = "sudo -n -u momotus /usr/bin/python3 -I /opt/momotus/reports.py summary {since}"
HOME = Path.home() / ".local/share/momotus"
OUT = HOME / "reports"
SYMBOLS = HOME / "symbols"
DAY_MS = 24 * 60 * 60 * 1000
HOUR_MS = 60 * 60 * 1000
KEEP_DAYS = 30


def summary(db: Path, since_ms: int) -> list[dict]:
    """The server's totals, then each group received at or after `since_ms` (by receive hour)."""
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        total = conn.execute("SELECT COUNT(*) FROM report").fetchone()[0]
        since_hour = since_ms // HOUR_MS
        rows = conn.execute(
            """SELECT grp, kind, group_concat(DISTINCT app), group_concat(DISTINCT version),
                      MIN(day * 24 + hour), MAX(day * 24 + hour), SUM(count), COUNT(*),
                      COUNT(DISTINCT batch), (SELECT body FROM report r2 WHERE r2.grp = r.grp ORDER BY id DESC LIMIT 1)
               FROM report r WHERE day * 24 + hour >= ? GROUP BY grp, kind ORDER BY SUM(count) DESC""",
            (since_hour,),
        ).fetchall()
    finally:
        conn.close()
    out: list[dict] = [{"server": {"rows": total}}]
    for grp, kind, apps, versions, first, last, count, reports, batches, body in rows:
        out.append(
            {
                "grp": grp,
                "kind": kind,
                "apps": sorted((apps or "").split(",")),
                "versions": sorted((versions or "").split(",")),
                "first": first * HOUR_MS,
                "last": last * HOUR_MS,
                "count": count,
                "reports": reports,
                "batches": batches,
                "body": json.loads(body),
            }
        )
    return out


def ssh_summary(since_ms: int) -> list[dict]:
    target = os.environ.get("MQ_RELAY_SSH")
    if not target:
        raise RuntimeError("MQ_RELAY_SSH is not set")
    done = subprocess.run(
        ["ssh", target, REMOTE.format(since=int(since_ms))], capture_output=True, text=True, timeout=120, check=False
    )
    if done.returncode != 0:
        raise RuntimeError(f"ssh exited {done.returncode}: {done.stderr.strip()[:300]}")
    return [json.loads(line) for line in done.stdout.splitlines() if line.strip()]


def retrace(lines: list[str], version: str) -> list[str]:
    """Android frames through R8's retrace with the kept mapping, when both are there."""
    tool = shutil.which("retrace")
    maps = sorted(SYMBOLS.glob(f"android-{version}-*/mapping.txt"))
    if not tool or not maps or not lines:
        return lines
    done = subprocess.run([tool, str(maps[-1])], input="\n".join(lines), capture_output=True, text=True, check=False)
    return done.stdout.splitlines() if done.returncode == 0 and done.stdout else lines


def atos(lines: list[str], app: str, version: str) -> list[str]:
    """Apple frames (`Momotus 0x1a2b`, an offset into the binary's text) through atos with the kept dSYM."""
    dsyms = sorted(SYMBOLS.glob(f"{app}-{version}-*/*.dSYM"))
    if not shutil.which("atos") or not dsyms:
        return lines
    dwarf = next((dsyms[-1] / "Contents/Resources/DWARF").glob("*"), None)
    if dwarf is None:
        return lines
    out = []
    for line in lines:
        parts = line.split()
        if len(parts) == 2 and parts[0] == dwarf.name and parts[1].startswith("0x"):
            address = hex(0x100000000 + int(parts[1], 16))
            done = subprocess.run(
                ["atos", "-arch", "arm64", "-o", str(dwarf), "-l", "0x100000000", address],
                capture_output=True,
                text=True,
                check=False,
            )
            out.append(done.stdout.strip() or line)
        else:
            out.append(line)
    return out


def readable(group: dict) -> list[str]:
    report = group["body"].get("report", {})
    device = group["body"].get("device", {})
    stack = report.get("stack", [])
    app, version = device.get("app", ""), device.get("version", "")
    if app == "android":
        stack = retrace(stack, version)
    elif app in ("ios", "macos"):
        stack = atos(stack, app, version)
    return stack


def hour(ms: int) -> str:
    return dt.datetime.fromtimestamp(ms / 1000, dt.UTC).strftime("%Y-%m-%d %H:00Z")


def line(group: dict) -> str:
    report = group["body"].get("report", {})
    place = report.get("where") or (report.get("stack") or [""])[0]
    return (
        f"`{group['grp']}` {group['kind']} x{group['count']} ({group['batches']} batches) "
        f"{'/'.join(group['apps'])} {', '.join(group['versions'])}: {report.get('message', '')} @ {place}"
    )


def render(groups: list[dict], server: dict, seen: set[str], previous: dict[str, int], window: str, ok: str) -> str:
    def channel(g: dict) -> str:
        return g["body"].get("device", {}).get("channel", "release")

    release = [g for g in groups if channel(g) == "release"]
    builds = [g for g in groups if channel(g) != "release"]
    new = [g for g in release if g["grp"] not in seen]
    new.sort(key=lambda g: (-g["count"], -g["batches"]))
    grown = [g for g in release if g["grp"] in seen and g["count"] > 2 * previous.get(g["grp"], g["count"])]
    rest = [g for g in release if g not in new and g not in grown]
    out = [
        f"# Error reports, {window}",
        "",
        f"- SSH: {ok}",
        (
            f"- New groups: {len(new)}; groups this window: {len(release)}; reports this window: "
            f"{sum(g['reports'] for g in groups)}; rows on the server: {server.get('rows', '?')}"
        ),
        "",
        "## New",
        "",
    ]
    for g in new:
        out.append(f"- {line(g)}")
        out.extend(f"    {frame}" for frame in readable(g)[:12])
        log = g["body"].get("report", {}).get("log", [])
        out.extend(f"    log: {entry}" for entry in log[-5:])
    out += ["", "## More than doubled", ""] + [f"- {line(g)}" for g in grown]
    out += ["", "## Known", ""] + [f"- {line(g)}" for g in rest]
    out += ["", "## Walk and debug builds", ""] + [f"- {line(g)}" for g in builds]
    return "\n".join(out) + "\n"


def nightly(fetch: Callable[[int], list[dict]] = ssh_summary, out: Path = OUT, now: dt.datetime | None = None) -> int:
    now = now or dt.datetime.now(dt.UTC)
    out.mkdir(parents=True, exist_ok=True)
    if sys.platform == "darwin":
        subprocess.run(["tmutil", "addexclusion", str(out)], capture_output=True, check=False)
    state_path, seen_path = out / "state.json", out / "seen.json"
    state = json.loads(state_path.read_text()) if state_path.exists() else {}
    seen = set(json.loads(seen_path.read_text())) if seen_path.exists() else set()
    now_ms = int(now.timestamp() * 1000)
    since = int(state.get("since_ms", now_ms - DAY_MS))
    window = f"{hour(since)} to {hour(now_ms)}"
    page = out / f"{now:%Y-%m-%d}.md"
    try:
        lines = fetch(since)
    except Exception as e:  # noqa: BLE001  the file must say what failed, whatever it was
        page.write_text(f"# Error reports, {window}\n\n- SSH: FAILED: {e}\n- Nothing was read; this is not a quiet night.\n")
        print(f"reports: {page} (failed: {e})", file=sys.stderr)
        return 1
    server = next((l["server"] for l in lines if "server" in l), {})
    groups = [l for l in lines if "grp" in l]
    page.write_text(render(groups, server, seen, state.get("counts", {}), window, "ok"))
    seen |= {g["grp"] for g in groups}
    seen_path.write_text(json.dumps(sorted(seen)))
    state_path.write_text(json.dumps({"since_ms": now_ms, "counts": {g["grp"]: g["count"] for g in groups}}))
    cutoff = now - dt.timedelta(days=KEEP_DAYS)
    for old in out.glob("????-??-??.md"):
        try:
            if dt.datetime.strptime(old.stem, "%Y-%m-%d").replace(tzinfo=dt.UTC) < cutoff:
                old.unlink()
        except ValueError:
            continue
    print(f"reports: {page}")
    return 0


def self_test() -> None:
    schema = """CREATE TABLE report (id INTEGER PRIMARY KEY, day INTEGER NOT NULL, hour INTEGER NOT NULL,
        batch INTEGER NOT NULL, app TEXT NOT NULL, version TEXT NOT NULL, kind TEXT NOT NULL, grp TEXT NOT NULL,
        count INTEGER NOT NULL, body TEXT NOT NULL)"""
    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch)
        db = root / "reports.sqlite"
        conn = sqlite3.connect(db)
        conn.execute(schema)
        now = dt.datetime(2026, 11, 8, 3, tzinfo=dt.UTC)
        now_ms = int(now.timestamp() * 1000)
        today, h = now_ms // DAY_MS, (now_ms % DAY_MS) // HOUR_MS

        def body(message: str, channel: str = "release") -> str:
            return json.dumps(
                {"device": {"app": "android", "version": "1.0", "channel": channel}, "report": {"kind": "panic", "message": message, "where": "crates/a.rs:1"}}
            )

        rows = [
            (today, h, 1, "android", "1.0", "panic", "aaaa", 3, body("old")),
            (today, h, 2, "android", "1.0", "panic", "aaaa", 2, body("new one")),
            (today, h, 2, "android", "1.1", "panic", "bbbb", 1, body("b")),
            (today, h, 3, "android", "1.1", "core", "wwww", 1, body("walk", "walk")),
            (today - 2, 0, 4, "android", "1.0", "panic", "cccc", 9, body("too old for the window")),
        ]
        conn.executemany("INSERT INTO report (day, hour, batch, app, version, kind, grp, count, body) VALUES (?,?,?,?,?,?,?,?,?)", rows)
        conn.commit()
        conn.close()
        got = summary(db, now_ms - DAY_MS)
        assert got[0] == {"server": {"rows": 5}}, got[0]
        groups = {g["grp"]: g for g in got[1:]}
        assert set(groups) == {"aaaa", "bbbb", "wwww"}, groups.keys()
        assert (groups["aaaa"]["count"], groups["aaaa"]["reports"], groups["aaaa"]["batches"]) == (5, 2, 2)
        assert groups["aaaa"]["body"]["report"]["message"] == "new one", "the newest body"
        # A first night: everything is new, walk builds apart.
        out = root / "out"
        assert nightly(lambda since: got, out, now) == 0
        page = (out / "2026-11-08.md").read_text()
        new = page.split("## New")[1].split("## More")[0]
        assert "`aaaa`" in new and "`bbbb`" in new and "`wwww`" not in new, page
        assert "`wwww`" in page.split("## Walk and debug builds")[1]
        # The next night: aaaa more than doubled, bbbb is known, nothing new.
        later = now + dt.timedelta(days=1)
        grown = [got[0], dict(groups["aaaa"], count=11), dict(groups["bbbb"])]
        assert nightly(lambda since: grown, out, later) == 0
        page = (out / "2026-11-09.md").read_text()
        assert "New groups: 0" in page
        assert "`aaaa`" in page.split("## More than doubled")[1].split("## Known")[0]
        assert "`bbbb`" in page.split("## Known")[1]
        # A failed SSH call still writes a file that says so, and fails.
        def broken(since: int) -> list[dict]:
            raise RuntimeError("ssh exited 255")

        assert nightly(broken, out, later + dt.timedelta(days=1)) == 1
        assert "FAILED" in (out / "2026-11-10.md").read_text()
        # Summaries past 30 days go.
        (out / "2026-09-01.md").write_text("old")
        assert nightly(lambda since: grown, out, later + dt.timedelta(days=2)) == 0
        assert not (out / "2026-09-01.md").exists()
    print("reports.py self-test: ok")


def main(argv: list[str]) -> int:
    if argv[1:] == ["--self-test"]:
        self_test()
        return 0
    if len(argv) == 3 and argv[1] == "summary":
        for entry in summary(DB, int(argv[2])):
            print(json.dumps(entry))
        return 0
    if argv[1:] == ["nightly"]:
        return nightly()
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
