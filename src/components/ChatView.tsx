import { useEffect, useRef } from "react";
import type { Engine } from "../lib/api";
import type { StoredMessage } from "../lib/db";

export interface Pending {
  id: string;
  engine: Engine;
  text: string;
  note: string | null;
  actions: string[];
}

interface Props {
  messages: StoredMessage[];
  pending: Pending | null;
  userName: string;
  ready: boolean;
  onSuggestion: (text: string) => void;
}

const engineLabel: Record<Engine, string> = { local: "Local", claude: "Claude", system: "Nova" };

const suggestions = [
  "¿Qué puedes hacer?",
  "Abre Notas y escribe la lista de la compra",
  "Cuéntame un chiste malo",
  "Pregúntale a Claude cómo estructurar una app en Rust",
];

function Meta({ engine }: { engine: Engine | null }) {
  return (
    <div className="meta">
      Nova{engine && engine !== "system" && <span className={`engine-${engine}`}>· {engineLabel[engine]}</span>}
    </div>
  );
}

export function ChatView({ messages, pending, userName, ready, onSuggestion }: Props) {
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [messages.length, pending?.text, pending?.actions.length]);

  if (messages.length === 0 && !pending) {
    return (
      <div className="chat">
        <div className="empty">
          <div className="orb" />
          <h1>{userName ? `Hola, ${userName}` : "Hola, soy Nova"}</h1>
          <p>Escríbeme o activa la escucha y di «Oye Nova». Puedo abrir apps y escribir en ellas por ti.</p>
          {ready && (
            <div className="chips">
              {suggestions.map((s) => (
                <button key={s} className="chip" onClick={() => onSuggestion(s)}>
                  {s}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="chat">
      <div className="chat-inner">
        {messages.map((m) =>
          m.role === "system" ? (
            <div key={m.id} className="sys-msg">
              {m.content}
            </div>
          ) : (
            <div key={m.id} className={`msg ${m.role === "user" ? "user" : "nova"}`}>
              {m.role === "assistant" && <Meta engine={m.engine} />}
              <div className="bubble">{m.content}</div>
            </div>
          ),
        )}
        {pending && (
          <div className="msg nova">
            <Meta engine={pending.engine} />
            {pending.note && <div className="note">{pending.note}</div>}
            {pending.actions.length > 0 && (
              <div className="actions">
                {pending.actions.map((a, i) => (
                  <div key={i}>{a}</div>
                ))}
              </div>
            )}
            <div className="bubble">{pending.text || <span className="thinking">Pensando…</span>}</div>
          </div>
        )}
        <div ref={end} />
      </div>
    </div>
  );
}
