#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "[integration] cargo test"
cargo test --quiet

echo "[integration] run demo in pseudo-tty"
LOG_FILE="$(mktemp)"
trap 'rm -f "$LOG_FILE"' EXIT

python3 - "$LOG_FILE" <<'PY'
import os
import pty
import select
import subprocess
import sys
import time

log_path = sys.argv[1]
env = os.environ.copy()
env["EGUI_TERM_AUTOTEST_FRAMES"] = env.get("EGUI_TERM_AUTOTEST_FRAMES", "4")

master_fd, slave_fd = pty.openpty()
proc = subprocess.Popen(
    ["cargo", "run", "--quiet", "--example", "demo"],
    stdin=slave_fd,
    stdout=slave_fd,
    stderr=slave_fd,
    env=env,
)
os.close(slave_fd)

buf = bytearray()
timeout_sec = 25.0
start = time.monotonic()

try:
    while True:
        if time.monotonic() - start > timeout_sec:
            proc.kill()
            proc.wait(timeout=2)
            with open(log_path, "wb") as f:
                f.write(buf)
            print("demo timeout", file=sys.stderr)
            sys.exit(124)

        rlist, _, _ = select.select([master_fd], [], [], 0.1)
        if rlist:
            try:
                data = os.read(master_fd, 65536)
            except OSError:
                data = b""
            if data:
                buf.extend(data)

        if proc.poll() is not None:
            try:
                while True:
                    data = os.read(master_fd, 65536)
                    if not data:
                        break
                    buf.extend(data)
            except OSError:
                pass
            break
finally:
    os.close(master_fd)

with open(log_path, "wb") as f:
    f.write(buf)

sys.exit(proc.returncode)
PY

if ! LC_ALL=C grep -aq $'\x1b_Ga=T' "$LOG_FILE"; then
  echo "[integration] error: kitty graphics sequence not found" >&2
  exit 1
fi

echo "[integration] ok"
