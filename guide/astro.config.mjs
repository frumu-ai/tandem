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
const splitSiteAndBase = (value) => {
  if (!value) return { site: undefined, base: undefined }
  try {
    const parsed = new URL(value)
    const pathname = normalizeBase(parsed.pathname)
    return {
      site: `${parsed.origin}/`,
      base: pathname === "/" ? undefined : pathname,
    }
  } catch {
    return { site: normalizeSite(value), base: undefined }
  }
}
const explicit = splitSiteAndBase(explicitSite)
const site = normalizeSite(explicit.site ?? (isCi ? `https://${owner}.github.io/${repo}/` : "http://localhost:4321"))
const base = normalizeBase(explicitBase ?? explicit.base ?? (isCi && !explicitSite ? `/${repo}/` : "/"))

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
          label: "Introduction",
          items: ["start-here", "install-cli-binaries"],
        },
        {
          label: "Desktop & TUI Guide",
          items: ["tui-guide", "first-run", "agents-and-sessions", "agent-teams", "configuration", "design-system"],
        },
        {
          label: "Server & Deployment",
          items: ["headless-service", "channel-integrations", "desktop/headless-deployment", "installation"],
        },
        {
          label: "Developer Guide & SDKs",
          items: ["sdk/typescript", "sdk/python", "mcp-automated-agents", "webmcp-for-agents"],
        },
        {
          label: "Reference Architecture",
          items: [
            "protocol-matrix",
            "reference/engine-commands",
            "reference/tui-commands",
            "reference/tools",
            "reference/spawn-policy",
            "reference/agent-team-api",
            "reference/agent-team-events",
            "architecture",
            "engine-testing",
            "build-from-source",
          ],
        },
      ],
      social: {
        github: `https://github.com/${owner}/${repo}`,
      },
    }),
  ],
})
