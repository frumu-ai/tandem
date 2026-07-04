import { defineConfig, type Plugin } from "vite";
import preact from "@preact/preset-vite";
import path from "node:path";
import {
  DEFAULT_THEME_ID,
  STRUCTURAL_THEME_VARS,
  THEMES,
} from "../tandem-theme-contract/src/index.ts";

// Keep this in sync with STORAGE_KEY in src/app/themes.js.
const THEME_STORAGE_KEY = "tandem.themeId";

/**
 * Apply the persisted theme's CSS variables to <html> synchronously in <head>,
 * before first paint. Without this the browser paints the static `:root`
 * fallback first and then the app's mount-time applyTheme() swaps it in — a
 * theme flash on every load (and, for anyone who saved a non-default theme, a
 * flash regardless of what the static fallback is). Generated from the theme
 * contract so the theme data stays single-sourced.
 */
function themeBootstrapPlugin(): Plugin {
  const themeMap = Object.fromEntries(
    THEMES.map((theme) => [theme.id, { id: theme.id, cssVars: theme.cssVars }]),
  );
  const bootstrap = `(function(){try{` +
    `var THEMES=${JSON.stringify(themeMap)};` +
    `var STRUCT=${JSON.stringify(STRUCTURAL_THEME_VARS)};` +
    `var DEFAULT=${JSON.stringify(DEFAULT_THEME_ID)};` +
    `var saved=null;try{saved=localStorage.getItem(${JSON.stringify(THEME_STORAGE_KEY)});}catch(e){}` +
    `var theme=THEMES[saved]||THEMES[DEFAULT];if(!theme)return;` +
    `var root=document.documentElement;` +
    `for(var s in STRUCT){root.style.setProperty(s,STRUCT[s]);}` +
    `for(var k in theme.cssVars){if(theme.cssVars[k]!=null){root.style.setProperty(k,theme.cssVars[k]);}}` +
    `root.dataset.theme=theme.id;` +
    `root.style.colorScheme=theme.id==='porcelain'?'light':'dark';` +
    `}catch(e){}})();`;
  return {
    name: "tandem-theme-bootstrap",
    transformIndexHtml() {
      return [{ tag: "script", children: bootstrap, injectTo: "head-prepend" }];
    },
  };
}

export default defineConfig({
  plugins: [themeBootstrapPlugin(), preact()],
  resolve: {
    alias: {
      "@frumu/tandem-client": path.resolve(__dirname, "../tandem-client-ts/src/index.ts"),
      zod: path.resolve(__dirname, "node_modules/zod/index.js"),
      react: "preact/compat",
      "react-dom": "preact/compat",
      "react-dom/client": "preact/compat",
      "react-dom/test-utils": "preact/test-utils",
      "react/jsx-runtime": "preact/jsx-runtime",
      "react/jsx-dev-runtime": "preact/jsx-dev-runtime",
    },
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("@frumu/tandem-client")) return "tandem-client";
          if (id.includes("@fullcalendar")) return "fullcalendar";
          if (id.includes("@tanstack/react-query")) return "react-query";
          if (id.includes("motion")) return "motion";
          if (id.includes("marked") || id.includes("dompurify")) return "markdown";
          if (id.includes("preact")) return "preact-vendor";
          return "vendor";
        },
      },
    },
  },
});
