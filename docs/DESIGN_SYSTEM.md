# Tandem Design System: "Charcoal & Fire"

A guide to the visual identity, colors, and typography used in the Tandem AI Workspace project.

## 1. Color Palette

| Name              | Hex Code                   | Usage                                | Tailwind Class                         |
| :---------------- | :------------------------- | :----------------------------------- | :------------------------------------- |
| **Deep Charcoal** | `#121212`                  | Main Background                      | `bg-background`                        |
| **Solar Yellow**  | `#F59E0B`                  | Power, Action, Primary Accents       | `text-solar-yellow`, `bg-solar-yellow` |
| **Crimson Red**   | `#EF4444`                  | Security, Privacy, Secondary Accents | `text-crimson-red`, `bg-crimson-red`   |
| **Off-White**     | `#F5F5F5`                  | Primary Text                         | `text-foreground`                      |
| **Muted Grey**    | `rgba(245, 245, 245, 0.5)` | Secondary Text                       | `text-foreground/50`                   |

---

## 2. Typography

### Primary Font: Geist Sans

Optimized for readability and technical precision.

- **Headings**: Weight 900 (Black). Tracking: `-0.05em` (Tighter).
- **Subheadings**: Weight 700 (Bold).
- **Body**: Weight 400 (Regular) or 500 (Medium).

### Monospace Font: Geist Mono

Used for status indicators, versioning, and code.

- **Usage**: Metadata, technical readouts, footer labels.

---

## 3. UI Elements & Special Effects

### Glassmorphism

The signature tactile feel of the Tandem UI.

- **Background**: `rgba(255, 255, 255, 0.03)`
- **Blur**: `20px` (backdrop-filter)
- **Border**: `1px solid rgba(255, 255, 255, 0.08)`
- **Corner Radius**: `1rem` (2xl) to `1.5rem` (3xl)

### Glows & Gradients

Subtle ambient light to guide the eye.

- **Solar Glow**: `bg-solar-yellow/5` with `blur-[120px]`
- **Security Glow**: `bg-crimson-red/5` with `blur-[120px]`

---

## 4. Animation Philosophy (Framer Motion)

- **Entrances**: `y: 50` to `y: 0` with `opacity: 0` to `1`.
- **Duration**: `0.8s` for major sections, `0.5s` for hero elements.
- **Interactivity**: `scale: 1.05` on hover for primary CTAs.
- **Feedback**: Bounce animations for "Action Required" states (e.g., Approval buttons).

---

## 5. Technical Implementation (CSS)

```css
:root {
  --background: #121212;
  --foreground: #f5f5f5;
  --solar-yellow: #f59e0b;
  --crimson-red: #ef4444;
}

.glass {
  background: rgba(255, 255, 255, 0.03);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border: 1px solid rgba(255, 255, 255, 0.08);
}
```
