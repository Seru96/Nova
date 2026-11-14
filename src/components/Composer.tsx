import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { VoicePhase } from "../lib/api";
import { Icon } from "./Icon";

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
  const ref = useRef<HTMLTextAreaElement>(null);

  // Autoajuste de altura.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
  }, [text]);

  const submit = () => {
    if (!text.trim() || disabled) return;
    onSend(text);
    setText("");
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      submit();
    }
    if (e.key === "Escape") onStopSpeaking();
  };

  return (
    <div className="composer">
      <div className="composer-inner">
        {listening && heard && <div className="heard" title={heard}>Oído: «{heard}»</div>}
        <div className="composer-box">
          <textarea
            ref={ref}
            rows={1}
            autoFocus
            placeholder={disabled ? "Esperando al modelo local…" : "Escribe a Nova…"}
            value={text}
            disabled={disabled}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={onKey}
          />
          {listening && (
            <button
              className={`round-btn ${voice === "awake" ? "live" : ""}`}
              title="Hablar sin decir «Oye Nova»"
              onClick={onTalk}
            >
              <Icon name="mic" />
            </button>
          )}
          {busy ? (
            <button className="round-btn" title="Parar" onClick={onCancel}>
              <Icon name="stop" />
            </button>
          ) : (
            <button className="round-btn primary" title="Enviar" disabled={disabled || !text.trim()} onClick={submit}>
              <Icon name="send" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
