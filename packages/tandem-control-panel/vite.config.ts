import { defineConfig } from "vite";
import preact from "@preact/preset-vite";
import path from "node:path";

export default defineConfig({
  plugins: [preact()],
  resolve: {
    alias: {
      "@frumu/tandem-client": path.resolve(__dirname, "../tandem-client-ts/src/index.ts"),
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
