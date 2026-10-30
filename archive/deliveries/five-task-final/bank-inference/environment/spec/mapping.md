# Mapping semantics

`word = byte_address / word_width` uses integer truncation. Each selected XOR
stage is then executed in listed order. Stage `s>d` reads bit `s` from the
current word and toggles bit `d`; a later stage therefore observes changes made
by an earlier stage. Stages may not repeat.

The rotated lane is `(lane + rotation) mod bank_count`. The raw bank is the low
`log2(bank_count)` bits of `word XOR rotated_lane`. Bank counts are powers of
two. Equal byte addresses form a broadcast: every member uses the raw bank of
the member with lowest lane number and consumes one service. Different addresses
at one bank conflict; conflict degree is the largest number of distinct
addresses served by any bank.

A trace may reveal any subset of lane banks and/or its conflict degree. A legal
mapping matches every revealed value. Recovery minimizes operation count
(stages plus one for a nonzero rotation), then selected-stage count, then the
lexicographic canonical string. No fitting rule exists outside this interpreter.
