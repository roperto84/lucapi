# Team secrets in the Keychain

A workspace file is meant to be shared: commit it, send it, keep it in a repo.
Secrets do not have to travel with it.

With **Settings → Secrets → Keep secrets in the macOS Keychain** enabled,
LucAPI writes secret values into the login Keychain and stores only a
reference in the workspace:

```json
{ "enabled": true, "key": "token", "value": "keychain://env:1011:token", "secret": true }
```

Sending, snippets, exports and the published documentation still see the real
value: references are resolved when the workspace is loaded (and again when a
variable is expanded, in case an item was removed behind the app's back).

## What is covered

- Environment variables marked **secret**.
- Headers and query parameters marked **secret** (the new checkbox next to each
  row in the Params and Headers tabs).
- Auth values that are secret by nature: bearer token, basic password, API key
  and the OAuth2 client secret.

Nothing else leaves the file.

## Auditing a workspace

```bash
lucapi secrets --audit --workspace ./api/workspace.json
# …/workspace.json: 8 secret(s), 8 in the Keychain, 0 in plain text
```

The command exits with `1` when any secret is still plain text, so it works as a
pre-commit or CI guard:

```yaml
- run: lucapi secrets --audit --workspace ./api/workspace.json
```

Check that the Keychain is usable (store, read back, delete a probe item):

```bash
lucapi secrets --check
```

## Working in a team

1. Each person keeps their own `token` values in their own Keychain.
2. The workspace still travels with the collection structure, environments and
   references, so ids and links keep working.
3. A new teammate opens the workspace, sees the requests, and types their
   secrets once: the next save stores them and leaves the references in place.
4. *Forget stored secrets* removes every item for the workspace from the
   Keychain; values stay in memory until the next save, so you can decide
   whether to retype them or turn the toggle off.

## Limits

- Referenced items are keyed by environment or request id plus the field name.
  Renaming a field or deleting a request leaves the old Keychain item behind;
  delete it from Keychain Access if it matters.
- Values are handed to `security` as arguments, so they appear briefly in the
  process table on the same machine. Keep that in mind on shared hosts.
- If the Keychain refuses a value (locked keychain, no permission), LucAPI warns
  and leaves the value in the workspace rather than losing it.
