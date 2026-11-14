import { useState, type KeyboardEvent } from "react";
import type { VoicePhase } from "../lib/api";

interface Props {
  disabled: boolean;
  busy: boolean;
  listening: boolean;
  voice: VoicePhase;
  heard: string;
  onSend: (text: string) => void;
  onCancel: () => void;
  onTalk: () => void;
  onStopSpeaking: () => void;
}

export function Composer({ disabled, busy, listening, voice, heard, onSend, onCancel, onTalk, onStopSpeaking }: Props) {
  const [text, setText] = useState("");

  const submit = () => {
    if (!text.trim() || disabled) return;
    onSend(text);
    setText("");
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
    if (e.key === "Escape") onStopSpeaking();
  };

  return (
    <footer className="border-t border-[var(--line)] px-3 py-3">
      {listening && heard && (
        <div className="mb-2 truncate px-1 text-[11px] text-[var(--muted)]" title={heard}>
          Oído: «{heard}»
        </div>
      )}
      <div className="flex items-end gap-2">
        <textarea
          className="max-h-40 min-h-[42px] flex-1 resize-none rounded-xl border border-[var(--line)] bg-[var(--panel)] px-3 py-2.5 text-sm outline-none focus:border-sky-500"
          rows={1}
          placeholder={disabled ? "Esperando al modelo local…" : "Escribe a Nova…"}
          value={text}
          disabled={disabled}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKey}
        />
        {listening && (
          <button
            className={`icon-btn h-[42px] w-[42px] ${voice === "awake" ? "!bg-sky-500 text-white" : ""}`}
            title="Hablar sin decir «Oye Nova»"
            onClick={onTalk}
          >
            🎙
          </button>
        )}
        {busy ? (
          <button className="btn h-[42px]" onClick={onCancel}>
            Parar
          </button>
        ) : (
          <button className="btn btn-primary h-[42px]" disabled={disabled || !text.trim()} onClick={submit}>
            Enviar
          </button>
        )}
      </div>
    </footer>
  );
}
