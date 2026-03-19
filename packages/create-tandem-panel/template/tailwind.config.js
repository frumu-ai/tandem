import forms from "@tailwindcss/forms";

/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,mjs,ts,tsx}"],
  theme: {
    extend: {
      colors: {
        canvas: "var(--color-background)",
        panel: "var(--color-surface)",
        card: "var(--color-surface-elevated)",
        muted: "color-mix(in srgb, var(--color-surface-elevated) 85%, #000 15%)",
        soft: "var(--color-border-subtle)",
        accent: "var(--color-text-muted)",
        accent2: "var(--color-text-subtle)",
        ok: "var(--color-success)",
        warn: "var(--color-warning)",
        err: "var(--color-error)",
        info: "var(--color-primary)",
      },
      fontFamily: {
        sans: ["var(--font-sans)"],
        mono: ["var(--font-mono)"],
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
