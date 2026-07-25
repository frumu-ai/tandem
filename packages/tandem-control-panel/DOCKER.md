# Tandem Control Panel in Docker

The easiest container path is to run the control panel and the Tandem engine as separate services on the same Docker network.

## What gets installed

- `@frumu/tandem-panel` from the checked-in package source in the control-panel container
- `@frumu/tandem` in the engine container

The engine is installed at an exact npm version when the image is built. The panel and TypeScript client are built from the checked-in lockfile and source so the image does not depend on registry lag or a stale panel artifact. Both images use the same digest-pinned Node base.

## Run

From `packages/tandem-control-panel`:

```bash
npm run docker:up
```

Then open:

```bash
http://localhost:39734
```

The default ports are:

- Control panel: `39734`
- Engine: `39731` inside the Docker network

The engine is not published to the host by default. The control panel is the public entry point.

## Login token

The host-side `docker:up` helper creates `./secrets/tandem_api_token` before Compose starts. The directory is mode `0700`, the file is mode `0600`, and an existing invalid, empty, non-regular, or symbolic-link token is rejected. Existing files are opened with `O_NOFOLLOW`, validated with `fstat`, and read and chmod’d through the same descriptor. The launcher maps both runtime services to the invoking non-root host UID/GID, so the owner-only token remains readable without making it group- or world-readable. Containers never create or modify this secret.

You can use that token to sign in to the control panel.

To read it from the host:

```bash
cat secrets/tandem_api_token
```

The single token file is mounted read-only into the engine container. Keep the ignored `secrets/` directory to retain the same token across restarts. To rotate it, stop the stack, replace the host file with a new `tk_` plus 32-lowercase-hex token at mode `0600`, then start the stack again.

Useful follow-up commands:

```bash
npm run docker:logs
npm run docker:ps
npm run docker:down
npm run docker:token
```

`npm run docker:token` prints the current engine token from `secrets/tandem_api_token`. It does not fall back to executing a command inside a running container.

## Environment overrides

Useful variables:

- `TANDEM_DOCKER_PANEL_PORT`
- `TANDEM_ENGINE_PORT`
- `TANDEM_DOCKER_UID` and `TANDEM_DOCKER_GID` (non-root numeric overrides for root launchers; ordinary non-root launchers must use their invoking host identity)

The image pins both the engine version and the SHA-256 of the extracted Linux x64 `tandem-engine` release binary in `docker/engine.Dockerfile`. Release preparation must update both together with `TANDEM_ENGINE_BINARY_SHA256=<extracted-binary-sha256> scripts/bump-version.sh <version>`; release CI fails before publishing if the canonical package and Docker pin differ. Runtime OS packages come from a dated Debian snapshot and are version-pinned.

On startup, `tandem-state-migrate` checks an ownership marker in each named volume. It performs a one-time recursive ownership migration when upgrading a volume created by the older root-running images, then both runtime services start under the mapped non-root identity. The migrator has a read-only root filesystem, drops all capabilities, adds back only `CHOWN` and `DAC_OVERRIDE`, and exits before either runtime starts.

If you already have an engine running elsewhere, you can point the panel at it by changing `TANDEM_ENGINE_URL` and disabling the local engine service.

## Why this layout works

- The browser only talks to the control panel.
- The control panel talks to the engine over the Docker network.
- The engine token stays in a file instead of being hard-coded into the browser.
- The panel does not auto-start a second engine when the engine URL is a Docker service name.
- Both services run as the invoking unprivileged UID/GID with a read-only root filesystem, all Linux capabilities dropped, `no-new-privileges`, and only their named state volume plus a constrained `/tmp` writable.
- The engine is not published to the host. This Compose profile is for local or single-host self-managed use; hosted-enterprise release evidence is a separate fail-closed gate documented in `docs/SECURITY_ASSURANCE_PROFILE.md`.
