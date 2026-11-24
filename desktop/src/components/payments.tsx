import { CircleCheck, CircleSlash, Clock, XCircle } from "lucide-react";
import type { PlanName, RequestStatus, Term } from "../lib/api";
import { planLabel } from "../lib/format";

const STATUS: Record<RequestStatus, { label: string; className: string; Icon: typeof Clock }> = {
  pending: { label: "Waiting for review", className: "bg-amber-50 text-amber-800 ring-amber-200", Icon: Clock },
  approved: { label: "Approved", className: "bg-teal-50 text-teal-800 ring-teal-200", Icon: CircleCheck },
  rejected: { label: "Not approved", className: "bg-red-50 text-red-800 ring-red-200", Icon: XCircle },
  cancelled: { label: "Cancelled", className: "bg-slate-100 text-slate-600 ring-slate-200", Icon: CircleSlash },
};

export function RequestStatusBadge({ status }: { status: RequestStatus }) {
  const s = STATUS[status];
  return (
    <span className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-semibold whitespace-nowrap ring-1 ${s.className}`}>
      <s.Icon className="size-3" aria-hidden /> {s.label}
    </span>
  );
}

export function planTermLabel(plan: PlanName, term: Term): string {
  return `${planLabel(plan)} · ${term === "monthly" ? "1 month" : "1 year"}`;
}

/** Biggest file we'll try to open; it's shrunk before upload anyway. */
const MAX_FILE_BYTES = 20 * 1024 * 1024;
/** Longest side after shrinking: still sharp enough to read a reference number. */
const MAX_SIDE = 1600;

/**
 * Reads a receipt screenshot or photo and returns a JPEG data URL, scaled so the
 * longest side is at most 1600 px (usually 150–400 KB). Throws a readable message.
 */
export async function readReceiptImage(file: File): Promise<string> {
  if (!/^image\/(png|jpeg|webp)$/.test(file.type)) throw new Error("Choose a PNG, JPG or WEBP image of the receipt.");
  if (file.size > MAX_FILE_BYTES) throw new Error("That image is too large. Use a screenshot of the receipt instead.");
  const original = await new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result as string);
    r.onerror = () => reject(new Error("Couldn't read that file."));
    r.readAsDataURL(file);
  });
  const img = await new Promise<HTMLImageElement>((resolve, reject) => {
    const i = new Image();
    i.onload = () => resolve(i);
    i.onerror = () => reject(new Error("Couldn't open that image. Try another screenshot."));
    i.src = original;
  });
  const scale = Math.min(1, MAX_SIDE / Math.max(img.naturalWidth, img.naturalHeight));
  const w = Math.max(1, Math.round(img.naturalWidth * scale));
  const h = Math.max(1, Math.round(img.naturalHeight * scale));
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) return original;
  ctx.fillStyle = "#fff"; // transparent PNG areas become white, not black
  ctx.fillRect(0, 0, w, h);
  ctx.drawImage(img, 0, 0, w, h);
  return canvas.toDataURL("image/jpeg", 0.88);
}
