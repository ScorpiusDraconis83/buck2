---
id: validation
title: Validations
---

Validations let a rule author declare additional pass/fail checks that Buck2
enforces whenever the target is in a requested build's transitive closure.
A validation succeeds when the action that produces its result artifact
writes a JSON document signalling success; otherwise the build fails.

## When validations run

A validation attached to target `//A:a` runs whenever a `buck2 build` or
`buck2 test` request resolves a graph that contains `//A:a` as a transitive
dependency. Validations execute in parallel with the rest of the build —
they only need to finish before Buck2 reports the requested target complete.

Validations are *not* re-run when the producing action's inputs are
unchanged (standard Buck2 caching).

## Declaring a validation

Return a [`ValidationInfo`](../../api/build/ValidationInfo) provider from your
rule's `impl`, populated with one or more
[`ValidationSpec`](../../api/build/ValidationSpec) values:

```python
def _my_rule_impl(ctx):
    report = ctx.actions.declare_output("schema_report.json")
    ctx.actions.run(
        cmd_args("validate-schema", "--out", report.as_output(), ctx.attrs.src),
        category = "schema_validation",
    )
    return [
        DefaultInfo(default_output = ctx.attrs.src),
        ValidationInfo(validations = [
            ValidationSpec(
                name = "schema",
                validation_result = report,
            ),
        ]),
    ]
```

Constraints:

- Each spec needs a non-empty name, unique within the
  [`ValidationInfo`](../../api/build/ValidationInfo).
- `validation_result` must be a build artifact (not a source file).
- The provider must contain at least one spec.

## Writing the validator

The validator is just an action — any binary that writes a JSON file in the
expected schema works. The schema:

```json
{
  "version": 1,
  "data": {
    "status": "success",
    "message": "Optional human-readable detail."
  }
}
```

| Field          | Type   | Required | Notes                                       |
| -------------- | ------ | -------- | ------------------------------------------- |
| `version`      | int    | yes      | Currently `1`.                              |
| `data.status`  | string | yes      | `"success"` or `"failure"`.                 |
| `data.message` | string | no       | Shown to the user; supply on failure.       |

Buck2 reports three distinct errors if the file is malformed: invalid JSON,
incompatible version, or schema mismatch. Treat the JSON file as a stable
machine contract, not a log — write logs to stderr or a separate artifact.

## Required vs optional validations

Pass `optional = True` to a [`ValidationSpec`](../../api/build/ValidationSpec)
to mark it advisory:

```python
ValidationSpec(name = "slow_lint", validation_result = report, optional = True)
```

Optional validations are skipped by default. Users opt in per-name on the
CLI:

```shell
buck2 build //A:a --enable-optional-validations slow_lint
```

Use this for expensive or noisy checks you don't want to gate every build on.
