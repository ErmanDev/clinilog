import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, X } from "lucide-react";
import { useState } from "react";
import { useSync } from "../lib/sync";
import { ConfirmDialog } from "./Dialog";

/**
 * Minimize / Close for the fullscreen window (it has no titlebar).
 * Closing with visits still waiting to upload asks first; they stay saved on this PC.
 */
export function WindowControls({ tone = "light" }: { tone?: "light" | "dark" }) {
  const { status } = useSync();
  const [confirm, setConfirm] = useState(false);
  const waiting = (status?.pending ?? 0) + (status?.failed ?? 0);
  const btn =
    tone === "dark"
      ? "text-teal-100 hover:bg-white/10 hover:text-white"
      : "text-slate-500 hover:bg-slate-100 hover:text-slate-900";

  const close = () => void getCurrentWindow().close();

  return (
    <div className="flex items-center gap-1 print:hidden">
      <button type="button" onClick={() => void getCurrentWindow().minimize()} className={`rounded-md p-2 ${btn}`} aria-label="Minimize Clinilog" title="Minimize">
        <Minus className="size-4" aria-hidden />
      </button>
      <button
        type="button"
        onClick={() => (waiting > 0 ? setConfirm(true) : close())}
        className={`rounded-md p-2 hover:!bg-red-600 hover:!text-white ${btn}`}
        aria-label="Close Clinilog"
        title="Close"
      >
        <X className="size-4" aria-hidden />
      </button>
      <ConfirmDialog
        open={confirm}
        title="Close Clinilog?"
        confirmLabel="Close anyway"
        tone="primary"
        onCancel={() => setConfirm(false)}
        onConfirm={close}
      >
        <p>
          {waiting} visit{waiting === 1 ? " hasn't" : "s haven't"} been uploaded yet. {waiting === 1 ? "It stays" : "They stay"} saved
          (encrypted) on this PC and upload the next time you open Clinilog while online.
        </p>
      </ConfirmDialog>
    </div>
  );
}
