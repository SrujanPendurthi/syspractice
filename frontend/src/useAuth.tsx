import { createContext, useCallback, useContext, useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import * as api from './api';
import type { User } from './types';

interface Auth {
  user: User | null;
  /** True until the first `/api/auth/me` answers. Distinguishes "signed out" from "not asked yet". */
  loading: boolean;
  signIn: (email: string, password: string) => Promise<void>;
  signUp: (email: string, displayName: string, password: string) => Promise<void>;
  signOut: () => Promise<void>;
  /** Called when a request comes back 401, so an expired cookie updates the UI. */
  clear: () => void;
}

const AuthContext = createContext<Auth | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);

  // The cookie is httpOnly, so the only way to know who is signed in is to ask.
  useEffect(() => {
    api
      .currentUser()
      .then(setUser)
      .catch(() => setUser(null))
      .finally(() => setLoading(false));
  }, []);

  const signIn = useCallback(async (email: string, password: string) => {
    setUser(await api.login(email, password));
  }, []);

  const signUp = useCallback(async (email: string, displayName: string, password: string) => {
    setUser(await api.register(email, displayName, password));
  }, []);

  const signOut = useCallback(async () => {
    // Drop the local user even if the request fails: the alternative is a UI insisting
    // you are signed in when every subsequent call is going to 401.
    await api.logout().catch(() => undefined);
    setUser(null);
  }, []);

  const clear = useCallback(() => setUser(null), []);

  return (
    <AuthContext.Provider value={{ user, loading, signIn, signUp, signOut, clear }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth(): Auth {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth must be used inside AuthProvider');
  return ctx;
}
