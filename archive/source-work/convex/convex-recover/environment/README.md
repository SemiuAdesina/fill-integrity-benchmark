# Exact convex recovery diagnostics

The public contract is in `spec/polyhedron.md`. Run
`cargo test --offline --locked --test diagnostics` for synthetic rule-level
feedback that does not disclose the graded fixtures.

| Semantic family | Public diagnostic | Hidden graded coverage | Load-bearing |
| --- | --- | --- | --- |
| bounded, full-dimensional intersection | yes | yes | yes |
| exact support equality and offset choice | yes | yes | yes |
| reduced rational vertices and deduplication | yes | yes | yes |
| positive-area, nonredundant facets | yes | yes | yes |
| canonical face incidence and vertex order | yes | yes | yes |
| minimum facet count | yes | yes | yes |
| lexicographic family/offset tie | yes | yes | yes |

Public failures name the violated rule. Hidden cases combine these rules rather
than introducing a new geometric concept.
