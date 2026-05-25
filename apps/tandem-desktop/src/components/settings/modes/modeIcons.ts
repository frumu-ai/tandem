import type { LucideIcon } from "lucide-react";
import {
  Bot,
  BookOpen,
  ClipboardList,
  Code,
  FileText,
  FlaskConical,
  ListChecks,
  MessageCircleQuestion,
  Rocket,
  Search,
  Shield,
  Sparkles,
  Terminal,
  Wrench,
  Zap,
} from "lucide-react";

export interface ModeIconOption {
  id: string;
  label: string;
  icon: LucideIcon;
}

export const MODE_ICON_OPTIONS: ModeIconOption[] = [
  { id: "zap", label: "Zap", icon: Zap },
  { id: "list-checks", label: "List Checks", icon: ListChecks },
  { id: "sparkles", label: "Sparkles", icon: Sparkles },
  { id: "code", label: "Code", icon: Code },
  { id: "message-circle-question", label: "Question", icon: MessageCircleQuestion },
  { id: "search", label: "Search", icon: Search },
  { id: "shield", label: "Shield", icon: Shield },
  { id: "book-open", label: "Book", icon: BookOpen },
  { id: "flask-conical", label: "Flask", icon: FlaskConical },
  { id: "wrench", label: "Wrench", icon: Wrench },
  { id: "rocket", label: "Rocket", icon: Rocket },
  { id: "file-text", label: "File Text", icon: FileText },
  { id: "clipboard-list", label: "Clipboard", icon: ClipboardList },
  { id: "bot", label: "Bot", icon: Bot },
  { id: "terminal", label: "Terminal", icon: Terminal },
];

export function normalizeModeIconId(value?: string | null): string | undefined {
  if (!value) return undefined;
  const cleaned = value
    .trim()
    .toLowerCase()
    .replace(/[_\s]+/g, "-")
    .replace(/[^a-z0-9-]/g, "")
    .replace(/-+/g, "-")
    .replace(/^-+|-+$/g, "");
  if (!cleaned) return undefined;
  return cleaned;
}

export function getModeIconOptionById(iconId?: string): ModeIconOption | undefined {
  const normalized = normalizeModeIconId(iconId);
  return MODE_ICON_OPTIONS.find((option) => option.id === normalized);
}
