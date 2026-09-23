# GitHub integration

Orbit uses one GitHub App per workspace. Workspace owners and admins start App registration in workspace settings. GitHub returns the App credentials to Orbit. Orbit saves the App ID and encrypts the private key and webhook secret in its database. Each project then selects an installed repository and an issue label. No repository webhook and no Orbit API token are needed.

## Server setup

1. Give Orbit a public HTTPS URL that serves both the UI and `/api/v1/integrations/github/webhook` to GitHub. In production, set the server's canonical `ORBIT__HTTP__PUBLIC_ORIGIN=https://orbit.example.com`. For local testing, use a public HTTPS tunnel to the Orbit web server with its `/api` proxy; Orbit uses the HTTPS domain open in your browser when you start App registration.
2. Generate a 32-byte key with `openssl rand -base64 32`. Store it as `ORBIT__SECRETS__APP_KEY` or use Orbit's secret-file configuration. Keep this key outside the database and its backups. Orbit cannot verify webhooks or use the saved App credentials if the key is lost.
3. Restart Orbit. Do not set `GITHUB_CONNECTIONS` or the older `GITHUB_*` connection variables; Orbit no longer reads them.

For local development, `just dev` also loads a root `.env.local` file. Put `ORBIT__SECRETS__APP_KEY=<base64-key>` there. Keep the file private and out of Git. Open Orbit through a public HTTPS tunnel to the web server before you select **Register GitHub App**. You do not need to set `ORBIT__HTTP__PUBLIC_ORIGIN` to the tunnel URL for this development flow. A plain `http://127.0.0.1` tab cannot register a GitHub App.

## Register the workspace App and connect a project

1. Open **Settings → GitHub** as a workspace owner or admin.
2. Enter a GitHub organization name if the organization should own the App, or leave it blank for a personal App. Select **Register GitHub App**. Orbit sends a private App manifest to GitHub; only the App owner can install it. Approve the registration on GitHub. GitHub sends a one-time code to Orbit, and Orbit stores the App credentials. This needs a reachable HTTPS origin.
3. Select **Install or change repositories** in workspace settings. Install the App on the GitHub account and select the repositories. GitHub sends installation events to Orbit.
4. Open **Tasks → Project settings → GitHub** and refresh after installation. Select a repository and enter a GitHub label, then save. Create the same label in GitHub. You can connect more projects to the same repository if each project uses a different label. A project has one repository connection.
5. Add that label to an issue or pull request. Orbit creates one task in the selected project. Later GitHub edits update its title and body; close and reopen events update its status. A merged pull request completes its task. A pull request closed without a merge cancels its task. Reopening it moves the task to unstarted. Removing the label pauses updates, keeps the task, and shows a **GitHub sync paused** badge. Adding the label again resumes updates on the same task. Do not put two configured project labels on one issue or pull request.

If GitHub rejects the App manifest, fix the problem and select **Register GitHub App** again. A new attempt replaces the pending registration. GitHub sends [`installation` and `installation_repositories` events](https://docs.github.com/en/webhooks/webhook-events-and-payloads#installation) to all GitHub Apps automatically; these events must not appear in the manifest's `default_events` list.

The task title and description are read-only in Orbit. Other task fields remain editable. Comments do not sync. Orbit does not send task edits to GitHub.

A labeled pull request creates its own Orbit task, with its GitHub title, body, and pull-request link. Orbit does not search the pull request title, body, or branch for an Orbit task URL. An unlabeled pull request does not create a task. Removing its project label pauses sync, and adding the label again resumes the same task.

If you delete a linked task in Orbit, the next GitHub event with the project label restores that same task and applies the current GitHub title, body, and status. An event without the label keeps the task deleted and pauses sync.

GitHub App registration uses the [GitHub App manifest flow](https://docs.github.com/en/apps/sharing-github-apps/registering-a-github-app-from-a-manifest). GitHub sends signed events as described in its [webhook validation guide](https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries). App installation is described in [GitHub's installation guide](https://docs.github.com/en/apps/using-github-apps/installing-your-own-github-app).
