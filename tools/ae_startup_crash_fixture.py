#!/usr/bin/env python3
"""Controlled process fixture for the AE startup crash probe.

This is intentionally not an After Effects replacement. Probe bundles label
its output as ``controlled_fixture`` and never publish it as real-AE evidence.
"""

from __future__ import annotations

import argparse
import ctypes
import os
import time


def load_target() -> None:
    target = os.environ.get("AEXCOMPAT_PROBE_AEX")
    if not target or os.name != "nt":
        return
    ctypes.WinDLL(target)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("normal", "no-load", "exit", "timeout", "dialog", "crash"), required=True)
    parser.add_argument("--seconds", type=float, default=30.0)
    args = parser.parse_args()

    if args.mode not in {"no-load", "exit"}:
        load_target()
    if args.mode == "normal":
        time.sleep(0.25)
        return 0
    if args.mode == "no-load":
        return 0
    if args.mode == "exit":
        return 12
    if args.mode in {"timeout", "dialog"}:
        time.sleep(args.seconds)
        return 0
    if os.name != "nt":
        return 134
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.RaiseException(0xE0000418, 0, 0, None)
    return 134


if __name__ == "__main__":
    raise SystemExit(main())
