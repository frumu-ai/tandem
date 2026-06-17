import type { IdentityPreset } from "@/lib/tauri";

export const FALLBACK_IDENTITY_PRESETS: IdentityPreset[] = [
  { id: "balanced", label: "Balanced" },
  { id: "concise", label: "Concise" },
  { id: "friendly", label: "Friendly" },
  { id: "mentor", label: "Mentor" },
  { id: "critical", label: "Critical" },
];
