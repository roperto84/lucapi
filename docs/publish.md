# Publishable documentation

LucAPI can turn a workspace into a **static, read-only documentation site**: a
single `index.html` with inline CSS and JavaScript, no external assets, no build
step. Publish it in a repo (GitHub Pages), an internal static host, or send the
file to a client.

## From the app

`Docs → Publish…` (or `File → Publish documentation…`).

- **Title** and **Intro** shown at the top.
- **Public URL**: where the site will live. Used to build share links, both by
  the per-endpoint *Copy link* buttons and by the *Copy a share link* action.
- **Toggles**: example responses, request bodies, auth summary, dark theme and
  whether `{{variables}}` are resolved against the active environment.
- The dialog lists every endpoint with its anchor (`#list-users-1001`), so you
  know exactly what you are about to share.
- **Publish…** asks for a folder and writes `index.html` inside it, then
  **Open site** opens it in the browser.

The Docs page also has a copy-link button on every request card, using the
saved public URL.

## From the command line

```bash
lucapi docs \
  --output ./site \
  --title "Acme API" \
  --intro "Reference for the public endpoints." \
  --base-url https://docs.acme.dev/api
```

| Flag | Effect |
|------|--------|
| `--output <dir>` | Directory that receives `index.html` (required) |
| `--workspace <path>` | Workspace JSON (default `~/.lucapi/workspace.json` or `$LUCAPI_WORKSPACE`) |
| `--title`, `--intro` | Header content |
| `--base-url` | Public URL used for share links |
| `--light` | Light theme |
| `--no-examples` / `--no-bodies` / `--no-auth` | Leave sections out |
| `--raw-urls` | Keep `{{variables}}` instead of resolving them |

`--output` can be a GitHub Pages `docs/` folder, so publishing is part of the
same pipeline that runs the tests:

```yaml
- run: lucapi docs --output public --base-url https://acme.dev/api
- uses: actions/upload-pages-artifact@v3
  with: { path: public }
```

## Secrets never leave

Before anything is written, the generator redacts:

- headers and parameters marked as secret,
- secret environment variables,
- bearer tokens, basic auth passwords, API key values and OAuth2 client
  secrets.

Redacted values become `••••` everywhere they appear, including inside bodies,
URLs and descriptions. A published site can be shared without a second review
pass.
