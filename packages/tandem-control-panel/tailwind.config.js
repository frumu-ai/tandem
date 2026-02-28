import forms from "@tailwindcss/forms";

/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,mjs}"],
  theme: {
    extend: {
      colors: {
        canvas: "#0f1115",
        panel: "#171a20",
        card: "#1c2129",
        muted: "#242a33",
        soft: "#2f3642",
        accent: "#6b7280",
        accent2: "#7c8799",
        ok: "#84cc16",
        warn: "#f59e0b",
        err: "#f43f5e",
        info: "#60a5fa",
      },
      fontFamily: {
        sans: ["Manrope", "Inter", "system-ui", "sans-serif"],
        mono: ["JetBrains Mono", "ui-monospace", "SFMono-Regular", "monospace"],
      },
      boxShadow: {
        soft: "0 8px 30px rgba(0, 0, 0, 0.22)",
      },
      borderRadius: {
        xl2: "1rem",
      },
    },
  },
  plugins: [forms],
};
