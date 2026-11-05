#!/usr/bin/env python3
"""Print a Markdown table comparing Criterion results: Rango vs sqlx raw, SeaORM, Diesel.

Usage: python3 benchmarks/compare.py [target/criterion dir]
"""
import json
import pathlib
import sys

LIBS = ["sqlx_raw", "rango", "seaorm", "diesel_async"]
root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "benchmarks/target/criterion")

results = {}  # bench name -> lib -> mean ns
for est in root.glob("**/new/estimates.json"):
    meta = json.loads((est.parent / "benchmark.json").read_text())
    lib, _, bench = meta["full_id"].partition("/")
    if lib in LIBS:
        results.setdefault(bench, {})[lib] = json.loads(est.read_text())["mean"]["point_estimate"]


def fmt(ns):
    for unit, div in (("s", 1e9), ("ms", 1e6), ("µs", 1e3)):
        if ns >= div:
            return f"{ns / div:.2f} {unit}"
    return f"{ns:.0f} ns"


print("| Benchmark | " + " | ".join(LIBS) + " | Rango vs best ORM |")
print("|---" * (len(LIBS) + 2) + "|")
for bench in sorted(results):
    row = results[bench]
    cells = [fmt(row[lib]) if lib in row else "—" for lib in LIBS]
    orms = [row[lib] for lib in ("seaorm", "diesel_async") if lib in row]
    delta = f"{(row['rango'] / min(orms) - 1) * 100:+.0f}%" if "rango" in row and orms else "—"
    print(f"| `{bench}` | " + " | ".join(cells) + f" | {delta} |")
