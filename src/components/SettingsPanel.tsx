import { useEffect, useState } from "react";
import { api, type Personality, type Settings, type VoiceInfo } from "../lib/api";
import { Toggle } from "./Toggle";

interface Props {
  settings: Settings;
  onClose: () => void;
  onSaved: (s: Settings) => void;
  onError: (msg: string) => void;
}

const personalities: { id: Personality; label: string; hint: string }[] = [
  { id: "humor", label: "Con humor", hint: "Ingeniosa y bromista, sin dejar de ser útil (por defecto)" },
  { id: "profesional", label: "Profesional", hint: "Seria, precisa y al grano" },
  { id: "amable", label: "Amable", hint: "Cálida y paciente" },
  { id: "sarcastica", label: "Sarcástica", hint: "Ironía afilada, pero ayuda igual" },
  { id: "personalizada", label: "Personalizada", hint: "Solo tus instrucciones" },
];

const claudeModels = [
  { id: "claude-opus-5-5", label: "Claude Opus 5.5 (máxima calidad)" },
  { id: "claude-sonnet-5-5", label: "Claude Sonnet 5.5 (más barato)" },
  { id: "claude-haiku-5-5", label: "Claude Haiku 5.5 (el más económico)" },
];

export function SettingsPanel({ settings, onClose, onSaved, onError }: Props) {
  const [s, setS] = useState<Settings>(settings);
  const [hasKey, setHasKey] = useState(false);
  const [keyInput, setKeyInput] = useState("");
  const [voices, setVoices] = useState<VoiceInfo[]>([]);

  useEffect(() => {
    api.hasApiKey().then(setHasKey);
    api.listVoices().then(setVoices).catch(() => setVoices([]));
  }, []);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS((prev) => ({ ...prev, [k]: v }));

  const save = async () => {
    try {
      onSaved(await api.saveSettings(s));
      onClose();
    } catch (e) {
      onError(String(e));
    }
  };

  const saveKey = async () => {
    try {
      await api.setApiKey(keyInput);
      setKeyInput("");
      setHasKey(true);
    } catch (e) {
      onError(String(e));
    }
  };

  const spanishVoices = voices.filter((v) => v.language.toLowerCase().startsWith("es"));

  return (
    <div className="fixed inset-0 z-10 flex items-end justify-center bg-black/40 sm:items-center" onClick={onClose}>
      <div
        className="max-h-[90vh] w-full max-w-md overflow-y-auto rounded-t-2xl bg-[var(--bg)] p-5 shadow-2xl sm:rounded-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-base font-semibold">Ajustes</h2>
          <button className="icon-btn" onClick={onClose}>
            ✕
          </button>
        </div>

        <section className="field">
          <label>¿Cómo te llamo?</label>
          <input value={s.user_name} placeholder="Tu nombre (opcional)" onChange={(e) => set("user_name", e.target.value)} />
        </section>

        <section className="field">
          <label>Personalidad</label>
          <div className="grid gap-1.5">
            {personalities.map((p) => (
              <button
                key={p.id}
                onClick={() => set("personality", p.id)}
                className={`rounded-lg border px-3 py-2 text-left text-sm ${
                  s.personality === p.id ? "border-sky-500 bg-sky-500/10" : "border-[var(--line)]"
                }`}
              >
                <div>{p.label}</div>
                <div className="text-xs text-[var(--muted)]">{p.hint}</div>
              </button>
            ))}
          </div>
          <textarea
            rows={3}
            value={s.personality_custom}
            placeholder="Instrucciones extra (p. ej. «tutéame y hazme referencias a Star Wars»)"
            onChange={(e) => set("personality_custom", e.target.value)}
          />
        </section>

        <section className="field">
          <label>Claude (para tareas complejas)</label>
          {hasKey ? (
            <div className="flex items-center justify-between text-sm">
              <span className="text-emerald-500">API key guardada en el llavero del sistema</span>
              <button
                className="btn"
                onClick={async () => {
                  await api.clearApiKey();
                  setHasKey(false);
                }}
              >
                Borrar
              </button>
            </div>
          ) : (
            <div className="flex gap-2">
              <input
                type="password"
                value={keyInput}
                placeholder="sk-ant-…"
                onChange={(e) => setKeyInput(e.target.value)}
              />
              <button className="btn btn-primary" disabled={!keyInput} onClick={saveKey}>
                Guardar
              </button>
            </div>
          )}
          <select value={s.claude_model} onChange={(e) => set("claude_model", e.target.value)}>
            {claudeModels.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
          <Toggle
            label="Escalar a Claude automáticamente cuando haga falta"
            checked={s.auto_escalate}
            onChange={(v) => set("auto_escalate", v)}
          />
          <p className="text-xs text-[var(--muted)]">
            Con el Modo Confidencial activo, Nova nunca contacta con Claude. También puedes decir «esto es confidencial»
            o «modo privado».
          </p>
        </section>

        <section className="field">
          <label>Voz</label>
          <Toggle
            label="Leer en voz alta las respuestas a lo que pido por voz"
            checked={s.speak_replies}
            onChange={(v) => set("speak_replies", v)}
          />
          <select value={s.voice_name} onChange={(e) => set("voice_name", e.target.value)}>
            <option value="">Automática (voz femenina en español)</option>
            {(spanishVoices.length ? spanishVoices : voices).map((v) => (
              <option key={v.name} value={v.name}>
                {v.name} · {v.language}
              </option>
            ))}
          </select>
          <button className="btn self-start" onClick={() => api.speak("Hola, soy Nova. ¿Me oyes bien?")}>
            Probar voz
          </button>
        </section>

        <div className="mt-5 flex justify-end gap-2">
          <button className="btn" onClick={onClose}>
            Cancelar
          </button>
          <button className="btn btn-primary" onClick={save}>
            Guardar
          </button>
        </div>
      </div>
    </div>
  );
}
