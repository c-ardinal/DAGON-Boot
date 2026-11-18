"""Drives `dagon ai key` against the OS credential store through a pseudo-terminal, typing the key
as a person would, and checks what it answers. CI only: it registers and removes the claude/api
entry of the store it finds (.github/workflows/keystore.yml).

  python3 scripts/keystore_check.py store <dagon>     a usable store: status, set, status, delete, status
  python3 scripts/keystore_check.py no-store <dagon>  no store: set and delete exit 1, status exits 0

The key typed is an obviously fake value; no output may carry it.
"""
import os
import pty
import select
import signal
import sys
import termios
import time

FAKE = "sk-ci-fake-0000-not-a-real-key"
WAY = "claude/api"
PROMPT = "（表示されません）".encode()
NO_STORE = ("cannot be used (", "DAGON keeps API keys nowhere else")
TIMEOUT_S = 60


def fail(why, out=""):
    print(out.replace(FAKE, "<the fake key>"))
    sys.exit(f"FAIL: {why}")


def run(dagon, *args):
    """(exit code, output, whether the key was typed) of `dagon ai key <args>` on a pty; types FAKE
    once asked, with echo off."""
    pid, fd = pty.fork()
    if pid == 0:
        os.execv(dagon, [dagon, "ai", "key", *args])
    out, typed, deadline = b"", False, time.monotonic() + TIMEOUT_S
    while True:
        if time.monotonic() > deadline:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
            fail(f"dagon ai key {' '.join(args)} did not finish", out.decode(errors="replace"))
        if select.select([fd], [], [], 0.1)[0]:
            try:
                chunk = os.read(fd, 4096)
            except OSError:  # EIO on Linux once the child has gone
                chunk = b""
            if not chunk:
                break
            out += chunk
        # rpassword prints the prompt before it turns echo off; type only after.
        if not typed and PROMPT in out and not termios.tcgetattr(fd)[3] & termios.ECHO:
            os.write(fd, (FAKE + "\n").encode())
            typed = True
    code = os.waitstatus_to_exitcode(os.waitpid(pid, 0)[1])
    os.close(fd)
    text = out.decode(errors="replace").replace("\r\n", "\n")
    if FAKE in text:
        fail("the key showed in the output", text)
    print(f"$ dagon ai key {' '.join(args)}  -> exit {code}\n{text}")
    return code, text, typed


def expect(ok, why, out):
    if not ok:
        fail(why, out)


def state(out):
    """The way's state in `status`: what follows `claude/api (...): `."""
    for line in out.splitlines():
        if line.startswith(WAY + " ("):
            return line.split("): ", 1)[1]
    fail(f"status lists no {WAY}", out)


def with_store(dagon):
    code, out, _ = run(dagon, "status")
    expect(code == 0 and state(out) == "未登録", f"{WAY} is not unregistered before the check", out)
    done = False
    try:
        code, out, _ = run(dagon, "set")
        expect(code == 0 and f"{WAY}: 登録済" in out, "set", out)
        code, out, _ = run(dagon, "status")
        expect(code == 0 and state(out) == "登録済", "status after set", out)
        code, out, _ = run(dagon, "delete")
        expect(code == 0 and f"{WAY}: 削除済み" in out, "delete", out)
        done = True
    finally:
        if not done:
            run(dagon, "delete")
    code, out, _ = run(dagon, "status")
    expect(code == 0 and state(out) == "未登録", "status after delete", out)


def without_store(dagon):
    for cmd in ("set", "delete"):
        code, out, typed = run(dagon, cmd)
        expect(code == 1 and all(s in out for s in NO_STORE), f"{cmd} without a store", out)
        # set refuses before it asks for the key.
        expect(not typed and PROMPT.decode() not in out, f"{cmd} asked for the key without a store", out)
    code, out, _ = run(dagon, "status")
    expect(code == 0 and state(out).startswith("未登録 (") and NO_STORE[0] in state(out), "status without a store", out)


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ("store", "no-store"):
        sys.exit(__doc__)
    if os.environ.get("GITHUB_ACTIONS") != "true":
        sys.exit("refused: this changes the store's claude/api entry; it runs in CI only")
    (with_store if sys.argv[1] == "store" else without_store)(os.path.abspath(sys.argv[2]))
    print("ok")


if __name__ == "__main__":
    main()
