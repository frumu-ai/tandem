import { defineConfig } from "astro/config"
import mermaid from "astro-mermaid"
import starlight from "@astrojs/starlight"

const [owner, repo] = (process.env.GITHUB_REPOSITORY ?? "frumu-ai/tandem").split("/")
const isCi = process.env.GITHUB_ACTIONS === "true"
const explicitSite = process.env.DOCS_SITE_URL
const explicitBase = process.env.DOCS_BASE_PATH
const site = explicitSite ?? (isCi ? `https://${owner}.github.io/${repo}/` : "http://localhost:4321")
const base = explicitBase ?? (isCi && !explicitSite ? `/${repo}/` : "/")

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
          items: ["installation", "usage"],
        },
        {
          label: "User Guide",
          items: [
            "desktop/learn-walkthroughs",
            "tui-guide",
            "configuration",
            "agents-and-sessions",
            "design-system",
          ],
        },
        {
          label: "Reference",
          items: [
            "reference/engine-commands",
            "reference/tui-commands",
            "reference/tools",
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
