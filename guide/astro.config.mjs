import { defineConfig } from "astro/config"
import mermaid from "astro-mermaid"
import starlight from "@astrojs/starlight"

const [owner, repo] = (process.env.GITHUB_REPOSITORY ?? "frumu-ai/tandem").split("/")
const isCi = process.env.GITHUB_ACTIONS === "true"
const explicitSite = process.env.DOCS_SITE_URL
const explicitBase = process.env.DOCS_BASE_PATH
const normalizeSite = (value) => (value ? (value.endsWith("/") ? value : `${value}/`) : value)
const normalizeBase = (value) => {
  if (!value || value === "/") return "/"
  const withLeading = value.startsWith("/") ? value : `/${value}`
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`
}
const site = normalizeSite(explicitSite ?? (isCi ? `https://${owner}.github.io/${repo}/` : "http://localhost:4321"))
const base = normalizeBase(explicitBase ?? (isCi && !explicitSite ? `/${repo}/` : "/"))

export default defineConfig({
  site,
  base,
  integrations: [
    mermaid({
      autoTheme: true,
      theme: "forest",
    }),
    starlight({
      title: "Tandem Engine",
      customCss: ["./src/styles/custom.css"],
      editLink: {
        baseUrl: `https://github.com/${owner}/${repo}/edit/main/tandem/guide/src/content/docs/`,
      },
      sidebar: [
        {
          label: "Getting Started",
          items: [
            "start-here",
            "install-cli-binaries",
            "first-run",
            "build-from-source",
            "installation",
            "usage",
            "headless-service",
            "webmcp-for-agents",
          ],
        },
        {
          label: "User Guide",
          items: [
            "tui-guide",
            "configuration",
            "agents-and-sessions",
            "desktop/headless-deployment",
            "agent-teams",
            "mcp-automated-agents",
            "webmcp-for-agents",
            "design-system",
          ],
        },
        {
          label: "Reference",
          items: [
            "reference/engine-commands",
            "reference/tui-commands",
            "reference/tools",
            "reference/spawn-policy",
            "reference/agent-team-api",
            "reference/agent-team-events",
            "protocol-matrix",
          ],
        },
        {
          label: "Developer Documentation",
          items: ["architecture", "engine-testing", "cli-vision", "sdk-vision"],
        },
      ],
      social: {
        github: `https://github.com/${owner}/${repo}`,
      },
    }),
  ],
})
