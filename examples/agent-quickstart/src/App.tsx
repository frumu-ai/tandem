import React from "react";
import { BrowserRouter, Routes, Route, Link, Navigate, useLocation } from "react-router-dom";
import { AuthProvider, useAuth } from "./AuthContext";
import Login from "./pages/Login";
import ChatBrain from "./pages/ChatBrain";
import Agents from "./pages/Agents";
import Channels from "./pages/Channels";
import LiveFeed from "./pages/LiveFeed";
import ProviderSetup from "./pages/ProviderSetup";
import McpSetup from "./pages/McpSetup";
import {
  BrainCircuit,
  Clock,
  MessageCircle,
  Radio,
  Settings2,
  LogOut,
  AlertTriangle,
  PlugZap,
} from "lucide-react";

/* ─── Protected Route ─── */
function Protected({ children }: { children: React.ReactNode }) {
  const { token, isLoading, providerConfigured, providerLoading } = useAuth();
  const { pathname } = useLocation();
  if (isLoading)
    return (
      <div className="flex h-screen items-center justify-center bg-gray-950 text-gray-600">
        Loading…
      </div>
    );
  if (!token) return <Navigate to="/login" replace />;
  if (!providerLoading && !providerConfigured && pathname !== "/setup") {
    return <Navigate to="/setup" replace />;
  }
  return <>{children}</>;
}

/* ─── Nav item ─── */
interface NavItem {
  to: string;
  icon: React.ReactNode;
  label: string;
  color?: string;
}
function NavLink({ to, icon, label, color = "text-gray-400" }: NavItem) {
  const { pathname } = useLocation();
  const active = pathname === to || pathname.startsWith(to + "/");
  return (
    <Link
      to={to}
      className={`flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm font-medium transition-all ${
        active ? "bg-gray-800 text-white" : `${color} hover:text-white hover:bg-gray-800/60`
      }`}
    >
      {icon}
      <span>{label}</span>
    </Link>
  );
}

/* ─── Sidebar ─── */
function Sidebar() {
  const { logout, providerConfigured, providerLoading } = useAuth();

  return (
    <aside className="w-56 bg-gray-900/80 border-r border-gray-800 flex flex-col shrink-0">
      {/* Brand */}
      <div className="px-4 py-5 border-b border-gray-800">
        <div className="flex items-center gap-2.5">
          <div className="w-8 h-8 rounded-lg bg-violet-600/20 border border-violet-600/30 flex items-center justify-center shrink-0">
            <BrainCircuit size={16} className="text-violet-400" />
          </div>
          <div>
            <p className="text-sm font-bold text-white leading-none">Tandem</p>
            <p className="text-[10px] text-gray-500 mt-0.5">Agent Quickstart</p>
          </div>
        </div>
      </div>

      {/* Provider warning */}
      {!providerLoading && !providerConfigured && (
        <Link
          to="/setup"
          className="mx-3 mt-3 flex items-center gap-2 bg-amber-900/20 border border-amber-800/40 rounded-lg px-3 py-2 text-xs text-amber-300 hover:bg-amber-900/30 transition-colors"
        >
          <AlertTriangle size={12} className="shrink-0" />
          Configure provider
        </Link>
      )}

      {/* Nav */}
      <nav className="flex-1 px-3 py-3 space-y-1">
        <NavLink
          to="/chat"
          icon={<BrainCircuit size={16} />}
          label="Chat"
          color="text-violet-400"
        />
        <NavLink to="/agents" icon={<Clock size={16} />} label="Agents" color="text-emerald-400" />
        <NavLink
          to="/channels"
          icon={<MessageCircle size={16} />}
          label="Channels"
          color="text-purple-400"
        />
        <NavLink to="/mcp" icon={<PlugZap size={16} />} label="MCP" color="text-cyan-400" />
        <NavLink to="/feed" icon={<Radio size={16} />} label="Live Feed" color="text-sky-400" />
      </nav>

      {/* Bottom */}
      <div className="px-3 py-3 border-t border-gray-800 space-y-1">
        <NavLink
          to="/setup"
          icon={<Settings2 size={16} />}
          label="Provider"
          color="text-blue-400"
        />
        <button
          onClick={logout}
          className="w-full flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm text-gray-500 hover:text-white hover:bg-gray-800/60 transition-colors"
        >
          <LogOut size={16} />
          <span>Sign out</span>
        </button>
      </div>
    </aside>
  );
}

/* ─── App shell ─── */
function Shell({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar />
      <main className="flex-1 min-w-0 overflow-hidden">{children}</main>
    </div>
  );
}

/* ─── Routes ─── */
function AppRoutes() {
  const { token, providerConfigured, providerLoading } = useAuth();
  const authedTarget = !providerLoading && !providerConfigured ? "/setup" : "/chat";
  return (
    <Routes>
      <Route path="/login" element={token ? <Navigate to={authedTarget} replace /> : <Login />} />
      <Route
        path="/chat"
        element={
          <Protected>
            <Shell>
              <ChatBrain />
            </Shell>
          </Protected>
        }
      />
      <Route
        path="/agents"
        element={
          <Protected>
            <Shell>
              <Agents />
            </Shell>
          </Protected>
        }
      />
      <Route
        path="/channels"
        element={
          <Protected>
            <Shell>
              <Channels />
            </Shell>
          </Protected>
        }
      />
      <Route
        path="/feed"
        element={
          <Protected>
            <Shell>
              <LiveFeed />
            </Shell>
          </Protected>
        }
      />
      <Route
        path="/mcp"
        element={
          <Protected>
            <Shell>
              <McpSetup />
            </Shell>
          </Protected>
        }
      />
      <Route
        path="/setup"
        element={
          <Protected>
            <Shell>
              <ProviderSetup />
            </Shell>
          </Protected>
        }
      />
      <Route path="*" element={<Navigate to={token ? authedTarget : "/login"} replace />} />
    </Routes>
  );
}

export default function App() {
  return (
    <BrowserRouter>
      <AuthProvider>
        <AppRoutes />
      </AuthProvider>
    </BrowserRouter>
  );
}
