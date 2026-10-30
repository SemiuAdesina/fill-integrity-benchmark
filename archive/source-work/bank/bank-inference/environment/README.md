# Bank inference project

The project in `/app` contains a deterministic shared-memory bank interpreter
and an incomplete recovery routine. Edit only `/app/src/recover.cpp`.

Build with `cmake -S /app -B /app/build` followed by
`cmake --build /app/build`. The verifier uses its own pristine interpreter and
build files, and collects only the recovery source.

## Public feedback ladder

After building, run `ctest --test-dir /app/build --output-on-failure`.

| Semantic family | Public diagnostic | Hidden graded coverage | Load-bearing |
| --- | --- | --- | --- |
| byte-to-word integer truncation | yes | yes | yes |
| ordered XOR stages | yes | yes | yes |
| lane rotation | yes | yes | yes |
| lowest-lane broadcast leader | yes | yes | yes |
| distinct-address conflict degree | yes | yes | yes |
| evidence shared across traces | yes | yes | yes |
| operation cost, stage count and canonical tie | yes | yes | yes |

The synthetic diagnostics print one named public rule on failure. They do not
publish the verifier's compound traces or expected mappings.
