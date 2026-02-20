import React, { createContext, useContext, useState, useEffect } from "react";
import { api } from "./api";

interface AuthContextType {
  token: string | null;
  login: (token: string) => Promise<boolean>;
  logout: () => void;
  isLoading: boolean;
}

const AuthContext = createContext<AuthContextType>({
  token: null,
  login: async () => false,
  logout: () => {},
  isLoading: true,
});

export const useAuth = () => useContext(AuthContext);

export const AuthProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [token, setToken] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    const storedToken = localStorage.getItem("tandem_portal_token");
    if (storedToken) {
      api.setToken(storedToken);
      setToken(storedToken);
    }
    setIsLoading(false);
  }, []);

  const login = async (newToken: string) => {
    try {
      // Test the token
      api.setToken(newToken);
      await api.getSystemHealth();
      localStorage.setItem("tandem_portal_token", newToken);
      setToken(newToken);
      return true;
    } catch (error) {
      console.error(error);
      api.setToken(""); // clear bad token
      return false;
    }
  };

  const logout = () => {
    localStorage.removeItem("tandem_portal_token");
    api.setToken("");
    setToken(null);
  };

  return (
    <AuthContext.Provider value={{ token, login, logout, isLoading }}>
      {children}
    </AuthContext.Provider>
  );
};
