# Vendor data

Three tables from the public IEEE Registration Authority registry, all fetched on
**2026-10-16** and reduced to `<hex prefix><TAB><organization>` by `tools/gen_oui.py`:

| File | Registry | Prefix | Source URL | Entries |
|---|---|---|---|---|
| `oui.tsv` | MA-L | 24 bit (6 hex digits) | <https://standards-oui.ieee.org/oui/oui.csv> | 40 226 |
| `oui28.tsv` | MA-M | 28 bit (7 hex digits) | <https://standards-oui.ieee.org/oui28/mam.csv> | 6 637 |
| `oui36.tsv` | MA-S | 36 bit (9 hex digits) | <https://standards-oui.ieee.org/oui36/oui36.csv> | 7 234 |

Lookup is longest prefix first (36, 28, then 24 bit), because MA-M and MA-S blocks are carved
out of MA-L prefixes whose own entry only names the registration authority.

Rules applied by the generator:

* Rows registered as "Private" are omitted from `oui.tsv`. In `oui28.tsv` and `oui36.tsv` they
  are kept with an **empty** name, so a private block shows no vendor instead of falling back
  to the parent's "IEEE Registration Authority".
* A few old MA-L prefixes were registered to several organizations; they appear once with the
  names joined by ` / `.
* Addresses, tabs and other columns are dropped; only prefix and organization name remain.

## Refreshing

```sh
UA="Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0"
mkdir /tmp/ieee && cd /tmp/ieee
for f in oui/oui.csv oui28/mam.csv oui36/oui36.csv; do
  curl -sSL -A "$UA" -o "$(basename $f)" "https://standards-oui.ieee.org/$f"
done
python3 tools/gen_oui.py /tmp/ieee crates/openspot-wlan/data
```

The IEEE server answers 418 to clients without a browser-like `User-Agent`. Update the fetch
date and counts above, then run `cargo test -p openspot-wlan vendor`. The IEEE publishes this
data for public use; the assignments themselves are facts, not creative work.
