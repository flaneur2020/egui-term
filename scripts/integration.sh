#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "[integration] cargo test"
cargo test --quiet

echo "[integration] run demo in pseudo-tty"
LOG_FILE="$(mktemp)"
INTERACTION_LOG_FILE="$(mktemp)"
trap 'rm -f "$LOG_FILE" "$INTERACTION_LOG_FILE"' EXIT

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

echo "[integration] run interaction check (click button)"
python3 - "$INTERACTION_LOG_FILE" <<'PY'
import os
import pty
import select
import subprocess
import sys
import time

log_path = sys.argv[1]
env = os.environ.copy()
env["EGUI_TERM_AUTOTEST_FRAMES"] = env.get("EGUI_TERM_AUTOTEST_FRAMES", "300")
env["EGUI_TERM_AUTOTEST_EXIT_ON_CLICK"] = "1"
env["EGUI_TERM_AUTOTEST_LOG_CLICKS"] = "1"

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
start = time.monotonic()
timeout_sec = 25.0

CLICK_SENTINEL = b"[[AUTOTEST_CLICK=1]]"

def send_mouse_down(col: int, row: int) -> None:
    os.write(master_fd, f"\x1b[<0;{col};{row}M".encode())

def send_mouse_up(col: int, row: int) -> None:
    os.write(master_fd, f"\x1b[<0;{col};{row}m".encode())

def send_mouse_click(col: int, row: int, hold_sec: float = 0.12) -> None:
    send_mouse_down(col, row)
    time.sleep(hold_sec)
    send_mouse_up(col, row)

def replay_click_activity() -> None:
    # Replay a robust variant of recorded manual activity:
    # frequent clicks around (3,3)/(6,3) with occasional row+1 probing.
    #
    # In this pseudo-tty harness crossterm reports mouse col/row one-less than
    # encoded SGR values, so these are intentionally shifted by +1.
    attempts = [
        (4, 4),  # -> (3,3)
        (7, 4),  # -> (6,3)
        (8, 4),  # -> (7,3)
        (7, 5),  # -> (6,4)
        (6, 4),  # -> (5,3)
    ]
    for (col, row) in attempts:
        send_mouse_click(col, row)
        time.sleep(0.1)

try:
    sent = False
    while True:
        now = time.monotonic()
        if now - start > timeout_sec:
            proc.kill()
            proc.wait(timeout=2)
            with open(log_path, "wb") as f:
                f.write(buf)
            print("interaction timeout", file=sys.stderr)
            sys.exit(124)

        if not sent and now - start > 0.8:
            replay_click_activity()
            sent = True

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

if ! LC_ALL=C grep -aq "\\[\\[AUTOTEST_CLICK=1\\]\\]" "$INTERACTION_LOG_FILE"; then
  echo "[integration] error: interaction check failed, no [[AUTOTEST_CLICK=1]] found" >&2
  exit 1
fi

echo "[integration] interaction ok"
