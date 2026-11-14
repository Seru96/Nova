import { useEffect, useRef } from "react";
import type { Engine } from "../lib/api";
import type { StoredMessage } from "../lib/db";

export interface Pending {
  id: string;
  engine: Engine;
  text: string;
  note: string | null;
}

interface Props {
  messages: StoredMessage[];
  pending: Pending | null;
  engineLabel: Record<Engine, string>;
}

function Badge({ engine, label }: { engine: Engine; label: string }) {
  const color =
    engine === "claude" ? "text-orange-400" : engine === "system" ? "text-amber-500" : "text-sky-400";
  return <span className={`text-[10px] uppercase tracking-wider ${color}`}>{label}</span>;
}

export function ChatView({ messages, pending, engineLabel }: Props) {
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages.length, pending?.text]);

  return (
    <main className="flex-1 space-y-3 overflow-y-auto px-4 py-4">
      {messages.length === 0 && !pending && (
        <div className="mt-16 text-center text-sm text-[var(--muted)]">
          Hola, soy Nova. Escríbeme o activa la escucha y di «Oye Nova».
        </div>
      )}
      {messages.map((m) =>
        m.role === "system" ? (
          <div key={m.id} className="mx-auto max-w-[90%] text-center text-xs text-red-500">
            {m.content}
          </div>
        ) : (
          <div key={m.id} className={`flex ${m.role === "user" ? "justify-end" : "justify-start"}`}>
            <div className={`bubble ${m.role === "user" ? "bubble-user" : "bubble-nova"}`}>
              {m.role === "assistant" && m.engine && <Badge engine={m.engine} label={engineLabel[m.engine]} />}
              <div className="whitespace-pre-wrap">{m.content}</div>
            </div>
          </div>
        ),
      )}
      {pending && (
        <div className="flex justify-start">
          <div className="bubble bubble-nova">
            <Badge engine={pending.engine} label={engineLabel[pending.engine]} />
            {pending.note && <div className="text-[11px] italic text-[var(--muted)]">{pending.note}</div>}
            <div className="whitespace-pre-wrap">
              {pending.text || <span className="animate-pulse text-[var(--muted)]">Pensando…</span>}
            </div>
          </div>
        </div>
      )}
      <div ref={end} />
    </main>
  );
}
