# Deploy with Coolify

1. Create a new resource in Coolify and select **Docker Compose Empty**.
2. Paste the contents of [`compose.yaml`](compose.yaml) into the compose editor.
3. Assign the production domain to port `3000`.
4. Open the [coolBot OAuth2 settings](https://discord.com/developers/applications/1301743930785660958/oauth2):
   - Add `https://<production-domain>/api/auth/callback` as a redirect URL.
   - Set `DISCORD_OAUTH_CLIENT_ID` to the displayed client ID.
   - Reset the client secret and set the new value as `DISCORD_OAUTH_CLIENT_SECRET`.
5. Get `COOLBOT_DISCORD_TOKEN` from the [coolBot bot settings](https://discord.com/developers/applications/1301743930785660958/bot). Reuse the existing production token if available.
6. Get `MODMAIL_DISCORD_TOKEN` from the [Modmail bot settings](https://discord.com/developers/applications/1539918676348764161/bot).
7. Reuse `NTFY_TOPIC` and `NTFY_RESPONSE_TOPIC` from the old coolBot deployment, or contact ShadowArcanist for them.
8. Set `TRUST_PROXY_HOPS` to `1` for Traefik or `2` for Cloudflare in front of Traefik, then deploy.
9. Add the deployment webhook URL as the `COOLIFY_WEBHOOK_COOLBOT` GitHub Actions secret, and add a Coolify API token as `COOLIFY_TOKEN`.

Coolify generates the public URL and dashboard session secret used by the compose file. The `coolbot-data` volume keeps the SQLite database between deployments.
