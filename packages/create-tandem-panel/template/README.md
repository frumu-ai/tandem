# **PROJECT_NAME**

Editable Tandem control panel app scaffold.

## Quick Start

```bash
npm install
npm run init:env
npm run dev
```

`npm run dev` starts:

- the local Tandem panel backend on `http://127.0.0.1:39733`
- the Vite app on `http://127.0.0.1:39732`

`npm run start` serves the built production app from the local runtime in `bin/setup.js`.

## What To Customize

- Pages and route content: `src/pages/` and `src/app/`
- UI primitives and shared chrome: `src/ui/`
- Themes and visual tokens: `src/app/themeContract.ts` and `src/app/themes.js`
- Swarm/orchestrator backend behavior: `server/` and `bin/setup.js`

## Engine Connection

Environment variables live in `.env`. Start from:

```bash
cp .env.example .env
```

Useful vars:

- `TANDEM_ENGINE_URL`
- `TANDEM_ENGINE_HOST`
- `TANDEM_ENGINE_PORT`
- `TANDEM_CONTROL_PANEL_PORT`
- `TANDEM_CONTROL_PANEL_ENGINE_TOKEN`
- `TANDEM_API_TOKEN`

If the panel auto-starts an engine, the token is printed by the local runtime.

## What You Can Delete

If you want a smaller app, it is safe to remove pages, presets, routes, and unused UI components as long as you also remove their imports.

This scaffold is the supported customization path for changing the UI and behavior. Avoid editing installed dependency files in `node_modules`.
