#!/usr/bin/env python3.12
"""Builds the vendor tables in crates/openspot-wlan/data from the public IEEE registry CSVs.

Usage: gen_oui.py <dir with oui.csv, mam.csv, oui36.csv> <output dir>

Inputs (public IEEE Registration Authority downloads):
  https://standards-oui.ieee.org/oui/oui.csv       MA-L, 24-bit assignments
  https://standards-oui.ieee.org/oui28/mam.csv     MA-M, 28-bit assignments
  https://standards-oui.ieee.org/oui36/oui36.csv   MA-S, 36-bit assignments
The server rejects clients without a browser-like User-Agent header.

Outputs, one `<hex prefix><TAB><organization>` line per assignment, sorted by prefix:
  oui.tsv    6 hex digits  (MA-L)
  oui28.tsv  7 hex digits  (MA-M)
  oui36.tsv  9 hex digits  (MA-S)

A prefix registered to several organizations is written once with the names joined by " / ".
Organizations registered as "Private" are left out of oui.tsv. In the two longer tables they
are written with an empty name instead: a private sub-block of an MA-L must stay vendorless
and must not fall back to the "IEEE Registration Authority" entry of its parent OUI.
"""

import csv
import sys
from pathlib import Path

PRIVATE = "Private"

# (input file, registry label, hex digits in the assignment, output file, keep private rows)
TABLES = [
    ("oui.csv", "MA-L", 6, "oui.tsv", False),
    ("mam.csv", "MA-M", 7, "oui28.tsv", True),
    ("oui36.csv", "MA-S", 9, "oui36.tsv", True),
]


def clean(name: str) -> str:
    # Tabs and newlines would break the line format; the registry has none today.
    return " ".join(name.split())


def convert(src: Path, registry: str, digits: int, keep_private: bool) -> list[str]:
    names: dict[str, set[str]] = {}
    with src.open(newline="", encoding="utf-8") as f:
        for row in csv.DictReader(f):
            if row["Registry"] != registry:
                continue
            key = row["Assignment"].strip().upper()
            if len(key) != digits or any(c not in "0123456789ABCDEF" for c in key):
                raise SystemExit(f"{src}: malformed assignment {key!r}")
            name = clean(row["Organization Name"])
            if name == PRIVATE:
                if not keep_private:
                    continue
                name = ""
            elif not name:
                continue
            names.setdefault(key, set()).add(name)
    # A few historical MA-L prefixes were registered to several organizations; list them all
    # rather than pick one, so the displayed vendor is never wrong, only broader.
    return [f"{k}\t{' / '.join(sorted(v - {''}) or [''])}" for k, v in sorted(names.items())]


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    src_dir, out_dir = Path(sys.argv[1]), Path(sys.argv[2])
    for file, registry, digits, out, keep_private in TABLES:
        lines = convert(src_dir / file, registry, digits, keep_private)
        (out_dir / out).write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"{out}: {len(lines)} {registry} entries")


if __name__ == "__main__":
    main()
