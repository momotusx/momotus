"""The relay's nightly backup (plans/accounts-social-sync.md H9).

    python3 -I backup.py DATA_DIR [--keep 14]   back up DATA_DIR/relay.sqlite and its roster journal
    python3 -I backup.py --self-test            check this file's own rules on a scratch database

Each run writes one set into DATA_DIR/backups: <stamp>.sqlite (SQLite's online backup API, so
pages still in the WAL are in it), <stamp>.roster.jsonl (copied after the database, so it holds at
least every entry the database does) and <stamp>.SHA256SUMS, written last: a set without its sums
is unfinished. Every file is written under a temporary name and renamed. The journal is
read after the database, so it holds every entry the database does, and is cut at its last
whole line (the relay may be writing one). Standard library only:
the server has python3 and no sqlite3 tool.
"""

from __future__ import annotations

import hashlib
import os
import sqlite3
import sys
import tempfile
import time
from contextlib import closing
from pathlib import Path

DB = "relay.sqlite"
JOURNAL = "relay.roster.jsonl"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def check_integrity(path: Path) -> None:
    """Raises unless SQLite finds the copy whole."""
    try:
        with closing(sqlite3.connect(path)) as conn:
            result = conn.execute("PRAGMA integrity_check").fetchone()[0]
    except sqlite3.DatabaseError as e:
        raise RuntimeError(f"{path.name}: {e}") from e
    if result != "ok":
        raise RuntimeError(f"{path.name}: integrity check says {result}")


def back_up(data: Path, keep: int = 14) -> str:
    out = data / "backups"
    out.mkdir(mode=0o700, exist_ok=True)
    stamp = time.strftime("%Y-%m-%dT%H%M%SZ", time.gmtime())
    tmp_db = out / f".{stamp}.sqlite.tmp"
    tmp_journal = out / f".{stamp}.roster.jsonl.tmp"
    try:
        # closing(): a sqlite3 connection's own `with` commits but never closes, and an open
        # copy keeps its -wal and -shm files beside the temporary name after the rename.
        with closing(sqlite3.connect(data / DB)) as src, closing(sqlite3.connect(tmp_db)) as dst:
            src.backup(dst)
        check_integrity(tmp_db)
        # The relay appends while this runs: a line it is still writing is
        # left for the next backup, so the copy ends on a whole line.
        journal = (data / JOURNAL).read_bytes()
        tmp_journal.write_bytes(journal[: journal.rfind(b"\n") + 1])
        os.replace(tmp_db, out / f"{stamp}.sqlite")
        os.replace(tmp_journal, out / f"{stamp}.roster.jsonl")
    finally:
        tmp_db.unlink(missing_ok=True)
        tmp_journal.unlink(missing_ok=True)
    sums = "".join(f"{sha256(out / name)}  {name}\n" for name in (f"{stamp}.sqlite", f"{stamp}.roster.jsonl"))
    tmp_sums = out / f".{stamp}.SHA256SUMS.tmp"
    tmp_sums.write_text(sums)
    os.replace(tmp_sums, out / f"{stamp}.SHA256SUMS")
    prune(out, keep)
    return stamp


def prune(out: Path, keep: int) -> None:
    """Keeps the newest `keep` finished sets; anything else (older sets, a set a crash left without
    its sums) goes. ponytail: two runs at once could prune each other's half-written set; the
    nightly timer and a deploy's backup are minutes long apart, so one flock if that ever changes."""
    finished = sorted((p.name.split(".")[0] for p in out.glob("*.SHA256SUMS")), reverse=True)[:keep]
    for path in out.iterdir():
        if not path.name.startswith(".") and path.name.split(".")[0] not in finished:
            path.unlink()


def self_test() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        data = Path(tmp)
        # A relay mid-write: rows still in the WAL, never checkpointed into the main file.
        live = sqlite3.connect(data / DB)
        live.execute("PRAGMA journal_mode = WAL")
        live.execute("PRAGMA wal_autocheckpoint = 0")
        live.execute("CREATE TABLE records (id INTEGER PRIMARY KEY, body BLOB)")
        live.commit()
        for i in range(500):
            live.execute("INSERT INTO records (body) VALUES (?)", (os.urandom(64),))
        live.commit()
        (data / JOURNAL).write_text('{"account":"a","entry":{"body":"b"}}\n{"account":"a","ent')

        stamp = back_up(data, keep=2)
        copy = data / "backups" / f"{stamp}.sqlite"
        with sqlite3.connect(copy) as conn:
            rows = conn.execute("SELECT COUNT(*) FROM records").fetchone()[0]
        assert rows == 500, f"the backup holds {rows} of 500 rows: pages in the WAL were left out"
        sums = (data / "backups" / f"{stamp}.SHA256SUMS").read_text().splitlines()
        for line in sums:
            digest, name = line.split("  ")
            assert sha256(data / "backups" / name) == digest, f"{name} does not match its sum"
        assert (data / "backups" / f"{stamp}.roster.jsonl").read_text() == '{"account":"a","entry":{"body":"b"}}\n', "a torn last line is left out"

        # A damaged copy is refused.
        bad = data / "bad.sqlite"
        bad.write_bytes(copy.read_bytes()[:4096] + os.urandom(8192))
        try:
            check_integrity(bad)
        except RuntimeError:
            pass
        else:
            raise AssertionError("a damaged copy passed the integrity check")

        # Three runs a second apart keep the newest two sets.
        for _ in range(2):
            time.sleep(1.1)
            back_up(data, keep=2)
        sets = sorted(p.name for p in (data / "backups").glob("*.SHA256SUMS"))
        assert len(sets) == 2 and stamp + ".SHA256SUMS" not in sets, f"pruning kept {sets}"
        # prune() never removes a dot file, so a temporary one left behind stays for good.
        left = [p.name for p in (data / "backups").iterdir() if p.name.startswith(".")]
        assert not left, f"temporary files left behind: {left}"
        live.close()
    print("backup.py self-test: ok")


def main(argv: list[str]) -> int:
    if argv[1:] == ["--self-test"]:
        self_test()
        return 0
    if len(argv) not in (2, 4) or (len(argv) == 4 and argv[2] != "--keep"):
        print(__doc__, file=sys.stderr)
        return 2
    keep = int(argv[3]) if len(argv) == 4 else 14
    print(f"backup {back_up(Path(argv[1]), keep)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
