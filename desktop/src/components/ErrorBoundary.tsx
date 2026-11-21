import { AlertTriangle } from "lucide-react";
import { Component, type ErrorInfo, type ReactNode } from "react";

/**
 * Catches a crash while drawing a page so the app shows a message (and the
 * sidebar stays usable) instead of going blank. `resetKey` clears the error
 * when the user navigates elsewhere.
 */
export class ErrorBoundary extends Component<{ children: ReactNode; resetKey: string }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Page crashed:", error, info.componentStack);
  }

  componentDidUpdate(prev: { resetKey: string }) {
    if (prev.resetKey !== this.props.resetKey && this.state.error) this.setState({ error: null });
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div role="alert" className="max-w-xl rounded-xl border border-red-200 bg-red-50 p-5 text-sm text-red-900">
        <p className="flex items-center gap-2 font-semibold">
          <AlertTriangle className="size-4" aria-hidden /> This page couldn't be shown.
        </p>
        <p className="mt-2">Try another page, or close and reopen Clinilog. Your records are safe on the server.</p>
        <p className="mt-3 font-mono text-xs text-red-700">{this.state.error.message}</p>
      </div>
    );
  }
}
