# Recover an exact bounded polyhedron

Repair `src/lib.rs`. It currently selects the first available offset from each
supplied family and returns no vertices or faces, so it neither rejects planes
the evidence rules out nor reconstructs the required exact geometry.

A legal object is the intersection of a subset of supplied
plane families, choosing at most one supplied rational offset per family, under
`n·x <= d`. Support measurements are exact maxima in integer directions.

The chosen intersection must be nonempty, full-dimensional and bounded. Every
selected plane must own a positive-area facet; redundant planes are illegal.
Minimize selected facet count, then the lexicographic list of `(family,offset)`.

Compute vertices by exact triple intersection, reduce rationals, deduplicate and
sort lexicographically. Faces sort by plane key and contain sorted vertex indices
lying exactly on that plane. Equality is inside. The verifier evaluates withheld
supports from returned vertices; floating epsilon or a bounding-box substitute
is incorrect.
