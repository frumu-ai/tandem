# Tandem Component Manifests

These manifests give AI agents a conservative map of the first-party components implemented in this repository.

They are intended to answer:

- what Tandem is
- which top-level components this repo owns
- which runtime surfaces are present here
- which repo paths are the source of truth for each component

## Manifest Locations

- Canonical source manifests live in `manifests/components/`.
- Agent-runtime copies live in `apps/tandem-desktop/src-tauri/resources/agent-context/component-manifests/`.
- When a manifest is updated, both locations should be updated in the same change.

## Included Components

| Component              | Kind              | Manifest                                                                         | Why it has its own manifest                                                                                           | Primary source-of-truth paths                                                                                                                    |
| ---------------------- | ----------------- | -------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `tandem-engine`        | `runtime`         | [`tandem-engine.yaml`](../manifests/components/tandem-engine.yaml)               | Central runtime, API surface, orchestration, workflows, automations, memory, tools, and coordination state live here. | `engine/`, `crates/tandem-server/`, `crates/tandem-runtime/`, `crates/tandem-orchestrator/`, `crates/tandem-memory/`, `crates/tandem-workflows/` |
| `tandem-desktop`       | `desktop-client`  | [`tandem-desktop.yaml`](../manifests/components/tandem-desktop.yaml)             | The Tauri desktop application is a distinct first-party user-facing client with its own frontend and native backend.  | `apps/tandem-desktop/src/`, `apps/tandem-desktop/src-tauri/`                                                                                                                             |
| `tandem-tui`           | `terminal-client` | [`tandem-tui.yaml`](../manifests/components/tandem-tui.yaml)                     | The terminal UI is a separate client surface with its own UX, commands, and engine attach/bootstrap behavior.         | `crates/tandem-tui/`                                                                                                                             |
| `tandem-control-panel` | `web-client`      | [`tandem-control-panel.yaml`](../manifests/components/tandem-control-panel.yaml) | The browser-based control panel is implemented in-repo as its own package, gateway, and service bootstrap layer.      | `packages/tandem-control-panel/`                                                                                                                 |
| `tandem-sdk-clients`   | `sdk`             | [`tandem-sdk-clients.yaml`](../manifests/components/tandem-sdk-clients.yaml)     | The official TypeScript and Python SDKs are implemented in-repo and wrap the engine HTTP + SSE API.                   | `packages/tandem-client-ts/`, `packages/tandem-client-py/`                                                                                       |

## Ownership Boundaries

- The engine manifest owns engine capabilities that are clearly implemented here, including workflows, routines/automations, channels, MCP integration, browser automation integration, packs/skills loading, and shared runtime state.
- Client manifests describe first-party interfaces to that runtime. They do not own core orchestration state, workflow execution, or shared persistence.
- The SDK manifest describes API clients only. It does not imply that SDKs bundle or embed the runtime.

## Intentionally Excluded

These areas are present in the repo but are not modeled as standalone component manifests:

- `examples/`: deployment examples, quickstarts, and demos
- `agent-templates/` and `resources/skill-templates/`: templates and starter material
- `third_party/`: vendored external code
- `packages/tandem-engine/`, `packages/tandem-tui/`, `packages/tandem-ai/`: distribution and launcher wrappers, not separate top-level product components
- external providers, external MCP servers, Slack/Discord/Telegram platforms, and consumer applications not implemented in this repo

## Guidance For Agents

- Prefer conservative truth over inferred architecture.
- If a field is unclear from repository evidence, use `unknown`.
- Treat `source_of_truth` paths as the primary implementation anchors.
- Do not infer ownership of downstream apps, hosted services, or external ecosystems from integration code alone.
- Update manifests only after verifying ownership in code, config, or docs under the listed `source_of_truth` paths.
- Keep each `owns` entry paired with at least one concrete implementation path.
- When boundaries change, update both `manifests/components/` and `apps/tandem-desktop/src-tauri/resources/agent-context/component-manifests/` together.

## Optional Product-Messaging Fields

For product and marketing workflows, manifests may include these optional fields:

- `product_value`: one-sentence value proposition for the component.
- `primary_persona`: the main operator or buyer persona.
- `differentiators`: short list of concrete points that are true from repo evidence.
- `positioning_notes`: constraints and caveats for safe messaging.

These fields should never contradict implementation evidence in `source_of_truth`.
