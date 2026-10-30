
# Exact Channel Sounding model

All values are integers. Distance is millimetres, frequency is kilohertz, RTT
is picoseconds, and PBR phase is millidegrees. `C = 299792458`.

`round_ratio(n,d)` rounds a nonnegative rational to nearest integer; an exact
half rounds upward. Propagation phase is
`round_ratio(720000 * frequency_khz * distance_mm, C)`. Add the named antenna
path bias, then wrap into `[-180000,180000)`. Thus `+180000` canonicalizes to
`-180000`. Circular phase error is the absolute canonical wrap of predicted
minus observed phase.

Propagation RTT is `round_ratio(2000000000 * distance_mm, C)`. Add the global
clock bias and the observation's supplied path delay.

Every candidate vector is a finite bounded grid. Its first and last values are
legal. Every named path must have a nonempty bias grid. Duplicate grid values
are ignored.

For a complete candidate compute, in canonical observation order, the score:

1. sum of circular PBR errors;
2. sum of absolute RTT errors;
3. maximum circular PBR error;
4. maximum absolute RTT error.

Minimize that tuple lexicographically. Equal scores are broken by distance,
clock bias, then the full `(path_id,bias)` vector. PBR predictions sort by
`(frequency_khz,path_id,measured_phase_md)`. RTT predictions sort by
`(path_id,path_delay_ps,measured_rtt_ps)`.
