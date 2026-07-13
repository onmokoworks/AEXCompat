#!/usr/bin/env python3
"""Compare safe PPM fixtures without rendering or invoking native code."""
from __future__ import annotations
import argparse, json, sys
from pathlib import Path
try:
    from tools.ppm_fixture_tool import PpmImage, read_ppm
except ModuleNotFoundError:
    from ppm_fixture_tool import PpmImage, read_ppm

LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
TESTS_ROOT = LAB_ROOT / "tests"
OUTPUT_ROOT = TARGET_ROOT / "compat-oracle"

def safe_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm": raise ValueError("input must be PPM")
    resolved = path.resolve(strict=True)
    if not any(resolved.is_relative_to(root.resolve(strict=True)) for root in (TARGET_ROOT, TESTS_ROOT)):
        raise ValueError("input must stay under target or tests")
    return resolved

def compare_images(reference: PpmImage, candidate: PpmImage, tolerance: int) -> dict:
    if not 0 <= tolerance <= 255: raise ValueError("tolerance must be 0..255")
    dimensions_match = (reference.width, reference.height) == (candidate.width, candidate.height)
    if not dimensions_match:
        state, maximum, mean, exceeding = "nonmatching", None, None, None
    else:
        deltas = [abs(a-b) for a,b in zip(reference.pixels, candidate.pixels)]
        maximum = max(deltas, default=0)
        mean = sum(deltas) / len(deltas) if deltas else 0.0
        exceeding = sum(any(deltas[i+c] > tolerance for c in range(3)) for i in range(0, len(deltas), 3))
        state = "identical" if maximum == 0 else ("within_tolerance" if exceeding == 0 else "nonmatching")
    return {"schema_version":1,"report_kind":"compat_oracle","match_state":state,
            "dimensions_match":dimensions_match,"reference_width":reference.width,"reference_height":reference.height,
            "candidate_width":candidate.width,"candidate_height":candidate.height,"max_channel_delta":maximum,
            "mean_channel_delta":mean,"exceeding_pixel_count":exceeding,"tolerance":tolerance,
            "render_performed":False,"ae_invoked":False,"native_load_performed":False,"pixel_values_serialized":False}

def output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json": raise ValueError("output must be JSON")
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved=(path if path.is_absolute() else LAB_ROOT/path).resolve(strict=False)
    if not resolved.is_relative_to(OUTPUT_ROOT.resolve(strict=True)): raise ValueError("output outside oracle root")
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if resolved.exists(): raise FileExistsError("refusing overwrite")
    return resolved

def main(argv=None):
    p=argparse.ArgumentParser(description=__doc__); p.add_argument("--reference",required=True,type=Path); p.add_argument("--candidate",required=True,type=Path); p.add_argument("--tolerance",type=int,default=0); p.add_argument("--out",required=True,type=Path); a=p.parse_args(argv)
    try:
        out=output_path(a.out); report=compare_images(read_ppm(safe_ppm(a.reference)),read_ppm(safe_ppm(a.candidate)),a.tolerance)
        out.write_text(json.dumps(report,indent=2,sort_keys=True)+"\n",encoding="utf-8")
    except (OSError,ValueError) as exc: print(f"aex_compat_oracle: {type(exc).__name__}",file=sys.stderr); return 2
    print(json.dumps(report,indent=2)); return 0 if report["match_state"] != "nonmatching" else 1
if __name__ == "__main__": raise SystemExit(main())
