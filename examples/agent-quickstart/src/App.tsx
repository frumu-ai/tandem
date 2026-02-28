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
import Swarm from "./pages/Swarm";
import {
  BrainCircuit,
  Clock,
  MessageCircle,
  Radio,
  Settings2,
  LogOut,
  AlertTriangle,
  PlugZap,
  Network,
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
      className={`group flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm font-medium transition-all duration-300 relative overflow-hidden ${
        active ? "text-white shadow-lg" : `${color} hover:text-white hover:bg-white/5`
      }`}
    >
      {active && (
        <div className="absolute inset-0 bg-gradient-to-r from-violet-500/20 to-fuchsia-500/0 opacity-100" />
      )}
      {active && (
        <div className="absolute left-0 top-0 bottom-0 w-1 bg-violet-500 rounded-r-full shadow-[0_0_10px_rgba(139,92,246,0.8)]" />
      )}
      <div
        className={`relative z-10 flex items-center gap-3 transition-transform duration-300 ${active ? "translate-x-1" : "group-hover:translate-x-1"}`}
      >
        {icon}
        <span>{label}</span>
      </div>
    </Link>
  );
}

/* ─── Sidebar ─── */
function Sidebar() {
  const { logout, providerConfigured, providerLoading } = useAuth();

  return (
    <aside className="w-64 bg-gray-950/80 backdrop-blur-xl border-r border-white/5 shadow-2xl z-20 flex flex-col shrink-0">
      {/* Brand */}
      <div className="px-5 py-6 border-b border-white/5 bg-black/40 relative overflow-hidden">
        <div className="absolute top-0 right-0 w-32 h-32 bg-violet-500/10 blur-[50px] rounded-full -translate-y-1/2 translate-x-1/2"></div>
        <div className="flex items-center gap-3 relative z-10">
          <div className="w-10 h-10 rounded-xl bg-violet-500/20 border border-violet-500/30 shadow-[0_0_15px_rgba(139,92,246,0.3)] flex items-center justify-center shrink-0">
            <BrainCircuit size={20} className="text-violet-400" />
          </div>
          <div>
            <p className="text-base font-bold text-white tracking-tight">Tandem</p>
            <p className="text-[11px] font-medium tracking-wide text-violet-400/80 uppercase mt-0.5">
              Agent Quickstart
            </p>
          </div>
        </div>
      </div>

      {/* Provider warning */}
      {!providerLoading && !providerConfigured && (
        <Link
          to="/setup"
          className="mx-4 mt-4 flex items-center gap-2 bg-amber-500/10 border border-amber-500/20 rounded-xl px-3 py-2 text-xs font-medium text-amber-400 hover:bg-amber-500/20 transition-colors shadow-inner"
        >
          <AlertTriangle size={14} className="shrink-0" />
          Configure provider
        </Link>
      )}

      {/* Nav */}
      <nav className="flex-1 px-3 py-4 space-y-1 overflow-y-auto custom-scrollbar">
        <NavLink
          to="/chat"
          icon={<BrainCircuit size={18} />}
          label="Chat"
          color="text-violet-400"
        />
        <NavLink to="/agents" icon={<Clock size={18} />} label="Agents" color="text-emerald-400" />
        <NavLink
          to="/channels"
          icon={<MessageCircle size={18} />}
          label="Channels"
          color="text-purple-400"
        />
        <NavLink to="/mcp" icon={<PlugZap size={18} />} label="MCP Plugins" color="text-cyan-400" />
        <NavLink
          to="/swarm"
          icon={<Network size={18} />}
          label="Agent Swarm"
          color="text-teal-300"
        />
        <NavLink to="/feed" icon={<Radio size={18} />} label="Live Feed" color="text-sky-400" />
      </nav>

      {/* Bottom */}
      <div className="px-4 py-4 border-t border-white/5 bg-black/20 space-y-1">
        <NavLink
          to="/setup"
          icon={<Settings2 size={18} />}
          label="Provider Setup"
          color="text-blue-400"
        />
        <button
          onClick={logout}
          className="w-full group flex items-center justify-center gap-2 px-3 py-2.5 rounded-xl text-sm font-medium text-gray-400 hover:text-white hover:bg-white/5 transition-all duration-300"
        >
          <LogOut size={18} className="group-hover:-translate-x-1 transition-transform" />
          <span>Sign out</span>
        </button>
      </div>
    </aside>
  );
}

/* ─── App shell ─── */
function Shell({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-[100dvh] bg-transparent overflow-hidden selection:bg-violet-500/30">
      <Sidebar />
      <main className="flex-1 min-w-0 flex flex-col relative z-10 backdrop-blur-3xl bg-black/20 custom-scrollbar">
        {children}
      </main>
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
        path="/swarm"
        element={
          <Protected>
            <Shell>
              <Swarm />
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
