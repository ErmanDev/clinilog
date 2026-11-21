import { useEffect, useRef } from "react";

/**
 * Login-screen animation (decorative). Three.js is loaded on demand so the login
 * form appears immediately; with "reduce motion" on, it shows a still picture.
 */
export function ClinicScene({ className }: { className?: string }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let handle: { dispose(): void } | null = null;
    let cancelled = false;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    import("../scene/clinicScene")
      .then(({ startClinicScene }) => {
        if (!cancelled) handle = startClinicScene(el, { reducedMotion });
      })
      .catch((e) => console.error("Clinic animation failed to load:", e)); // the panel just stays plain
    return () => {
      cancelled = true;
      handle?.dispose();
    };
  }, []);

  return <div ref={ref} className={className} aria-hidden="true" />;
}
