import React, { useState } from "react";
import { useAuth } from "../AuthContext";
import { useNavigate } from "react-router-dom";
import { KeyRound, Server } from "lucide-react";

export const Login: React.FC = () => {
  const [key, setKey] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const { login } = useAuth();
  const navigate = useNavigate();

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setLoading(true);

    const success = await login(key);
    if (success) {
      navigate("/research");
    } else {
      setError("Connection failed. Verify your server key or engine status.");
    }
    setLoading(false);
  };

  return (
    <div className="min-h-screen bg-gray-950 flex flex-col justify-center py-12 sm:px-6 lg:px-8">
      <div className="sm:mx-auto sm:w-full sm:max-w-md">
        <div className="flex justify-center text-emerald-500">
          <Server size={48} />
        </div>
        <h2 className="mt-6 text-center text-3xl font-extrabold text-white">
          Tandem Engine Portal
        </h2>
        <p className="mt-2 text-center text-sm text-gray-400">
          Connect to your remote headless server
        </p>
      </div>

      <div className="mt-8 sm:mx-auto sm:w-full sm:max-w-md">
        <div className="bg-gray-900 py-8 px-4 shadow-xl border border-gray-800 sm:rounded-lg sm:px-10">
          <form className="space-y-6" onSubmit={handleSubmit}>
            <div>
              <label htmlFor="serverKey" className="block text-sm font-medium text-gray-300">
                Server API Key
              </label>
              <div className="mt-1 relative rounded-md shadow-sm">
                <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                  <KeyRound className="h-5 w-5 text-gray-500" />
                </div>
                <input
                  id="serverKey"
                  name="serverKey"
                  type="password"
                  required
                  value={key}
                  onChange={(e) => setKey(e.target.value)}
                  className="focus:ring-emerald-500 focus:border-emerald-500 block w-full pl-10 sm:text-sm border-gray-700 bg-gray-800 text-white rounded-md py-2"
                  placeholder="Enter TANDEM_API_TOKEN"
                />
              </div>
            </div>

            {error && <div className="text-red-400 text-sm mt-2 font-medium">{error}</div>}

            <div>
              <button
                type="submit"
                disabled={loading}
                className="w-full flex justify-center py-2 px-4 border border-transparent rounded-md shadow-sm text-sm font-medium text-white bg-emerald-600 hover:bg-emerald-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-emerald-500 disabled:opacity-50"
              >
                {loading ? "Connecting..." : "Connect to Engine"}
              </button>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
};
