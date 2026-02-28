import React, { useEffect, useState } from "react";
import {
  BrowserRouter as Router,
  Routes,
  Route,
  Navigate,
  Link,
  useNavigate,
  useLocation,
} from "react-router-dom";
import { AuthProvider, useAuth } from "./AuthContext";
import { Login } from "./pages/Login";
import { ProviderSetup } from "./pages/ProviderSetup";
import { ResearchDashboard } from "./pages/ResearchDashboard";
import { SwarmDashboard } from "./pages/SwarmDashboard";
import { TextAdventure } from "./pages/TextAdventure";
import { SecondBrainDashboard } from "./pages/SecondBrainDashboard";
import { ConnectorsDashboard } from "./pages/ConnectorsDashboard";
import { OpsWorkspace } from "./pages/OpsWorkspace";
import { RepoAgentDashboard } from "./pages/RepoAgentDashboard";
import { IncidentTriageDashboard } from "./pages/IncidentTriageDashboard";
import { DataExtractionDashboard } from "./pages/DataExtractionDashboard";
import { TicketTriageDashboard } from "./pages/TicketTriageDashboard";
import { ScheduledWatchDashboard } from "./pages/ScheduledWatchDashboard";
import { ContentCreatorDashboard } from "./pages/ContentCreatorDashboard";
import { HtmlExtractorDashboard } from "./pages/HtmlExtractorDashboard";
import {
  LayoutDashboard,
  Users,
  MessageSquareQuote,
  BrainCircuit,
  LogOut,
  Cable,
  ShieldCheck,
  Settings,
  GitPullRequest,
  FileWarning,
  DatabaseZap,
  Ticket,
  Clock,
  PenTool,
  Code,
  FolderOpen,
  RefreshCw,
  FolderPlus,
  ArrowUp,
  Menu,
  X,
  Bolt,
} from "lucide-react";
import { api, getPortalWorkspaceRoot, setPortalWorkspaceRoot } from "./api";
import { StressLab } from "./pages/StressLab";

const ProtectedRoute = ({ children }: { children: React.ReactNode }) => {
  const { token, isLoading } = useAuth();
  if (isLoading) return <div className="text-white p-8">Loading session...</div>;
  return token ? <>{children}</> : <Navigate to="/" replace />;
};

const ProviderReadyRoute = ({ children }: { children: React.ReactNode }) => {
  const { providerConfigured, providerLoading } = useAuth();
  if (providerLoading) return <div className="text-white p-8">Loading provider config...</div>;
  return providerConfigured ? <>{children}</> : <Navigate to="/setup" replace />;
};

const NavLink = ({
  to,
  icon,
  label,
  color = "text-gray-400",
}: {
  to: string;
  icon: React.ReactNode;
  label: string;
  color?: string;
}) => {
  const { pathname } = useLocation();
  const active = pathname === to || pathname.startsWith(to + "/");
  return (
    <Link
      to={to}
      className={`group flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm font-medium transition-all duration-300 relative overflow-hidden ${
        active ? "text-white shadow-lg" : `${color} hover:text-white hover:bg-white/5`
      }`}
    >
      {active && (
        <div className="absolute inset-0 bg-gradient-to-r from-emerald-500/20 to-teal-500/0 opacity-100" />
      )}
      {active && (
        <div className="absolute left-0 top-0 bottom-0 w-1 bg-emerald-500 rounded-r-full shadow-[0_0_10px_rgba(16,185,129,0.8)]" />
      )}
      <div
        className={`relative z-10 flex items-center gap-3 transition-transform duration-300 ${active ? "translate-x-1" : "group-hover:translate-x-1"}`}
      >
        {icon}
        <span>{label}</span>
      </div>
    </Link>
  );
};

const NavigationLayout = ({ children }: { children: React.ReactNode }) => {
  const { logout } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [showSetupHint, setShowSetupHint] = useState(false);
  const [pendingApprovals, setPendingApprovals] = useState<
    Array<{ id: string; tool: string; sessionID: string }>
  >([]);
  const [permissionRulesCount, setPermissionRulesCount] = useState(0);
  const [approvalError, setApprovalError] = useState<string | null>(null);
  const [approving, setApproving] = useState(false);
  const [autoApproveEnabled, setAutoApproveEnabled] = useState(false);
  const [workspaceInput, setWorkspaceInput] = useState("");
  const [workspaceSaved, setWorkspaceSaved] = useState<string | null>(null);
  const [workspaceDirs, setWorkspaceDirs] = useState<Array<{ name: string; path: string }>>([]);
  const [workspaceDirPath, setWorkspaceDirPath] = useState<string>("");
  const [workspaceParentPath, setWorkspaceParentPath] = useState<string | null>(null);
  const [workspaceBrowseLoading, setWorkspaceBrowseLoading] = useState(false);
  const [workspaceBrowseError, setWorkspaceBrowseError] = useState<string | null>(null);
  const [newDirectoryName, setNewDirectoryName] = useState("");
  const [creatingDirectory, setCreatingDirectory] = useState(false);

  useEffect(() => {
    const key = "tandem_portal_setup_hint_dismissed";
    const autoApproveKey = "tandem_portal_auto_approve_all";
    const existingWorkspace = getPortalWorkspaceRoot();
    if (existingWorkspace) {
      setWorkspaceInput(existingWorkspace);
    }
    setAutoApproveEnabled(localStorage.getItem(autoApproveKey) === "1");
    if (!localStorage.getItem(key)) {
      setShowSetupHint(true);
    }
  }, []);

  useEffect(() => {
    localStorage.setItem("tandem_portal_auto_approve_all", autoApproveEnabled ? "1" : "0");
  }, [autoApproveEnabled]);

  useEffect(() => {
    setMobileNavOpen(false);
  }, [location.pathname]);

  const loadWorkspaceDirectories = async (targetPath?: string) => {
    setWorkspaceBrowseLoading(true);
    setWorkspaceBrowseError(null);
    try {
      const response = await api.listPortalDirectories(targetPath);
      setWorkspaceDirs(response.directories || []);
      setWorkspaceDirPath(response.current || "");
      setWorkspaceParentPath(response.parent || null);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setWorkspaceBrowseError(message);
    } finally {
      setWorkspaceBrowseLoading(false);
    }
  };

  useEffect(() => {
    const initial = getPortalWorkspaceRoot() || undefined;
    void loadWorkspaceDirectories(initial);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let stopped = false;
    const getReqStatus = (req: { status?: unknown }) =>
      String(req.status || "")
        .trim()
        .toLowerCase();
    const isPendingReq = (req: { status?: unknown }) => {
      const status = getReqStatus(req);
      return status === "pending" || status === "asked" || status === "waiting";
    };
    const refresh = async () => {
      try {
        const snapshot = await api.listPermissions();
        const pending = (snapshot.requests || [])
          .filter((req) => isPendingReq(req))
          .map((req) => ({
            id: req.id,
            tool: req.tool || req.permission || "tool",
            sessionID: String(req.sessionID || req.sessionId || req.session_id || "unknown"),
          }));
        if (!stopped) {
          setPendingApprovals(pending);
          setPermissionRulesCount((snapshot.rules || []).length);
          setApprovalError(null);
        }
      } catch (error) {
        if (!stopped) {
          const message = error instanceof Error ? error.message : String(error);
          setApprovalError(message);
        }
      }
    };

    void refresh();
    const interval = window.setInterval(() => {
      void refresh();
    }, 5000);
    return () => {
      stopped = true;
      window.clearInterval(interval);
    };
  }, []);

  const dismissSetupHint = () => {
    localStorage.setItem("tandem_portal_setup_hint_dismissed", "1");
    setShowSetupHint(false);
  };

  const approveAllPending = async () => {
    if (pendingApprovals.length === 0 || approving) return;
    setApproving(true);
    setApprovalError(null);
    try {
      for (const req of pendingApprovals) {
        // Use persistent approval so repeated tool calls don't re-block later turns.
        await api.replyPermission(req.id, "always");
      }
      const snapshot = await api.listPermissions();
      const pending = (snapshot.requests || [])
        .filter((req) => {
          const status = String(req.status || "")
            .trim()
            .toLowerCase();
          return status === "pending" || status === "asked" || status === "waiting";
        })
        .map((req) => ({
          id: req.id,
          tool: req.tool || req.permission || "tool",
          sessionID: String(req.sessionID || req.sessionId || req.session_id || "unknown"),
        }));
      setPendingApprovals(pending);
      setPermissionRulesCount((snapshot.rules || []).length);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setApprovalError(message);
    } finally {
      setApproving(false);
    }
  };

  useEffect(() => {
    if (!autoApproveEnabled || approving || pendingApprovals.length === 0) return;
    void approveAllPending();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoApproveEnabled, pendingApprovals.length, approving]);

  const saveWorkspaceRoot = () => {
    const trimmed = workspaceInput.trim();
    if (!trimmed) {
      setPortalWorkspaceRoot(null);
      setWorkspaceSaved("Workspace cleared. New sessions will use engine default directory.");
      return;
    }
    setPortalWorkspaceRoot(trimmed);
    setWorkspaceSaved(`Workspace set for new sessions: ${trimmed}`);
  };

  const createDirectory = async () => {
    const name = newDirectoryName.trim();
    if (!name || creatingDirectory) return;
    setCreatingDirectory(true);
    setWorkspaceBrowseError(null);
    try {
      const created = await api.createPortalDirectory({
        parentPath: workspaceDirPath || workspaceInput || undefined,
        name,
      });
      setNewDirectoryName("");
      setWorkspaceInput(created.path);
      setWorkspaceSaved(`Created directory: ${created.path}`);
      await loadWorkspaceDirectories(created.parentPath || workspaceDirPath || undefined);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setWorkspaceBrowseError(message);
    } finally {
      setCreatingDirectory(false);
    }
  };

  const sidebarContent = (showBrand = true) => (
    <>
      {showBrand && (
        <div className="p-5 border-b border-white/5 bg-black/40 backdrop-blur-md relative overflow-hidden">
          <div className="absolute top-0 right-0 w-32 h-32 bg-emerald-500/10 blur-[50px] rounded-full -translate-y-1/2 translate-x-1/2"></div>
          <h1 className="text-xl font-bold text-white flex items-center gap-2 relative z-10 tracking-tight">
            <div className="p-1.5 bg-emerald-500/20 rounded-lg border border-emerald-500/30 shadow-[0_0_15px_rgba(16,185,129,0.3)]">
              <BrainCircuit className="text-emerald-400" size={20} />
            </div>
            Tandem Portal
          </h1>
        </div>
      )}
      <nav className="flex-1 p-4 space-y-1 overflow-y-auto custom-scrollbar">
        <div className="mb-4 rounded-xl border border-white/5 bg-white/[0.02] p-4 shadow-inner relative overflow-hidden group">
          <div className="absolute inset-0 bg-gradient-to-br from-gray-800/50 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"></div>
          <p className="text-[11px] font-medium tracking-wider text-gray-400 flex items-center gap-1.5 uppercase mb-2 relative z-10">
            <FolderOpen size={12} />
            Workspace Root
          </p>
          <input
            type="text"
            value={workspaceInput}
            onChange={(e) => {
              setWorkspaceSaved(null);
              setWorkspaceInput(e.target.value);
            }}
            placeholder="/home/user/projects/my-repo"
            className="mt-2 w-full rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200 placeholder:text-gray-500 focus:outline-none focus:ring-1 focus:ring-emerald-500"
          />
          <div className="mt-2 flex items-center justify-between gap-2">
            <button
              type="button"
              onClick={saveWorkspaceRoot}
              className="rounded border border-gray-700 px-2 py-1 text-xs text-gray-300 hover:text-white hover:bg-gray-800"
            >
              Save Root
            </button>
            <button
              type="button"
              onClick={() => {
                setWorkspaceInput("");
                setPortalWorkspaceRoot(null);
                setWorkspaceSaved(
                  "Workspace cleared. New sessions will use engine default directory."
                );
              }}
              className="rounded border border-gray-700 px-2 py-1 text-xs text-gray-400 hover:text-white hover:bg-gray-800"
            >
              Clear
            </button>
          </div>
          <div className="mt-2 rounded border border-gray-800 bg-gray-900/70 p-2">
            <div className="flex items-center justify-between gap-2 mb-2">
              <p className="text-[10px] text-gray-400">Available directories on this machine</p>
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  onClick={() => void loadWorkspaceDirectories(workspaceParentPath || undefined)}
                  disabled={!workspaceParentPath || workspaceBrowseLoading}
                  className="rounded border border-gray-700 px-1.5 py-1 text-[10px] text-gray-300 hover:text-white hover:bg-gray-800 disabled:opacity-40"
                  title="Go to parent directory"
                >
                  <ArrowUp size={12} />
                </button>
                <button
                  type="button"
                  onClick={() => void loadWorkspaceDirectories(workspaceDirPath || undefined)}
                  disabled={workspaceBrowseLoading}
                  className="rounded border border-gray-700 px-1.5 py-1 text-[10px] text-gray-300 hover:text-white hover:bg-gray-800 disabled:opacity-40"
                  title="Refresh directory list"
                >
                  <RefreshCw size={12} className={workspaceBrowseLoading ? "animate-spin" : ""} />
                </button>
              </div>
            </div>
            <p className="text-[10px] text-gray-500 break-all mb-2">
              Browsing: <span className="text-gray-300">{workspaceDirPath || "(loading...)"}</span>
            </p>
            <div className="max-h-28 overflow-y-auto space-y-1 pr-1">
              {workspaceBrowseLoading && (
                <p className="text-[10px] text-gray-500">Loading directories...</p>
              )}
              {!workspaceBrowseLoading && workspaceDirs.length === 0 && (
                <p className="text-[10px] text-gray-500">No child directories found.</p>
              )}
              {!workspaceBrowseLoading &&
                workspaceDirs.map((entry) => (
                  <button
                    key={entry.path}
                    type="button"
                    onClick={() => {
                      setWorkspaceInput(entry.path);
                      setWorkspaceSaved(null);
                      void loadWorkspaceDirectories(entry.path);
                    }}
                    className="w-full text-left rounded border border-gray-800 px-2 py-1 text-[10px] text-gray-300 hover:text-white hover:bg-gray-800"
                    title={entry.path}
                  >
                    {entry.name}
                  </button>
                ))}
            </div>
            <div className="mt-2 flex items-center gap-1">
              <input
                type="text"
                value={newDirectoryName}
                onChange={(e) => setNewDirectoryName(e.target.value)}
                placeholder="new-folder"
                className="flex-1 rounded border border-gray-700 bg-gray-900 px-2 py-1 text-[10px] text-gray-200 placeholder:text-gray-500 focus:outline-none focus:ring-1 focus:ring-emerald-500"
              />
              <button
                type="button"
                onClick={() => void createDirectory()}
                disabled={!newDirectoryName.trim() || creatingDirectory}
                className="rounded border border-gray-700 px-2 py-1 text-[10px] text-gray-300 hover:text-white hover:bg-gray-800 disabled:opacity-40"
                title="Create directory"
              >
                <FolderPlus size={12} />
              </button>
            </div>
            {workspaceBrowseError && (
              <p className="mt-1 text-[10px] text-rose-300 break-all">{workspaceBrowseError}</p>
            )}
          </div>
          <p className="mt-2 text-[10px] text-gray-500">
            Applies to new sessions. Use absolute paths (no `~`).
          </p>
          <p className="mt-1 text-[10px] text-gray-400 break-all">
            Current:{" "}
            <span className="text-gray-300">
              {workspaceInput.trim().length > 0 ? workspaceInput.trim() : "(engine default)"}
            </span>
          </p>
          {workspaceSaved && <p className="mt-1 text-[10px] text-emerald-300">{workspaceSaved}</p>}
        </div>
        <NavLink
          to="/setup"
          icon={<Settings size={18} />}
          label="Provider Setup"
          color="text-gray-400"
        />
        <NavLink
          to="/research"
          icon={<LayoutDashboard size={18} />}
          label="Research"
          color="text-blue-400"
        />
        <NavLink
          to="/repo"
          icon={<GitPullRequest size={18} />}
          label="Repo Agent"
          color="text-indigo-400"
        />
        <NavLink
          to="/triage"
          icon={<FileWarning size={18} />}
          label="Incident Triage"
          color="text-orange-400"
        />
        <NavLink
          to="/data"
          icon={<DatabaseZap size={18} />}
          label="Data Extraction"
          color="text-emerald-400"
        />
        <NavLink
          to="/tickets"
          icon={<Ticket size={18} />}
          label="Ticket Triage"
          color="text-cyan-400"
        />
        <NavLink
          to="/watch"
          icon={<Clock size={18} />}
          label="Scheduled Watch"
          color="text-violet-400"
        />
        <NavLink
          to="/content"
          icon={<PenTool size={18} />}
          label="Content Creator"
          color="text-fuchsia-400"
        />
        <NavLink
          to="/html"
          icon={<Code size={18} />}
          label="HTML Escape-Hatch"
          color="text-rose-400"
        />
        <NavLink
          to="/swarm"
          icon={<Users size={18} />}
          label="Agent Swarm"
          color="text-purple-400"
        />
        <NavLink
          to="/adventure"
          icon={<MessageSquareQuote size={18} />}
          label="Adventure"
          color="text-yellow-400"
        />
        <NavLink
          to="/second-brain"
          icon={<BrainCircuit size={18} />}
          label="Second Brain"
          color="text-teal-400"
        />
        <NavLink
          to="/channels"
          icon={<Cable size={18} />}
          label="Connectors"
          color="text-pink-400"
        />
        <NavLink to="/stress" icon={<Bolt size={18} />} label="Stress Lab" color="text-red-400" />
        <NavLink to="/ops" icon={<ShieldCheck size={18} />} label="Ops" color="text-emerald-400" />
      </nav>
      <div className="p-4 border-t border-white/5 bg-black/20">
        <button
          onClick={logout}
          className="flex items-center justify-center gap-2 text-gray-400 hover:text-white hover:bg-white/5 w-full p-2.5 rounded-xl transition-all duration-300 font-medium text-sm group"
        >
          <LogOut size={18} className="group-hover:-translate-x-1 transition-transform" />{" "}
          Disconnect
        </button>
      </div>
    </>
  );

  return (
    <div className="flex h-[100dvh] bg-transparent overflow-hidden selection:bg-emerald-500/30">
      <div className="hidden lg:flex w-72 bg-gray-950/80 backdrop-blur-xl border-r border-white/5 flex-col shadow-2xl z-20">
        {sidebarContent(true)}
      </div>
      {mobileNavOpen && (
        <div className="fixed inset-0 z-50 lg:hidden">
          <button
            type="button"
            className="absolute inset-0 bg-black/60"
            onClick={() => setMobileNavOpen(false)}
            aria-label="Close navigation menu"
          />
          <div className="absolute left-0 top-0 h-full w-[88vw] max-w-sm bg-gray-900 border-r border-gray-800 flex flex-col">
            <div className="flex items-center justify-between p-4 border-b border-gray-800">
              <h2 className="text-white font-semibold flex items-center gap-2">
                <BrainCircuit className="text-emerald-500" size={18} /> Tandem Portal
              </h2>
              <button
                type="button"
                onClick={() => setMobileNavOpen(false)}
                className="rounded border border-gray-700 p-1 text-gray-300"
              >
                <X size={18} />
              </button>
            </div>
            {sidebarContent(false)}
          </div>
        </div>
      )}

      <div className="flex-1 min-w-0 flex flex-col relative z-10 backdrop-blur-3xl bg-black/20">
        <div className="lg:hidden border-b border-white/5 bg-gray-950/80 backdrop-blur-xl px-3 py-3 flex items-center justify-between shadow-sm">
          <button
            type="button"
            onClick={() => setMobileNavOpen(true)}
            className="rounded-lg border border-white/10 p-1.5 text-gray-300 hover:bg-white/5 hover:text-white transition-colors"
          >
            <Menu size={20} />
          </button>
          <span className="text-sm font-bold text-white tracking-tight flex items-center gap-2">
            <BrainCircuit size={16} className="text-emerald-400" /> Tandem
          </span>
          <span className="text-[11px] font-medium px-2 py-0.5 rounded-full bg-white/5 text-gray-400 border border-white/5">
            Pending: {pendingApprovals.length}
          </span>
        </div>
        <div className="flex-1 min-h-0 overflow-auto pb-20 lg:pb-0 custom-scrollbar">
          {children}
        </div>
      </div>

      <div className="fixed right-3 bottom-20 lg:right-6 lg:top-6 lg:bottom-auto z-50 flex items-center gap-3 rounded-2xl border border-white/10 bg-gray-950/80 backdrop-blur-xl px-3 py-2.5 shadow-[0_8px_30px_rgb(0,0,0,0.5)] transition-all">
        <span className="text-xs text-gray-400 hidden sm:inline">Approvals</span>
        <span
          className={`text-xs font-medium ${
            pendingApprovals.length > 0 ? "text-amber-300" : "text-gray-500"
          }`}
        >
          {pendingApprovals.length}
        </span>
        <button
          type="button"
          onClick={() => void approveAllPending()}
          disabled={pendingApprovals.length === 0 || approving}
          className="rounded border border-gray-700 px-2 py-1 text-xs text-gray-300 hover:text-white hover:bg-gray-800 disabled:opacity-40 disabled:cursor-not-allowed"
          title="Approve all currently pending requests (always)"
        >
          {approving ? "..." : "Allow All"}
        </button>
        <button
          type="button"
          onClick={() => setAutoApproveEnabled((prev) => !prev)}
          className={`rounded border px-2 py-1 text-xs ${
            autoApproveEnabled
              ? "border-emerald-600 bg-emerald-600/20 text-emerald-300"
              : "border-gray-700 text-gray-300 hover:text-white hover:bg-gray-800"
          }`}
          title="Automatically approve new permission prompts (always)"
        >
          {autoApproveEnabled ? "Auto: ON" : "Auto: OFF"}
        </button>
      </div>

      <div className="hidden lg:block fixed right-4 bottom-4 z-40 w-80 rounded-lg border border-gray-800 bg-gray-900/95 shadow-xl">
        <div className="flex items-center justify-between px-3 py-2 border-b border-gray-800">
          <p className="text-xs text-gray-300 tracking-wide">PENDING APPROVALS</p>
          <span
            className={`text-xs font-medium ${
              pendingApprovals.length > 0 ? "text-amber-300" : "text-gray-500"
            }`}
          >
            {pendingApprovals.length}
          </span>
        </div>
        <div className="px-3 py-2 max-h-36 overflow-y-auto space-y-1">
          {approvalError ? (
            <p className="text-[11px] text-red-300">{approvalError}</p>
          ) : pendingApprovals.length === 0 ? (
            <p className="text-[11px] text-gray-500">
              No pending permission prompts. Active rules: {permissionRulesCount}.
            </p>
          ) : (
            pendingApprovals.slice(0, 8).map((req) => (
              <p key={req.id} className="text-[11px] text-gray-300 font-mono">
                <span className="text-amber-300">{req.tool}</span>{" "}
                <span className="text-gray-500">[{req.sessionID.slice(0, 8)}]</span>
              </p>
            ))
          )}
        </div>
        <div className="px-3 py-2 border-t border-gray-800 flex items-center justify-end">
          <button
            type="button"
            onClick={() => void approveAllPending()}
            disabled={pendingApprovals.length === 0 || approving}
            className="rounded border border-gray-700 px-2 py-1 text-xs text-gray-300 hover:text-white hover:bg-gray-800 disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {approving ? "Approving..." : "Approve All (Always)"}
          </button>
        </div>
      </div>
      {showSetupHint && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
          <div className="w-full max-w-md rounded-xl border border-gray-700 bg-gray-900 p-5 shadow-2xl">
            <div className="flex items-center gap-2 text-white">
              <Settings className="text-emerald-500" size={20} />
              <h3 className="text-lg font-semibold">Provider Setup</h3>
            </div>
            <p className="mt-3 text-sm text-gray-300">
              Configure your default provider and model from{" "}
              <span className="font-medium">Provider Setup</span> so example runs use the expected
              model every time.
            </p>
            <div className="mt-4 flex items-center justify-end gap-2">
              <button
                onClick={dismissSetupHint}
                className="rounded-md border border-gray-700 px-3 py-2 text-sm text-gray-300 hover:bg-gray-800"
              >
                Dismiss
              </button>
              <button
                onClick={() => {
                  dismissSetupHint();
                  navigate("/setup");
                }}
                className="rounded-md bg-emerald-600 px-3 py-2 text-sm font-medium text-white hover:bg-emerald-500"
              >
                Open Setup
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default function App() {
  return (
    <AuthProvider>
      <Router>
        <Routes>
          <Route path="/" element={<Login />} />
          <Route
            path="/setup"
            element={
              <ProtectedRoute>
                <ProviderSetup />
              </ProtectedRoute>
            }
          />

          <Route
            path="/research"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <ResearchDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/repo"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <RepoAgentDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/triage"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <IncidentTriageDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/data"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <DataExtractionDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/tickets"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <TicketTriageDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/watch"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <ScheduledWatchDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/content"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <ContentCreatorDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/html"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <HtmlExtractorDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/swarm"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <SwarmDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/adventure"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <TextAdventure />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/second-brain"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <SecondBrainDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/ops"
            element={
              <ProtectedRoute>
                <NavigationLayout>
                  <OpsWorkspace />
                </NavigationLayout>
              </ProtectedRoute>
            }
          />
          <Route
            path="/channels"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <ConnectorsDashboard />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
          <Route
            path="/stress"
            element={
              <ProtectedRoute>
                <ProviderReadyRoute>
                  <NavigationLayout>
                    <StressLab />
                  </NavigationLayout>
                </ProviderReadyRoute>
              </ProtectedRoute>
            }
          />
        </Routes>
      </Router>
    </AuthProvider>
  );
}
