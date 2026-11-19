# Running LucAPI collections in CI

`lucapi run` executes a workspace without opening the window, reports the
assertions and exits with a useful status code.

```bash
lucapi run \
  --workspace ./api/workspace.json \
  --env Production \
  --format junit --output report.xml
```

| Exit code | Meaning |
|-----------|---------|
| 0 | Every request passed |
| 1 | At least one request or assertion failed |
| 2 | Invalid usage or unreadable workspace |

## What counts as a pass

A request passes when it receives a response **and** every enabled assertion
passes. Requests without assertions must answer with a status below 400, so a
plain 500 never slips through.

Captures run in order: a value captured from one request is available to the
next one, which makes login → use token flows work unattended.

## Selecting work

```bash
lucapi run --list                       # show what would run
lucapi run --folder Billing            # one collection
lucapi run --only "create" --only "list"
lucapi run --var baseUrl=http://127.0.0.1:8080
lucapi run --timeout 5000 --fail-fast
```

`--var` overrides variables of the selected environment, which is the simplest
way to point a suite at a preview deployment.

## Reports

- `--format text` (default): one line per request plus a summary, failures
  spelled out underneath.
- `--format json`: machine-readable totals and per-request results.
- `--format junit`: JUnit XML, grouped by collection, ready for GitHub Actions,
  GitLab, Jenkins or Buildkite.

## GitHub Actions

```yaml
- name: API tests
  run: lucapi run --env CI --format junit --output report.xml
- uses: actions/upload-artifact@v4
  if: always()
  with:
    name: api-report
    path: report.xml
```

The workspace path can also come from `LUCAPI_WORKSPACE`.
