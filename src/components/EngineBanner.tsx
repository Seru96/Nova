import type { EngineStatus } from "../lib/api";

export function EngineBanner({ status, onRetry }: { status: EngineStatus; onRetry: () => void }) {
  if (status.phase === "ready") return null;
  const pct = status.progress != null ? Math.round(status.progress * 100) : null;

  return (
    <div className="border-b border-[var(--line)] bg-[var(--panel)] px-4 py-3 text-xs">
      {status.phase === "error" ? (
        <div className="flex items-start gap-3">
          <div className="flex-1 whitespace-pre-wrap text-red-500">No he podido arrancar el modelo local: {status.detail}</div>
          <button className="btn" onClick={onRetry}>
            Reintentar
          </button>
        </div>
      ) : (
        <>
          <div className="mb-2 text-[var(--muted)]">
            {status.detail || "Preparando a Nova…"}
            {status.phase === "downloading" && " · solo la primera vez"}
          </div>
          <div className="h-1.5 overflow-hidden rounded bg-zinc-500/20">
            <div
              className={`h-full bg-sky-500 transition-all ${pct == null ? "w-1/3 animate-pulse" : ""}`}
              style={pct != null ? { width: `${pct}%` } : undefined}
            />
          </div>
        </>
      )}
    </div>
  );
}
