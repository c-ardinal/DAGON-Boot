"""This boot core's own check of a built image, which
scripts/bootcore_size.py runs for every configuration it builds:

    python image_check.py [<elf> <boot folder>]

The boot core runs no embassy executor: the port of
`dagon_mcu_rp2040::boot_receive` polls the links' futures, their
`with_timeout`s, its sleep's `Timer` and USB's `Timer::at` with a waker of
its own. embassy-time's integrated timer queue (its default when no
`generic-queue-*` feature is on) takes the executor's task out of every
waker it schedules and panics on any other waker ("Found waker not created
by the Embassy executor"): every listen window, hand-over and recovery
would reset the node. So the package as resolved for its target must build
embassy-time-queue-utils with a generic queue and without
`integrated-timers`. Prints each finding; exits 1 when there is one.
"""
import json
import pathlib
import subprocess
import sys
import tomllib

PKG = pathlib.Path(__file__).resolve().parent
QUEUE = "embassy-time-queue-utils"


def queue_problems(meta: dict) -> list[str]:
    """What is wrong with the timer queue in `cargo metadata` output."""
    names = {p["id"]: p["name"] for p in meta["packages"]}
    out = []
    for node in meta["resolve"]["nodes"]:
        if names[node["id"]] != QUEUE:
            continue
        features = set(node["features"])
        if "_generic-queue" not in features or "integrated-timers" in features:
            out.append(f"{QUEUE} resolves to the integrated timer queue "
                       f"(features: {', '.join(sorted(features)) or 'none'}); the boot core "
                       "polls without an executor and needs embassy-time/generic-queue-*")
    return out


def metadata() -> dict:
    config = tomllib.loads((PKG / ".cargo" / "config.toml").read_text(encoding="utf-8"))
    r = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked",
         "--filter-platform", config["build"]["target"]],
        cwd=PKG, capture_output=True, text=True, check=True)
    return json.loads(r.stdout)


if __name__ == "__main__":
    problems = queue_problems(metadata())
    print("\n".join(problems))
    sys.exit(1 if problems else 0)
