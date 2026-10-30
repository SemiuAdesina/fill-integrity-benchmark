# Synthetic CXL decoder and routing model

The input describes host address windows, memory devices, observed address
translations (including negative observations), cross-host coherent DMA
observations, and legal routes. A decoder is `(host, [start,end), device,
device_offset)`. It translates `a` to `device_offset + a - start` exactly when
`start <= a < end`. Its start and end are multiples of that host's granularity;
its device offset is also a multiple of that granularity. The translated
half-open device interval must fit in `[0,capacity)`. Decoders on one host may
touch but must not overlap. Different hosts may expose the same bytes at
different addresses.

Every positive and negative address probe is authoritative. A positive probe
must translate to exactly its named device and offset; a negative probe must be
unmapped. A DMA observation is coherent exactly when both half-open byte ranges
translate to the same device interval. Its `length` must be nonzero and neither
range may cross a decoder or device-capacity endpoint. Candidate maps must
predict every observation exactly.

Among valid maps choose the fewest decoders. Adjacent decoders with the same
host/device translation delta are one canonical decoder and must be coalesced.
Then choose the minimum aggregate route cost: exactly one legal route is chosen
for every `(host,device)` used by a decoder. If costs tie, compare hop sequences
lexicographically. Finally compare the serialized decoders lexicographically by
host, start, end, device, then device offset. Output decoders use that order and
routes use host, device, then hop-sequence order.

All values and rules are supplied here; there are no inferred secret constants.
The bounded fixtures keep every host window to at most twelve granularity units.

## Public semantic test groups

The verifier exercises the public model in independently attributable groups:

- maximal aligned half-open decoder recovery, including negative observations;
- legal touching ranges versus forbidden overlap and endpoint exclusion;
- deterministic route selection by cost and hop-sequence tie-breaking;
- distinct host views over shared device bytes and host-scoped translation;
- full half-open DMA range interpretation, including zero length and endpoints;
- capacity limits, malformed-window rejection, and missing-route failure;
- canonical decoder ordering and global canonical-choice tie-breaking; and
- a compound case combining multiple host views, adjacent boundaries, capacity,
  DMA coherence, and a route tie; and
- a compound case in which a coherent DMA span forces decoder coverage that
  cannot stay inside a device-capacity endpoint, so no valid map exists.

These are behavioral categories, not fixture values or expected canonical
outputs. The model and ordering rules in this specification remain normative.
