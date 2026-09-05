# Development Bootstrap Design

## Goal

Make local and Jean development start at the login screen with predictable credentials, without weakening production setup or origin security.

## Development migration flags

`orbit migrate run` gains these flags:

- `--seed`: after successful migrations, idempotently install representative development data, including `test@example.com` with password `password`. Existing development records and user changes are preserved.
- `--reset`: before migration, delete the configured development SQLite database, its SQLite sidecars, and configured attachment data. This flag is destructive, is accepted only in explicit `development` mode, and requires `--yes`.
- `--reset --seed`: reset, migrate, then seed in that order.

Both flags are rejected outside explicit development mode. A failure stops the sequence; later phases do not run.

The existing standalone development seed/reset commands may remain as compatibility wrappers, but the documented and Jean-facing workflow uses `migrate run` flags so database preparation has one entry point.

## Development startup

`just dev` runs `orbit migrate run --seed` before starting the API and Vite processes. Seeding is idempotent and never resets or overwrites existing development data.

Because seeded data marks the installation initialized, the frontend opens at login rather than one-time setup. Developers sign in with:

- Email: `test@example.com`
- Password: `password`

One-time setup tokens remain required for uninitialized production installations.

## Origin policy

In explicit `development` mode, the HTTP security layer accepts any syntactically valid `http` or `https` Origin. Missing Origin on unsafe requests remains rejected. This removes host-specific public-origin variables from Jean and permits localhost, LAN, and Tailscale development URLs.

Production continues to require the configured exact public Origin and the existing trusted-proxy/HTTPS checks.

## Jean configuration

Jean continues to run Orbit on the dedicated ports:

- Frontend: `18888`
- API: `18080`

Its run command sets only the development ports and invokes `just dev`; it does not set a public-origin URL. Tailscale Serve continues to proxy the frontend.

## Verification

Automated coverage must prove:

- `--seed` is idempotent and preserves existing development changes.
- The seeded credentials authenticate successfully.
- `--reset` requires both development mode and `--yes`.
- `--reset --seed` recreates a usable seeded database.
- Development accepts distinct localhost and Tailscale-style Origins but still rejects missing Origin.
- Production rejects non-configured Origins.
- `just dev` contains no reset step and Jean contains no origin-specific environment variable.

