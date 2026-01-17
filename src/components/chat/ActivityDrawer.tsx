import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  ChevronUp,
  ChevronDown,
  FileText,
  Search,
  FolderOpen,
  Terminal,
  Eye,
  Activity,
  Clock,
  CheckCircle2,
  XCircle,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";

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

interface ActivityDrawerProps {
  activities: ActivityItem[];
  isGenerating: boolean;
}

export function ActivityDrawer({ activities, isGenerating }: ActivityDrawerProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const [expandedItems, setExpandedItems] = useState<Set<string>>(new Set());
  const scrollRef = useRef<HTMLDivElement>(null);

  // Auto-expand when there's activity
  useEffect(() => {
    const hasRunning = activities.some((a) => a.status === "running");
    if (hasRunning && !isExpanded) {
      setIsExpanded(true);
    }
  }, [activities]);

  // Auto-scroll to bottom when new activities arrive
  useEffect(() => {
    if (scrollRef.current && isExpanded) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [activities, isExpanded]);

  const runningCount = activities.filter((a) => a.status === "running").length;
  const recentActivities = activities.slice(-20); // Show last 20 activities

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
    if (tool) {
      const toolLower = tool.toLowerCase();
      if (toolLower.includes("read") || toolLower.includes("file")) {
        return <FileText className="h-3.5 w-3.5" />;
      }
      if (toolLower.includes("search") || toolLower.includes("grep") || toolLower.includes("find")) {
        return <Search className="h-3.5 w-3.5" />;
      }
      if (toolLower.includes("bash") || toolLower.includes("shell") || toolLower.includes("command")) {
        return <Terminal className="h-3.5 w-3.5" />;
      }
      if (toolLower.includes("browse") || toolLower.includes("web")) {
        return <Eye className="h-3.5 w-3.5" />;
      }
      if (toolLower.includes("list") || toolLower.includes("dir")) {
        return <FolderOpen className="h-3.5 w-3.5" />;
      }
    }

    switch (type) {
      case "file_read":
      case "file_write":
        return <FileText className="h-3.5 w-3.5" />;
      case "search":
        return <Search className="h-3.5 w-3.5" />;
      case "command":
        return <Terminal className="h-3.5 w-3.5" />;
      case "browse":
        return <Eye className="h-3.5 w-3.5" />;
      default:
        return <Activity className="h-3.5 w-3.5" />;
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
      default:
        return "text-primary";
    }
  };

  const formatTime = (date: Date) => {
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  };

  // Don't show if no activities and not generating
  if (activities.length === 0 && !isGenerating) {
    return null;
  }

  return (
    <motion.div
      className="absolute bottom-0 left-0 right-0 bg-surface-elevated border-t border-border shadow-lg z-20"
      initial={{ y: "100%" }}
      animate={{ 
        y: 0,
        height: isExpanded ? 200 : 40 
      }}
      transition={{ type: "spring", damping: 25, stiffness: 300 }}
    >
      {/* Header / Collapsed Bar */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center justify-between px-4 h-10 hover:bg-surface transition-colors"
      >
        <div className="flex items-center gap-3">
          <Activity className="h-4 w-4 text-primary" />
          <span className="text-sm font-medium text-text">AI Activity</span>
          {runningCount > 0 && (
            <span className="flex items-center gap-1 text-xs text-primary bg-primary/10 px-2 py-0.5 rounded-full">
              <Loader2 className="h-3 w-3 animate-spin" />
              {runningCount} running
            </span>
          )}
          {!isExpanded && recentActivities.length > 0 && (
            <span className="text-xs text-text-muted truncate max-w-[200px]">
              {recentActivities[recentActivities.length - 1]?.title}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <span className="text-xs text-text-subtle">{activities.length} actions</span>
          {isExpanded ? (
            <ChevronDown className="h-4 w-4 text-text-muted" />
          ) : (
            <ChevronUp className="h-4 w-4 text-text-muted" />
          )}
        </div>
      </button>

      {/* Expanded Content */}
      {isExpanded && (
        <div
          ref={scrollRef}
          className="h-[160px] overflow-y-auto border-t border-border/50"
        >
          {recentActivities.length === 0 ? (
            <div className="flex items-center justify-center h-full text-text-muted">
              <p className="text-sm">Waiting for AI actions...</p>
            </div>
          ) : (
            <div className="divide-y divide-border/30">
              {recentActivities.map((activity) => (
                <div
                  key={activity.id}
                  className="px-4 py-2 hover:bg-surface/50 transition-colors"
                >
                  <div className="flex items-start gap-3">
                    {/* Icon */}
                    <div className={cn("mt-0.5", getTypeColor(activity.type))}>
                      {getIcon(activity.type, activity.tool)}
                    </div>

                    {/* Content */}
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm text-text truncate flex-1">
                          {activity.title}
                        </span>
                        {getStatusIcon(activity.status)}
                        <span className="text-xs text-text-subtle">
                          {formatTime(activity.timestamp)}
                        </span>
                      </div>
                      {activity.detail && (
                        <p className="text-xs text-text-muted mt-0.5 truncate">
                          {activity.detail}
                        </p>
                      )}
                      
                      {/* Expandable result */}
                      {activity.result && (
                        <button
                          onClick={() => toggleItem(activity.id)}
                          className="text-xs text-primary hover:underline mt-1"
                        >
                          {expandedItems.has(activity.id) ? "Hide result" : "Show result"}
                        </button>
                      )}
                      
                      <AnimatePresence>
                        {expandedItems.has(activity.id) && activity.result && (
                          <motion.pre
                            initial={{ height: 0, opacity: 0 }}
                            animate={{ height: "auto", opacity: 1 }}
                            exit={{ height: 0, opacity: 0 }}
                            className="text-xs bg-surface rounded p-2 mt-1 overflow-x-auto text-text-muted max-h-24 whitespace-pre-wrap"
                          >
                            {activity.result.slice(0, 500)}
                            {activity.result.length > 500 && "..."}
                          </motion.pre>
                        )}
                      </AnimatePresence>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </motion.div>
  );
}
