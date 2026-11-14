import type { Settings, VoicePhase } from "../lib/api";
import { Toggle } from "./Toggle";

interface Props {
  settings: Settings | null;
  voice: VoicePhase;
  engineReady: boolean;
  onListening: (on: boolean) => void;
  onConfidential: (on: boolean) => void;
  onSettings: () => void;
  onNew: () => void;
}

const voiceText: Record<VoicePhase, string> = {
  off: "Escucha desactivada",
  loading: "Cargando oído…",
  idle: "Di «Oye Nova»",
  awake: "Te escucho…",
  transcribing: "Entendiendo…",
};

export function Header({ settings, voice, engineReady, onListening, onConfidential, onSettings, onNew }: Props) {
  const orbClass =
    voice === "awake" || voice === "transcribing"
      ? "bg-sky-400 shadow-[0_0_18px_4px_rgba(56,189,248,0.7)] animate-pulse"
      : engineReady
        ? "bg-sky-500/80 shadow-[0_0_10px_2px_rgba(56,189,248,0.4)]"
        : "bg-zinc-400";

  return (
    <header className="flex flex-col gap-2 border-b border-[var(--line)] px-4 py-3">
      <div className="flex items-center gap-3">
        <div className={`h-4 w-4 rounded-full transition-all ${orbClass}`} />
        <div className="flex-1">
          <div className="text-sm font-semibold tracking-[0.3em]">NOVA</div>
          <div className="text-xs text-[var(--muted)]">
            {voice === "off" && settings?.listening ? "Preparando la escucha…" : voiceText[voice]}
          </div>
        </div>
        <button className="icon-btn" title="Nueva conversación" onClick={onNew}>
          ＋
        </button>
        <button className="icon-btn" title="Ajustes" onClick={onSettings}>
          ⚙
        </button>
      </div>
      <div className="flex gap-4 text-xs">
        <Toggle
          label="Escucha"
          checked={settings?.listening ?? false}
          disabled={!settings}
          onChange={onListening}
        />
        <Toggle
          label="Confidencial"
          checked={settings?.confidential ?? false}
          disabled={!settings}
          onChange={onConfidential}
          tone="amber"
        />
      </div>
    </header>
  );
}
