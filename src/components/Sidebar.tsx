import { useEffect, useMemo, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import type { EngineStatus, VoicePhase } from "../lib/api";
import type { Conversation } from "../lib/db";
import { Icon } from "./Icon";

interface Props {
  conversations: Conversation[];
  currentId: string | null;
  engine: EngineStatus;
  voice: VoicePhase;
  onNew: () => void;
  onOpen: (id: string) => void;
  onDelete: (id: string) => void;
  onSettings: () => void;
}

function when(ts: number): string {
  const d = new Date(ts);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString("es-ES", { hour: "2-digit", minute: "2-digit" });
  }
  const days = Math.floor((now.getTime() - ts) / 86_400_000);
  if (days < 7) return d.toLocaleDateString("es-ES", { weekday: "short" });
  return d.toLocaleDateString("es-ES", { day: "numeric", month: "short" });
}

export function Sidebar({ conversations, currentId, engine, voice, onNew, onOpen, onDelete, onSettings }: Props) {
  const [q, setQ] = useState("");
  const [version, setVersion] = useState("");
  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);
  const list = useMemo(() => {
    const n = q.trim().toLowerCase();
    return n ? conversations.filter((c) => c.title.toLowerCase().includes(n)) : conversations;
  }, [conversations, q]);

  const live = voice === "awake" || voice === "transcribing";
  const dot = engine.phase === "ready" ? "ok" : engine.phase === "error" ? "err" : "warn";
  const engineText =
    engine.phase === "ready" ? "Modelo local listo" : engine.phase === "error" ? "Modelo local con error" : "Preparando modelo local…";

  return (
    <aside className="side">
      <div className="side-drag" data-tauri-drag-region />
      <div className="brand" data-tauri-drag-region>
        <div className={`orb ${live ? "live" : ""} ${engine.phase !== "ready" ? "off" : ""}`} />
        <div className="brand-name">Nova</div>
      </div>

      <button className={`nav-item ${currentId === null ? "on" : ""}`} onClick={onNew}>
        <Icon name="plus" /> Nueva conversación
      </button>

      <label className="search">
        <Icon name="search" size={15} />
        <input placeholder="Buscar conversaciones" value={q} onChange={(e) => setQ(e.target.value)} />
      </label>

      <div className="side-title">Conversaciones</div>
      <div className="conv-list">
        {list.length === 0 && (
          <div className="conv-empty">{q ? "Nada coincide con la búsqueda." : "Aquí aparecerán tus conversaciones."}</div>
        )}
        {list.map((c) => (
          <div key={c.id} className={`conv ${c.id === currentId ? "on" : ""}`} onClick={() => onOpen(c.id)} title={c.title}>
            <span className="name">{c.title}</span>
            <span className="when">{when(c.updated_at)}</span>
            <button
              className="row-action danger"
              title="Borrar conversación"
              onClick={(e) => {
                e.stopPropagation();
                onDelete(c.id);
              }}
            >
              <Icon name="trash" size={15} />
            </button>
          </div>
        ))}
      </div>

      <div className="side-foot">
        <button className="nav-item" onClick={onSettings}>
          <Icon name="gear" /> Ajustes
        </button>
        <div className="engine-line">
          <span className={`dot ${dot}`} /> {engineText}
        </div>
        {version && <div className="version">Nova {version}</div>}
      </div>
    </aside>
  );
}
