import { motion, AnimatePresence } from "framer-motion";
import { Button } from "@/components/ui";
import { Shield, FileText, FolderOpen, Terminal, Globe, AlertTriangle, X } from "lucide-react";
import { cn } from "@/lib/utils";

export interface PermissionRequest {
  id: string;
  session_id: string;
  type:
    | "read_file"
    | "write_file"
    | "create_file"
    | "delete_file"
    | "run_command"
    | "fetch_url"
    | "list_directory";
  path?: string;
  url?: string;
  command?: string;
  reasoning: string;
  diff?: {
    before: string;
    after: string;
  };
  riskLevel: "low" | "medium" | "high";
  // Metadata for journaling/undo. Not required by the toast UI.
  tool?: string;
  args?: Record<string, unknown>;
  messageId?: string;
}

interface PermissionToastProps {
  request: PermissionRequest;
  onApprove: (remember?: "once" | "session" | "always") => void;
  onDeny: (remember?: boolean) => void;
}

export function PermissionToast({ request, onApprove, onDeny }: PermissionToastProps) {
  const toolLabel = request.tool ? request.tool.replace(/_/g, " ") : null;

  const getIcon = () => {
    switch (request.type) {
      case "read_file":
      case "write_file":
      case "create_file":
      case "delete_file":
        return <FileText className="h-5 w-5" />;
      case "list_directory":
        return <FolderOpen className="h-5 w-5" />;
      case "run_command":
        return <Terminal className="h-5 w-5" />;
      case "fetch_url":
        return <Globe className="h-5 w-5" />;
      default:
        return <Shield className="h-5 w-5" />;
    }
  };

  const getTitle = () => {
    switch (request.type) {
      case "read_file":
        return "Read File";
      case "write_file":
        return "Modify File";
      case "create_file":
        return "Create File";
      case "delete_file":
        return "Delete File";
      case "list_directory":
        return "List Directory";
      case "run_command":
        return "Run Command";
      case "fetch_url":
        return "Fetch URL";
      default:
        return "Permission Request";
    }
  };

  const getRiskColor = () => {
    switch (request.riskLevel) {
      case "low":
        return "text-success border-success/30 bg-success/10";
      case "medium":
        return "text-warning border-warning/30 bg-warning/10";
      case "high":
        return "text-error border-error/30 bg-error/10";
    }
  };

  return (
    <motion.div
      className="fixed bottom-24 right-6 z-50 w-96 overflow-hidden rounded-xl border border-border bg-surface shadow-2xl"
      initial={{ opacity: 0, y: 50, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 50, scale: 0.95 }}
      transition={{ type: "spring", damping: 25, stiffness: 300 }}
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-border bg-surface-elevated px-4 py-3">
        <div className="flex items-center gap-3">
          <div className={cn("rounded-lg border p-2", getRiskColor())}>{getIcon()}</div>
          <div>
            <h3 className="font-semibold text-text">{getTitle()}</h3>
            <p className="text-xs text-text-subtle">
              {toolLabel ? `Tool request: ${toolLabel}` : "Tandem requests your approval"}
            </p>
          </div>
        </div>
        <button
          onClick={() => onDeny(false)}
          className="rounded-lg p-1 text-text-subtle hover:bg-surface hover:text-text"
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      {/* Content */}
      <div className="p-4 space-y-4">
        {/* Path/URL/Command */}
        {request.path && (
          <div className="rounded-lg bg-surface-elevated p-3">
            <p className="text-xs text-text-subtle mb-1">Path</p>
            <p className="font-mono text-sm text-text break-all">{request.path}</p>
          </div>
        )}
        {request.url && (
          <div className="rounded-lg bg-surface-elevated p-3">
            <p className="text-xs text-text-subtle mb-1">URL</p>
            <p className="font-mono text-sm text-text break-all">{request.url}</p>
          </div>
        )}
        {request.command && (
          <div className="rounded-lg bg-surface-elevated p-3">
            <p className="text-xs text-text-subtle mb-1">Command</p>
            <p className="font-mono text-sm text-text">{request.command}</p>
          </div>
        )}

        {/* Reasoning */}
        <div>
          <p className="text-xs text-text-subtle mb-1">Reason</p>
          <p className="text-sm text-text">{request.reasoning}</p>
        </div>

        {/* Diff preview for write operations */}
        {request.diff && (
          <div className="rounded-lg border border-border overflow-hidden">
            <div className="bg-surface-elevated px-3 py-2 text-xs font-medium text-text-subtle">
              Changes Preview
            </div>
            <div className="max-h-40 overflow-auto bg-background p-3">
              <pre className="font-mono text-xs">
                {request.diff.before && <div className="text-error">- {request.diff.before}</div>}
                <div className="text-success">+ {request.diff.after}</div>
              </pre>
            </div>
          </div>
        )}

        {/* Risk warning for high-risk operations */}
        {request.riskLevel === "high" && (
          <div className="flex items-start gap-2 rounded-lg border border-error/30 bg-error/10 p-3">
            <AlertTriangle className="h-4 w-4 flex-shrink-0 text-error mt-0.5" />
            <p className="text-xs text-error">
              This is a high-risk operation. Please review carefully before approving.
            </p>
          </div>
        )}
      </div>

      {/* Actions */}
      <div className="border-t border-border bg-surface-elevated p-4">
        <div className="flex items-center justify-between gap-3">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onDeny(true)}
            className="text-text-muted"
          >
            Deny Always
          </Button>
          <div className="flex gap-2">
            <Button variant="secondary" size="sm" onClick={() => onApprove("once")}>
              Allow Once
            </Button>
            <Button size="sm" onClick={() => onApprove("session")}>
              Allow
            </Button>
          </div>
        </div>
      </div>
    </motion.div>
  );
}

interface PermissionToastContainerProps {
  requests: PermissionRequest[];
  onApprove: (id: string, remember?: "once" | "session" | "always") => void;
  onDeny: (id: string, remember?: boolean) => void;
}

export function PermissionToastContainer({
  requests,
  onApprove,
  onDeny,
}: PermissionToastContainerProps) {
  return (
    <AnimatePresence>
      {requests.map((request) => (
        <PermissionToast
          key={request.id}
          request={request}
          onApprove={(remember) => onApprove(request.id, remember)}
          onDeny={(remember) => onDeny(request.id, remember)}
        />
      ))}
    </AnimatePresence>
  );
}
