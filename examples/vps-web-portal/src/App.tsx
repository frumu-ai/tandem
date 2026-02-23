import React, { useEffect, useState } from "react";
import {
  BrowserRouter as Router,
  Routes,
  Route,
  Navigate,
  Link,
  useNavigate,
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
} from "lucide-react";
import { api, getPortalWorkspaceRoot, setPortalWorkspaceRoot } from "./api";

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

const NavigationLayout = ({ children }: { children: React.ReactNode }) => {
  const { logout } = useAuth();
  const navigate = useNavigate();
  const [showSetupHint, setShowSetupHint] = useState(false);
  const [pendingApprovals, setPendingApprovals] = useState<
    Array<{ id: string; tool: string; sessionID: string }>
  >([]);
  const [permissionRulesCount, setPermissionRulesCount] = useState(0);
  const [approvalError, setApprovalError] = useState<string | null>(null);
  const [approving, setApproving] = useState(false);
  const [workspaceInput, setWorkspaceInput] = useState("");
  const [workspaceSaved, setWorkspaceSaved] = useState<string | null>(null);

  useEffect(() => {
    const key = "tandem_portal_setup_hint_dismissed";
    const existingWorkspace = getPortalWorkspaceRoot();
    if (existingWorkspace) {
      setWorkspaceInput(existingWorkspace);
    }
    if (!localStorage.getItem(key)) {
      setShowSetupHint(true);
    }
  }, []);

  useEffect(() => {
    let stopped = false;
    const refresh = async () => {
      try {
        const snapshot = await api.listPermissions();
        const pending = (snapshot.requests || [])
          .filter((req) => req.status === "pending")
          .map((req) => ({
            id: req.id,
            tool: req.tool || req.permission || "tool",
            sessionID: String(req.sessionID || req.session_id || "unknown"),
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
        // `allow` is one-shot; keeps demos explicit while unblocking current run.
        await api.replyPermission(req.id, "allow");
      }
      const snapshot = await api.listPermissions();
      const pending = (snapshot.requests || [])
        .filter((req) => req.status === "pending")
        .map((req) => ({
          id: req.id,
          tool: req.tool || req.permission || "tool",
          sessionID: String(req.sessionID || req.session_id || "unknown"),
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

  return (
    <div className="flex h-screen bg-gray-950">
      {/* Sidebar */}
      <div className="w-64 bg-gray-900 border-r border-gray-800 flex flex-col">
        <div className="p-4 border-b border-gray-800">
          <h1 className="text-xl font-bold text-white flex items-center gap-2">
            <BrainCircuit className="text-emerald-500" />
            Tandem Portal
          </h1>
        </div>
        <nav className="flex-1 p-4 space-y-2 overflow-y-auto">
          <div className="mb-3 rounded-md border border-gray-800 bg-gray-950/60 p-3">
            <p className="text-[11px] tracking-wide text-gray-400 flex items-center gap-1">
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
            <p className="mt-2 text-[10px] text-gray-500">
              Applies to new sessions. Use absolute paths (no `~`).
            </p>
            <p className="mt-1 text-[10px] text-gray-400 break-all">
              Current:{" "}
              <span className="text-gray-300">
                {workspaceInput.trim().length > 0 ? workspaceInput.trim() : "(engine default)"}
              </span>
            </p>
            {workspaceSaved && (
              <p className="mt-1 text-[10px] text-emerald-300">{workspaceSaved}</p>
            )}
          </div>
          <Link
            to="/setup"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <Settings size={20} /> Provider Setup
          </Link>
          <Link
            to="/research"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <LayoutDashboard size={20} /> Research
          </Link>
          <Link
            to="/repo"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <GitPullRequest size={20} /> Repo Agent
          </Link>
          <Link
            to="/triage"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <FileWarning size={20} /> Incident Triage
          </Link>
          <Link
            to="/data"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <DatabaseZap size={20} /> Data Extraction
          </Link>
          <Link
            to="/tickets"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <Ticket size={20} /> Ticket Triage
          </Link>
          <Link
            to="/watch"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <Clock size={20} /> Scheduled Watch
          </Link>
          <Link
            to="/content"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <PenTool size={20} /> Content Creator
          </Link>
          <Link
            to="/html"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <Code size={20} /> HTML Escape-Hatch
          </Link>
          <Link
            to="/swarm"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <Users size={20} /> Agent Swarm
          </Link>
          <Link
            to="/adventure"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <MessageSquareQuote size={20} /> Adventure
          </Link>
          <Link
            to="/second-brain"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <BrainCircuit size={20} /> Second Brain
          </Link>
          <Link
            to="/channels"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <Cable size={20} /> Connectors
          </Link>
          <Link
            to="/ops"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <ShieldCheck size={20} /> Ops
          </Link>
        </nav>
        <div className="p-4 border-t border-gray-800">
          <button
            onClick={logout}
            className="flex items-center gap-3 text-gray-400 hover:text-white w-full p-2 rounded-md"
          >
            <LogOut size={20} /> Disconnect
          </button>
        </div>
      </div>

      {/* Main Content */}
      <div className="flex-1 overflow-auto">{children}</div>

      <div className="fixed right-4 bottom-4 z-40 w-80 rounded-lg border border-gray-800 bg-gray-900/95 shadow-xl">
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
            {approving ? "Approving..." : "Approve All"}
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
        </Routes>
      </Router>
    </AuthProvider>
  );
}
