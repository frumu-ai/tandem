import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

export default defineConfig({
  plugins: [preact()],
  resolve: {
    alias: {
      react: "preact/compat",
      "react-dom": "preact/compat",
      "react-dom/client": "preact/compat",
      "react-dom/test-utils": "preact/test-utils",
      "react/jsx-runtime": "preact/jsx-runtime",
      "react/jsx-dev-runtime": "preact/jsx-dev-runtime",
    },
  },
  server: {
    host: "127.0.0.1",
    port: Number.parseInt(process.env.TANDEM_CONTROL_PANEL_DEV_PORT || "39732", 10),
    proxy: {
      "/api": {
        target: process.env.TANDEM_CONTROL_PANEL_DEV_BACKEND_URL || "http://127.0.0.1:39733",
        changeOrigin: true,
      },
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
