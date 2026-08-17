#!/usr/bin/env python3
"""Capture what After Effects' PF_UtilCallbacks.get_callback_addr returns for
private callback ids by hooking the live host with Frida (issue #985).

Launches ``AfterFX.com -m -noui -r tools/ae-private-callback-capture.jsx``,
attaches Frida to the host process (AfterFX.com hosts AfterFXLib.dll itself in
this mode; there is no AfterFX.exe child), loads
``tools/frida/ae_private_callback_probe.js`` with the target plug-in module
names, and only then lets the JSX render one frame per requested effect. Every
event the probe sends is appended to ``events.jsonl`` in the output directory;
binary payloads (world pixels before/after each id -2 call, the synthetic-world
probe results) are written next to it. AE quits from the JSX; the driver never
kills an AE process it did not launch.

The AE installation is an exclusive machine resource: the run refuses to start
while any AfterFX / aerender / aerendercore process exists (another session
owns it - wait, do not kill it).

Example (the run behind docs/PRIVATE_CALLBACK_IDS_OBSERVATION_2026-08-17.md):

    uv run --with frida --with psutil python tools/capture_ae_private_callbacks.py \
        --after-effects "C:/Program Files/Adobe/Adobe After Effects 2026/Support Files/AfterFX.com" \
        --input target/private-callback-input.png --out target/private-callback-capture \
        --effect "ADBE Bulge|Bulge|bulge" \
        --effect "ADBE Compound Blur|Compound Blur|compound_blur|Maximum Blur=7.5" \
        --effect "CC Cross Blur|CC Cross Blur|cc_cross_blur|Radius X=6.3,Radius Y=3.7" \
        --effect "ADBE Matte Choker|Matte Choker|matte_choker|Geometric Softness 1=5.5" \
        --target Bulge.aex --target Compound_Blur.aex --target CrossBlur.aex --target Matte_Choker.aex

``frida`` and ``psutil`` are imported lazily so the module stays importable
without them.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

AE_PROCESS_NAMES = {"afterfx.exe", "afterfx.com", "aerender.exe", "aerendercore.exe"}


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--after-effects", required=True, help="path to AfterFX.com (the console host that runs -r scripts)")
    parser.add_argument("--input", required=True, help="input image imported as the footage")
    parser.add_argument("--out", required=True, help="output directory (created; must not exist)")
    parser.add_argument("--effect", action="append", required=True,
                        help='"matchName|displayName|outName[|Param=value,...]"; repeatable, rendered in order')
    parser.add_argument("--target", action="append", required=True,
                        help="plug-in module file name to hook (e.g. Bulge.aex); repeatable")
    parser.add_argument("--jsx", default=str(Path(__file__).with_name("ae-private-callback-capture.jsx")))
    parser.add_argument("--probe", default=str(Path(__file__).parent / "frida" / "ae_private_callback_probe.js"))
    parser.add_argument("--attach-timeout", type=float, default=120.0, help="seconds to wait for the host process")
    parser.add_argument("--run-timeout", type=float, default=900.0, help="seconds to wait for AE to finish")
    return parser.parse_args(argv)


def running_ae_processes(psutil) -> list[tuple[int, str]]:
    found = []
    for process in psutil.process_iter(["pid", "name"]):
        name = (process.info.get("name") or "").lower()
        if name in AE_PROCESS_NAMES:
            found.append((process.info["pid"], name))
    return found


def probe_source(path: Path, targets: list[str]) -> str:
    source = path.read_text(encoding="utf-8")
    placeholder = "AEXCAP_TARGETS_PLACEHOLDER"
    if placeholder not in source:
        raise SystemExit(f"probe script has no {placeholder}: {path}")
    for target in targets:
        if any(ch in target for ch in "';\\\""):
            raise SystemExit(f"target name contains characters the probe cannot carry: {target!r}")
    return source.replace(placeholder, ";".join(targets), 1)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        import frida  # noqa: PLC0415 (lazy: observation-only dependency)
        import psutil  # noqa: PLC0415
    except ImportError as error:  # pragma: no cover - environment dependent
        raise SystemExit(f"frida and psutil are required for the live capture: {error}")

    ae_com = Path(args.after_effects)
    if not ae_com.is_file():
        raise SystemExit(f"After Effects host not found: {ae_com}")
    input_path = Path(args.input).resolve()
    if not input_path.is_file():
        raise SystemExit(f"input image not found: {input_path}")
    out_dir = Path(args.out).resolve()
    if out_dir.exists():
        raise SystemExit(f"refusing to reuse an existing output directory: {out_dir}")
    for spec in args.effect:
        fields = spec.split("|")
        if len(fields) < 3 or not fields[0] or not fields[1] or not fields[2] or \
                not all(ch.isalnum() or ch in "_-" for ch in fields[2]):
            raise SystemExit(f"--effect must be 'matchName|displayName|outName[|Param=value,...]' "
                             f"with outName in [A-Za-z0-9_-]: {spec!r}")
    running = running_ae_processes(psutil)
    if running:
        raise SystemExit(f"After Effects is already running; refusing to attach: {running}")

    out_dir.mkdir(parents=True)
    go_file = out_dir / "go.flag"
    events_path = out_dir / "events.jsonl"
    source = probe_source(Path(args.probe), args.target)

    env = dict(os.environ)
    env.update({
        "AEXCAP_OUT_DIR": out_dir.as_posix(),
        "AEXCAP_GO_FILE": go_file.as_posix(),
        "AEXCAP_INPUT": input_path.as_posix(),
        "AEXCAP_EFFECTS": ";".join(args.effect),
    })
    launched = subprocess.Popen(
        [str(ae_com), "-m", "-noui", "-r", str(Path(args.jsx).resolve())],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    print(f"[+] launched {ae_com.name} pid {launched.pid}", flush=True)

    # The host is the process this run launched (AfterFX.com loads
    # AfterFXLib.dll itself under -noui -r; observed on 26.3, no AfterFX.exe
    # child). Only that pid is attached and, on timeout, terminated; an AE
    # session another owner starts in the meantime is never touched.
    device = frida.get_local_device()
    target_pid = launched.pid
    deadline = time.time() + args.attach_timeout
    while time.time() < deadline:
        if launched.poll() is not None:
            raise SystemExit(f"the launched host exited early with code {launched.returncode}")
        if any(process.pid == target_pid for process in device.enumerate_processes()):
            break
        time.sleep(0.5)
    else:
        launched.kill()
        raise SystemExit("the launched host never became attachable")
    print(f"[+] AE host pid {target_pid}", flush=True)

    state = {"ready": False, "hooked": 0}
    with events_path.open("w", encoding="utf-8") as events:
        def on_message(message, data):
            if message.get("type") == "send":
                payload = message["payload"]
                kind = payload.get("ev")
                if kind == "m2_probe_case" and data is not None:
                    (out_dir / f"probe_{payload['idx']}.bin").write_bytes(data)
                elif kind == "m2_pixels" and data is not None:
                    (out_dir / f"m2_{payload['idx']}_{payload['which']}.bin").write_bytes(data)
                    return
                events.write(json.dumps(payload) + "\n")
                events.flush()
                if kind == "ready":
                    state["ready"] = True
                if kind == "hooked":
                    state["hooked"] += 1
                if kind in ("hooked", "gca_found", "gca_call", "returned_fn", "m5_probe", "m2_call",
                            "m2_probe_done", "m2_probe_failed", "dispatcher_enum_failed"):
                    print(json.dumps(payload)[:300], flush=True)
            else:
                events.write(json.dumps(message) + "\n")
                events.flush()
                print("[frida]", message, flush=True)

        session = device.attach(target_pid)
        script = session.create_script(source)
        script.on("message", on_message)
        script.load()
        wait_until = time.time() + 60
        while not state["ready"] and time.time() < wait_until:
            time.sleep(0.2)
        if not state["ready"]:
            print("[!] probe never reported ready; releasing the JSX anyway", flush=True)
        go_file.write_text("go", encoding="utf-8")
        print("[+] hooks armed, JSX released", flush=True)

        finish_by = time.time() + args.run_timeout
        while psutil.pid_exists(target_pid) and time.time() < finish_by:
            time.sleep(1.0)
        if psutil.pid_exists(target_pid):
            print("[!] AE still alive after the run timeout; terminating the process this run launched", flush=True)
            try:
                launched.kill()
            except Exception:  # pragma: no cover - best effort
                pass
        try:
            session.detach()
        except Exception:  # pragma: no cover
            pass
    try:
        launched.wait(30)
    except Exception:  # pragma: no cover
        pass
    result = out_dir / "result.json"
    print(f"[+] done: {out_dir}")
    if not result.exists():
        print("no result.json (the JSX did not run to completion)")
        return 1
    print(result.read_text(encoding="utf-8"))
    if '"status":"captured"' not in result.read_text(encoding="utf-8"):
        return 1
    if state["hooked"] == 0:
        # The render ran but no target module was ever hooked in the process we
        # attached to (a host layout where the plug-ins load in a child would
        # look like this): a capture with no observations is a failure.
        print("[!] no target module was hooked in the attached process; nothing was observed")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
