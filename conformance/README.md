# comline-conformance

The codegen conformance corpus. Rollout step 3 of
`design/runtime-repo-structure.md`.

Each fixture in `src/lib.rs` is one schema as a hand-built `Vec<FrozenUnit>`
(not parsed — the corpus pins IR shape, not parser behaviour). `tests/` runs
every registered generator over every fixture and diffs the output against the
checked-in goldens under `tests/golden/<fixture>/<lang>.<ext>`.

```sh
cargo test -p comline-conformance                    # check
CONFORMANCE_BLESS=1 cargo test -p comline-conformance # regenerate goldens
```

A blessed golden is **a snapshot of what the generator does now**, not a
statement of what it *should* do. When a generator changes, the diff shows up in
that change's PR and the golden is re-blessed deliberately.

## Known gaps the current goldens reflect

These are visible in the checked-in output; each has its own fix later.

| fixture | gap |
|---|---|
| `optional_and_arrays`, `type_refs` (rust) | `Field.optional` is ignored — no `Option<T>` wrapper (TypeScript handles it: `field?: T`) |
| `primitives` (typescript) | `u128` is not in `map_str_type` — leaks through as the raw type name |
| `constant` (both) | `FrozenUnit::Constant` produces no output |
| `protocol` (both) | `throws` is dropped; the rust trait method has no `&self`. Error and dispatch codegen is the `core ↔ target` contract's surface 4, not designed into these generators yet |

When the `core` IR batch from that contract lands (`KindValue::Unit`,
`throws: Vec<u16>`, `Function.parameters`, drop `synchronous`), add a fixture
per change and re-bless.

See `ComlineProject/docs` → Design → *The `core` ↔ target contract*.
