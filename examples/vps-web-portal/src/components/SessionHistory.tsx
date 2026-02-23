import React, { useEffect, useState } from "react";
import { api } from "../api";
import type { SessionRecord } from "../api";
import { History, Trash2, Loader2, Play } from "lucide-react";

interface SessionHistoryProps {
  onSelectSession: (sessionId: string) => void;
  currentSessionId?: string | null;
  className?: string;
}

export const SessionHistory: React.FC<SessionHistoryProps> = ({
  onSelectSession,
  currentSessionId,
  className = "",
}) => {
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);

  const loadSessions = async () => {
    try {
      setLoading(true);
      setError(null);
      // Fetch the last 20 sessions globally
      const res = await api.listSessions({ pageSize: 20 });
      setSessions(res.sessions || []);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : "Failed to load history";
      setError(errorMessage);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadSessions();
  }, []);

  const handleDelete = async (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    try {
      setDeletingId(id);
      await api.deleteSession(id);
      setSessions((prev) => prev.filter((s) => s.id !== id));
      if (currentSessionId === id) {
        onSelectSession(""); // Clear current if deleted
      }
    } catch (err) {
      console.error("Failed to delete session", err);
      // Ideally show a toast here, but we'll stick to console for now
    } finally {
      setDeletingId(null);
    }
  };

  if (loading) {
    return (
      <div className={`p-4 flex justify-center text-gray-400 ${className}`}>
        <Loader2 className="animate-spin" size={20} />
      </div>
    );
  }

  if (error) {
    return (
      <div className={`p-4 text-red-400 text-sm ${className}`}>
        <p>Error loading history:</p>
        <p className="opacity-80 mt-1">{error}</p>
        <button onClick={loadSessions} className="mt-2 text-blue-400 hover:text-blue-300 underline">
          Retry
        </button>
      </div>
    );
  }

  return (
    <div className={`flex flex-col h-full bg-gray-900 border-l border-gray-800 ${className}`}>
      <div className="p-4 border-b border-gray-800 flex items-center justify-between">
        <h3 className="text-white font-medium flex items-center gap-2">
          <History size={16} className="text-blue-400" />
          Recent Sessions
        </h3>
        <button
          onClick={loadSessions}
          className="text-gray-400 hover:text-white"
          title="Refresh History"
        >
          <Loader2 size={14} className={loading ? "animate-spin" : ""} />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-2 space-y-1">
        {sessions.length === 0 ? (
          <div className="text-gray-500 text-sm p-4 text-center italic">
            No past sessions found.
          </div>
        ) : (
          sessions.map((session) => {
            const isCurrent = session.id === currentSessionId;
            const isDeleting = deletingId === session.id;

            return (
              <div
                key={session.id}
                onClick={() => onSelectSession(session.id)}
                className={`group flex flex-col p-3 rounded-md cursor-pointer transition-colors border ${
                  isCurrent
                    ? "bg-blue-900/20 border-blue-500/30"
                    : "bg-gray-800 border-transparent hover:bg-gray-700"
                }`}
              >
                <div className="flex items-start justify-between">
                  <span className="text-gray-200 text-sm font-medium truncate pr-2">
                    {session.title || "Untitled Session"}
                  </span>
                  <button
                    onClick={(e) => handleDelete(e, session.id)}
                    disabled={isDeleting}
                    className="text-gray-500 hover:text-red-400 opacity-0 group-hover:opacity-100 transition-opacity p-1"
                    title="Delete Session"
                  >
                    {isDeleting ? (
                      <Loader2 size={14} className="animate-spin" />
                    ) : (
                      <Trash2 size={14} />
                    )}
                  </button>
                </div>
                <div className="text-xs text-gray-500 mt-1 flex items-center justify-between">
                  <span>
                    {new Date(session.created_at_ms).toLocaleDateString()}{" "}
                    {new Date(session.created_at_ms).toLocaleTimeString([], {
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </span>
                  <span className="text-gray-600 font-mono" title="Session ID">
                    {session.id.substring(0, 8)}
                  </span>
                </div>
                {isCurrent && (
                  <div className="mt-2 text-xs text-blue-400 flex items-center gap-1 font-medium bg-blue-500/10 px-2 py-1 rounded w-max">
                    <Play size={10} className="fill-blue-400" /> Active
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
};
