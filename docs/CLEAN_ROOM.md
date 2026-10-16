# Independent-implementation rules for contributors

OpenSpot is an independent implementation of ideas that are common to Wi-Fi analyzers and site-survey
tools, built from public specifications and public operating-system APIs.

1. **Only contribute your own work.** Do not submit code, assets, icons, sounds, strings, catalogs or
   file formats copied or derived from any other product.
2. **Specs are the source of truth**: IEEE 802.11 (information elements), Microsoft WLAN API docs,
   `nl80211` docs, CoreWLAN docs, iperf3 docs, ITU-R P.1238 and P.2040, COST 231.
3. **Describe behaviour, not code.** Write tests from observed inputs and outputs and public
   documentation, never from decompiling or disassembling another product.
4. **No third-party names or logos in the product.** Factual comparative statements in the README
   (for example "an open-source alternative to commercial Wi-Fi survey tools") are fine; do not imply
   affiliation.
5. **No licensing bypass or activation code**, ever.
6. Note the source (paper or spec) of any non-trivial algorithm in code comments or `docs/`.
7. Test data must be synthetic. Never commit real SSIDs or BSSIDs of other people's networks.

This is not legal advice.
