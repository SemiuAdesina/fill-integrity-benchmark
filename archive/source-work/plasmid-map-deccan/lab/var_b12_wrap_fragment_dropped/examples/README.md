# Worked example

`small/` is a real construct at reduced size: the pUC19 backbone (GenBank
L09137.2), a lambda phage (J02459.1) restriction fragment placed at one of two
listed cloning sites, three enzymes, at most one insert-region site per enzyme,
every single digest and two doubles observed and one double held out.
`small/map.json` is the answer the specification requires for it, byte for
byte as the supplied writer emits it. Run the binary on the directory and
compare:

```sh
./target/release/plasmid-map examples/small /tmp/map.json
cmp /tmp/map.json examples/small/map.json
```
