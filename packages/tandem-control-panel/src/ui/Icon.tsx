import type { IconNode } from "lucide";
import {
  Activity, Archive, ArrowDown, ArrowLeftToLine, ArrowRight, ArrowUp,
  ArrowUpCircle, BadgeCheck, Binary, Blocks, BookOpen, Bot,
  Bug, Calendar, Check, ChevronDown, ChevronLeft, ChevronRight,
  ChevronUp, ChevronsDown, ChevronsUpDown, ClipboardList, Clock, Clock3,
  Code, Compass, Copy, CopyPlus, CornerUpLeft, Cpu,
  Database, DatabaseZap, Download, DownloadCloud, Eraser, ExternalLink,
  FileCheck2, FileInput, FilePlus, FileSearch, FileText, FileUp,
  Files, Filter, FilterX, FlaskConical, Folder, FolderCode,
  FolderGit2, FolderOpen, FolderPlus, FolderUp, Fullscreen, Globe,
  HardDrive, History, Home, Info, KeyRound, Layers,
  LayoutDashboard, LifeBuoy, Link, Link2, List, ListChecks,
  ListTree, ListX, LoaderCircle, Lock, LogIn, LogOut,
  Menu, MessageCircle, MessageSquare, MessagesSquare, MonitorCog, MoveHorizontal,
  Network, Package, PackagePlus, PaintBucket, PanelLeft, PanelRightOpen,
  Paperclip, Pause, PauseCircle, Pencil, Play, Plug,
  PlugZap, Plus, Radar, Radio, RefreshCw, Rocket,
  RotateCcw, RotateCw, Route, Save, Search, Send,
  Server, Settings, Settings2, Share2, Shield, ShieldAlert,
  ShieldCheck, ShieldOff, ShieldPlus, Siren, SkipForward, SlidersHorizontal,
  Sparkles, Square, SquareCheckBig, SquarePen, Star, Target,
  Terminal, Trash2, TriangleAlert, Unlink, Upload, UserCheck,
  UserPlus, Users, Webhook, Workflow, Wrench, X,
  XCircle, Zap,
} from "lucide";

const ICONS = {
  "activity": Activity,
  "archive": Archive,
  "arrow-down": ArrowDown,
  "arrow-left-to-line": ArrowLeftToLine,
  "arrow-right": ArrowRight,
  "arrow-up": ArrowUp,
  "arrow-up-circle": ArrowUpCircle,
  "badge-check": BadgeCheck,
  "binary": Binary,
  "blocks": Blocks,
  "book-open": BookOpen,
  "bot": Bot,
  "bug": Bug,
  "calendar": Calendar,
  "check": Check,
  "chevron-down": ChevronDown,
  "chevron-left": ChevronLeft,
  "chevron-right": ChevronRight,
  "chevron-up": ChevronUp,
  "chevrons-down": ChevronsDown,
  "chevrons-up-down": ChevronsUpDown,
  "clipboard-list": ClipboardList,
  "clock": Clock,
  "clock-3": Clock3,
  "code": Code,
  "compass": Compass,
  "copy": Copy,
  "copy-plus": CopyPlus,
  "corner-up-left": CornerUpLeft,
  "cpu": Cpu,
  "database": Database,
  "database-zap": DatabaseZap,
  "download": Download,
  "download-cloud": DownloadCloud,
  "eraser": Eraser,
  "external-link": ExternalLink,
  "file-check-2": FileCheck2,
  "file-input": FileInput,
  "file-plus": FilePlus,
  "file-search": FileSearch,
  "file-text": FileText,
  "file-up": FileUp,
  "files": Files,
  "filter": Filter,
  "filter-x": FilterX,
  "flask-conical": FlaskConical,
  "folder": Folder,
  "folder-code": FolderCode,
  "folder-git-2": FolderGit2,
  "folder-open": FolderOpen,
  "folder-plus": FolderPlus,
  "folder-up": FolderUp,
  "fullscreen": Fullscreen,
  "globe": Globe,
  "hard-drive": HardDrive,
  "history": History,
  "home": Home,
  "info": Info,
  "key-round": KeyRound,
  "layers": Layers,
  "layout-dashboard": LayoutDashboard,
  "life-buoy": LifeBuoy,
  "link": Link,
  "link-2": Link2,
  "list": List,
  "list-checks": ListChecks,
  "list-tree": ListTree,
  "list-x": ListX,
  "loader-circle": LoaderCircle,
  "lock": Lock,
  "log-in": LogIn,
  "log-out": LogOut,
  "menu": Menu,
  "message-circle": MessageCircle,
  "message-square": MessageSquare,
  "messages-square": MessagesSquare,
  "monitor-cog": MonitorCog,
  "move-horizontal": MoveHorizontal,
  "network": Network,
  "package": Package,
  "package-plus": PackagePlus,
  "paint-bucket": PaintBucket,
  "panel-left": PanelLeft,
  "panel-right-open": PanelRightOpen,
  "paperclip": Paperclip,
  "pause": Pause,
  "pause-circle": PauseCircle,
  "pencil": Pencil,
  "play": Play,
  "plug": Plug,
  "plug-zap": PlugZap,
  "plus": Plus,
  "radar": Radar,
  "radio": Radio,
  "refresh-cw": RefreshCw,
  "rocket": Rocket,
  "rotate-ccw": RotateCcw,
  "rotate-cw": RotateCw,
  "route": Route,
  "save": Save,
  "search": Search,
  "send": Send,
  "server": Server,
  "settings": Settings,
  "settings-2": Settings2,
  "share-2": Share2,
  "shield": Shield,
  "shield-alert": ShieldAlert,
  "shield-check": ShieldCheck,
  "shield-off": ShieldOff,
  "shield-plus": ShieldPlus,
  "siren": Siren,
  "skip-forward": SkipForward,
  "sliders-horizontal": SlidersHorizontal,
  "sparkles": Sparkles,
  "square": Square,
  "square-check-big": SquareCheckBig,
  "square-pen": SquarePen,
  "star": Star,
  "target": Target,
  "terminal": Terminal,
  "trash-2": Trash2,
  "triangle-alert": TriangleAlert,
  "unlink": Unlink,
  "upload": Upload,
  "user-check": UserCheck,
  "user-plus": UserPlus,
  "users": Users,
  "webhook": Webhook,
  "workflow": Workflow,
  "wrench": Wrench,
  "x": X,
  "x-circle": XCircle,
  "zap": Zap,
} as const satisfies Record<string, IconNode>;

export type IconName = keyof typeof ICONS;

export function isIconName(value: unknown): value is IconName {
  return typeof value === "string" && Object.prototype.hasOwnProperty.call(ICONS, value);
}

const A11Y_PROPS = ["aria-label", "aria-labelledby", "title", "role"];

export function Icon({
  name,
  size = 16,
  strokeWidth = 1.8,
  className = "",
  ...rest
}: {
  name: IconName;
  size?: number;
  strokeWidth?: number;
  className?: string;
  [key: string]: unknown;
}) {
  const node = ICONS[name];
  const hasA11yProp = Object.keys(rest).some((key) => A11Y_PROPS.includes(key));
  const classes = ["lucide", `lucide-${name}`, className].filter(Boolean).join(" ");
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width={strokeWidth}
      stroke-linecap="round"
      stroke-linejoin="round"
      {...(hasA11yProp ? {} : { "aria-hidden": "true" })}
      {...rest}
      className={classes}
    >
      {node.map(([tag, attrs], index) => {
        const Tag = tag as any;
        return <Tag key={index} {...attrs} />;
      })}
    </svg>
  );
}
