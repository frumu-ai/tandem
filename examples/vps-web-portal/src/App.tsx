import React from "react";
import { BrowserRouter as Router, Routes, Route, Navigate, Link } from "react-router-dom";
import { AuthProvider, useAuth } from "./AuthContext";
import { Login } from "./pages/Login";
import { ProviderSetup } from "./pages/ProviderSetup";
import { ResearchDashboard } from "./pages/ResearchDashboard";
import { SwarmDashboard } from "./pages/SwarmDashboard";
import { TextAdventure } from "./pages/TextAdventure";
import { SecondBrainDashboard } from "./pages/SecondBrainDashboard";
import { ConnectorsDashboard } from "./pages/ConnectorsDashboard";
import {
  LayoutDashboard,
  Users,
  MessageSquareQuote,
  BrainCircuit,
  LogOut,
  Cable,
} from "lucide-react";

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
        <nav className="flex-1 p-4 space-y-2">
          <Link
            to="/research"
            className="flex items-center gap-3 text-gray-300 hover:text-white hover:bg-gray-800 p-2 rounded-md"
          >
            <LayoutDashboard size={20} /> Research
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
