import { listen } from "@tauri-apps/api/event";
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { syncApi, type SyncStatus } from "./api";

interface SyncContextValue {
  status: SyncStatus | null;
  syncing: boolean;
  syncNow: () => Promise<void>;
}

const SyncContext = createContext<SyncContextValue | null>(null);

/** Live online/offline state and the number of visits waiting to upload. */
export function SyncProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [syncing, setSyncing] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    syncApi.status().then(setStatus).catch(() => {});
    listen<SyncStatus>("sync-status", (e) => setStatus(e.payload)).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  const syncNow = useCallback(async () => {
    setSyncing(true);
    try {
      setStatus(await syncApi.syncNow());
    } finally {
      setSyncing(false);
    }
  }, []);

  return <SyncContext.Provider value={{ status, syncing, syncNow }}>{children}</SyncContext.Provider>;
}

export function useSync(): SyncContextValue {
  const ctx = useContext(SyncContext);
  if (!ctx) throw new Error("useSync must be used inside <SyncProvider>");
  return ctx;
}
