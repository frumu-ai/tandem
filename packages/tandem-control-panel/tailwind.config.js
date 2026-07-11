import forms from "@tailwindcss/forms";

/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,mjs,ts,tsx}"],
  theme: {
    // TAN-685: the application type scale is intentionally limited to eight sizes.
    // Markdown content may define its own prose rhythm, but application UI must use these tokens.
    fontSize: {
      micro: ["0.625rem", { lineHeight: "0.875rem" }],
      caption: ["0.6875rem", { lineHeight: "1rem" }],
      xs: ["0.75rem", { lineHeight: "1rem" }],
      sm: ["0.875rem", { lineHeight: "1.25rem" }],
      base: ["1rem", { lineHeight: "1.5rem" }],
      lg: ["1.125rem", { lineHeight: "1.75rem" }],
      xl: ["1.5rem", { lineHeight: "2rem" }],
      "2xl": ["2rem", { lineHeight: "2.25rem" }],
    },
    borderRadius: {
      none: "var(--radius)",
      sm: "var(--radius)",
      DEFAULT: "var(--radius)",
      md: "var(--radius)",
      lg: "var(--radius)",
      xl: "var(--radius)",
      "2xl": "var(--radius)",
      "3xl": "var(--radius)",
      "4xl": "var(--radius)",
      xl2: "var(--radius)",
      full: "9999px",
    },
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
        "tcp-text-primary": "var(--color-text)",
        "tcp-text-secondary": "var(--color-text-muted)",
        "tcp-text-tertiary": "var(--color-text-subtle)",
      },
      fontFamily: {
        sans: ["var(--font-sans)"],
        mono: ["var(--font-mono)"],
        display: ["var(--font-display)"],
      },
      boxShadow: {
        soft: "var(--shadow-offset)",
        hard: "var(--shadow-offset-lg)",
      },
    },
  },
  plugins: [forms],
};
