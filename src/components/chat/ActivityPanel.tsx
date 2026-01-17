import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  FileText,
  Search,
  FolderOpen,
  Terminal,
  Eye,
  ChevronRight,
  ChevronDown,
  Activity,
  Clock,
  CheckCircle2,
  XCircle,
  Loader2,
  X,
  Maximize2,
  Minimize2,
} from "lucide-react";

export interface ActivityItem {
  id: string;
  type: "file_read" | "file_write" | "search" | "command" | "browse" | "thinking" | "tool";
  tool?: string;
  title: string;
  detail?: string;
  status: "pending" | "running" | "completed" | "failed";
  timestamp: Date;
  result?: string;
  args?: Record<string, unknown>;
}

interface ActivityPanelProps {
  activities: ActivityItem[];
  isVisible: boolean;
  onToggle: () => void;
}

export function ActivityPanel({ activities, isVisible, onToggle }: ActivityPanelProps) {
  const [expandedItems, setExpandedItems] = useState<Set<string>>(new Set());
  const [isExpanded, setIsExpanded] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when new activities arrive
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [activities]);

  const toggleItem = (id: string) => {
    setExpandedItems((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const getIcon = (type: ActivityItem["type"], tool?: string) => {
    // Check specific tool names first
    if (tool) {
      const toolLower = tool.toLowerCase();
      if (toolLower.includes("read") || toolLower.includes("file")) {
        return <FileText className="h-4 w-4" />;
      }
      if (toolLower.includes("search") || toolLower.includes("grep") || toolLower.includes("find")) {
        return <Search className="h-4 w-4" />;
      }
      if (toolLower.includes("bash") || toolLower.includes("shell") || toolLower.includes("command")) {
        return <Terminal className="h-4 w-4" />;
      }
      if (toolLower.includes("browse") || toolLower.includes("web")) {
        return <Eye className="h-4 w-4" />;
      }
      if (toolLower.includes("list") || toolLower.includes("dir")) {
        return <FolderOpen className="h-4 w-4" />;
      }
    }

    switch (type) {
      case "file_read":
      case "file_write":
        return <FileText className="h-4 w-4" />;
      case "search":
        return <Search className="h-4 w-4" />;
      case "command":
        return <Terminal className="h-4 w-4" />;
      case "browse":
        return <Eye className="h-4 w-4" />;
      case "thinking":
        return <Activity className="h-4 w-4" />;
      default:
        return <Activity className="h-4 w-4" />;
    }
  };

  const getStatusIcon = (status: ActivityItem["status"]) => {
    switch (status) {
      case "pending":
        return <Clock className="h-3 w-3 text-text-muted" />;
      case "running":
        return <Loader2 className="h-3 w-3 animate-spin text-primary" />;
      case "completed":
        return <CheckCircle2 className="h-3 w-3 text-success" />;
      case "failed":
        return <XCircle className="h-3 w-3 text-error" />;
    }
  };

  const getTypeColor = (type: ActivityItem["type"]) => {
    switch (type) {
      case "file_read":
        return "text-blue-400";
      case "file_write":
        return "text-amber-400";
      case "search":
        return "text-purple-400";
      case "command":
        return "text-green-400";
      case "browse":
        return "text-cyan-400";
      case "thinking":
        return "text-text-muted";
      default:
        return "text-primary";
    }
  };

  const formatTime = (date: Date) => {
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  };

  const formatArgs = (args: Record<string, unknown>) => {
    // Show relevant args in a readable format
    const relevantKeys = ["path", "file", "query", "pattern", "command", "cmd", "url", "content"];
    const entries = Object.entries(args).filter(([key]) => 
      relevantKeys.some(k => key.toLowerCase().includes(k))
    );
    
    if (entries.length === 0) {
      // Show first few args if no relevant ones found
      return Object.entries(args).slice(0, 2).map(([k, v]) => 
        `${k}: ${typeof v === 'string' ? v.slice(0, 100) : JSON.stringify(v).slice(0, 100)}`
      ).join(", ");
    }
    
    return entries.map(([k, v]) => 
      `${k}: ${typeof v === 'string' ? v : JSON.stringify(v).slice(0, 200)}`
    ).join("\n");
  };

  if (!isVisible) {
    return (
      <button
        onClick={onToggle}
        className="fixed bottom-24 right-4 z-50 flex items-center gap-2 rounded-full bg-surface-elevated px-4 py-2 shadow-lg border border-border hover:border-primary/50 transition-colors"
      >
        <Activity className="h-4 w-4 text-primary" />
        <span className="text-sm font-medium">Activity</span>
        {activities.filter(a => a.status === "running").length > 0 && (
          <span className="flex h-2 w-2">
            <span className="absolute inline-flex h-2 w-2 animate-ping rounded-full bg-primary opacity-75" />
            <span className="relative inline-flex h-2 w-2 rounded-full bg-primary" />
          </span>
        )}
      </button>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 20 }}
      className={`fixed right-4 z-50 bg-surface-elevated rounded-xl shadow-2xl border border-border overflow-hidden transition-all ${
        isExpanded 
          ? "bottom-4 top-20 w-[500px]" 
          : "bottom-24 w-80 max-h-96"
      }`}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-surface">
        <div className="flex items-center gap-2">
          <Activity className="h-4 w-4 text-primary" />
          <span className="font-medium text-sm">AI Activity</span>
          {activities.filter(a => a.status === "running").length > 0 && (
            <span className="text-xs text-primary bg-primary/10 px-2 py-0.5 rounded-full">
              {activities.filter(a => a.status === "running").length} running
            </span>
          )}
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={() => setIsExpanded(!isExpanded)}
            className="p-1 hover:bg-surface-elevated rounded transition-colors"
          >
            {isExpanded ? (
              <Minimize2 className="h-4 w-4 text-text-muted" />
            ) : (
              <Maximize2 className="h-4 w-4 text-text-muted" />
            )}
          </button>
          <button
            onClick={onToggle}
            className="p-1 hover:bg-surface-elevated rounded transition-colors"
          >
            <X className="h-4 w-4 text-text-muted" />
          </button>
        </div>
      </div>

      {/* Activity List */}
      <div 
        ref={scrollRef}
        className={`overflow-y-auto ${isExpanded ? "h-[calc(100%-48px)]" : "max-h-72"}`}
      >
        {activities.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-8 text-text-muted">
            <Activity className="h-8 w-8 mb-2 opacity-50" />
            <p className="text-sm">No activity yet</p>
            <p className="text-xs mt-1">AI actions will appear here</p>
          </div>
        ) : (
          <AnimatePresence>
            {activities.map((activity) => (
              <motion.div
                key={activity.id}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -10 }}
                className="border-b border-border/50 last:border-b-0"
              >
                <button
                  onClick={() => toggleItem(activity.id)}
                  className="w-full px-4 py-3 text-left hover:bg-surface/50 transition-colors"
                >
                  <div className="flex items-start gap-3">
                    {/* Icon */}
                    <div className={`mt-0.5 ${getTypeColor(activity.type)}`}>
                      {getIcon(activity.type, activity.tool)}
                    </div>

                    {/* Content */}
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-medium text-sm text-text truncate">
                          {activity.title}
                        </span>
                        {getStatusIcon(activity.status)}
                      </div>
                      {activity.detail && (
                        <p className="text-xs text-text-muted mt-0.5 truncate">
                          {activity.detail}
                        </p>
                      )}
                      <p className="text-xs text-text-subtle mt-1">
                        {formatTime(activity.timestamp)}
                      </p>
                    </div>

                    {/* Expand Icon */}
                    {(activity.args || activity.result) && (
                      <div className="text-text-muted">
                        {expandedItems.has(activity.id) ? (
                          <ChevronDown className="h-4 w-4" />
                        ) : (
                          <ChevronRight className="h-4 w-4" />
                        )}
                      </div>
                    )}
                  </div>
                </button>

                {/* Expanded Details */}
                <AnimatePresence>
                  {expandedItems.has(activity.id) && (activity.args || activity.result) && (
                    <motion.div
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: "auto", opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      className="overflow-hidden"
                    >
                      <div className="px-4 pb-3 pl-11">
                        {activity.args && Object.keys(activity.args).length > 0 && (
                          <div className="mb-2">
                            <p className="text-xs font-medium text-text-muted mb-1">Arguments</p>
                            <pre className="text-xs bg-surface rounded p-2 overflow-x-auto text-text-muted whitespace-pre-wrap">
                              {formatArgs(activity.args)}
                            </pre>
                          </div>
                        )}
                        {activity.result && (
                          <div>
                            <p className="text-xs font-medium text-text-muted mb-1">Result</p>
                            <pre className="text-xs bg-surface rounded p-2 overflow-x-auto text-text-muted max-h-32 whitespace-pre-wrap">
                              {activity.result.slice(0, 500)}
                              {activity.result.length > 500 && "..."}
                            </pre>
                          </div>
                        )}
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </motion.div>
            ))}
          </AnimatePresence>
        )}
      </div>
    </motion.div>
  );
}
