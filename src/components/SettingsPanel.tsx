import { useEffect, useState } from "react";
import { api, type Personality, type Settings, type Theme, type VoiceInfo } from "../lib/api";
import { Icon } from "./Icon";

interface Props {
  settings: Settings;
  onClose: () => void;
  onSaved: (s: Settings) => void;
  onError: (msg: string) => void;
}

const personalities: { id: Personality; label: string; hint: string }[] = [
  { id: "humor", label: "Con humor", hint: "Ingeniosa y bromista (por defecto)" },
  { id: "profesional", label: "Profesional", hint: "Seria, precisa y al grano" },
  { id: "amable", label: "Amable", hint: "Cálida y paciente" },
  { id: "sarcastica", label: "Sarcástica", hint: "Ironía afilada, pero ayuda" },
  { id: "personalizada", label: "Personalizada", hint: "Solo tus instrucciones" },
];

const themes: { id: Theme; label: string }[] = [
  { id: "dark", label: "Oscuro" },
  { id: "light", label: "Claro" },
  { id: "system", label: "Sistema" },
];

const claudeModels = [
  { id: "claude-opus-5-5", label: "Claude Opus 5.5 (máxima calidad)" },
  { id: "claude-sonnet-5-5", label: "Claude Sonnet 5.5 (más barato)" },
  { id: "claude-haiku-5-5", label: "Claude Haiku 5.5 (el más económico)" },
];

function Switch({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className={`switch ${checked ? "on" : ""}`}>
      <button type="button" role="switch" aria-checked={checked} onClick={() => onChange(!checked)} />
      <span>{label}</span>
    </label>
  );
}

export function SettingsPanel({ settings, onClose, onSaved, onError }: Props) {
  const [s, setS] = useState<Settings>(settings);
  const [hasKey, setHasKey] = useState(false);
  const [keyInput, setKeyInput] = useState("");
  const [voices, setVoices] = useState<VoiceInfo[]>([]);

  useEffect(() => {
    api.hasApiKey().then(setHasKey);
    api.listVoices().then((v) => setVoices(v ?? [])).catch(() => setVoices([]));
  }, []);

  // Vista previa del tema mientras se elige.
  useEffect(() => {
    applyTheme(s.theme);
    return () => applyTheme(settings.theme);
  }, [s.theme, settings.theme]);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS((prev) => ({ ...prev, [k]: v }));

  const save = async () => {
    try {
      const saved = await api.saveSettings(s);
      applyTheme(saved.theme);
      onSaved(saved);
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

  const spanish = voices.filter((v) => v.language.toLowerCase().startsWith("es"));

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="dialog" onMouseDown={(e) => e.stopPropagation()}>
        <header>
          <h3>Ajustes</h3>
          <button className="row-action" onClick={onClose} title="Cerrar">
            <Icon name="close" size={15} />
          </button>
        </header>

        <div className="dialog-body">
          <section>
            <h4>Apariencia</h4>
            <div className="segmented">
              {themes.map((t) => (
                <button key={t.id} className={s.theme === t.id ? "on" : ""} onClick={() => set("theme", t.id)}>
                  {t.label}
                </button>
              ))}
            </div>
          </section>

          <section>
            <h4>Tú</h4>
            <input value={s.user_name} placeholder="¿Cómo te llamo? (opcional)" onChange={(e) => set("user_name", e.target.value)} />
          </section>

          <section>
            <h4>Personalidad</h4>
            <div className="choice-list">
              {personalities.map((p) => (
                <button key={p.id} className={`choice ${s.personality === p.id ? "on" : ""}`} onClick={() => set("personality", p.id)}>
                  <span>{p.label}</span>
                  <small>{p.hint}</small>
                </button>
              ))}
            </div>
            <textarea
              rows={3}
              value={s.personality_custom}
              placeholder="Instrucciones extra (p. ej. «tutéame y haz referencias a Star Wars»)"
              onChange={(e) => set("personality_custom", e.target.value)}
            />
          </section>

          <section>
            <h4>Control del ordenador</h4>
            <Switch
              label="Permitir que Nova abra apps, escriba en ellas y pulse atajos"
              checked={s.app_control}
              onChange={(v) => set("app_control", v)}
            />
            <p className="hint">
              En Mac, la primera vez que escriba en otra app te pedirá el permiso de Accesibilidad (Ajustes del Sistema →
              Privacidad y seguridad → Accesibilidad).
            </p>
          </section>

          <section>
            <h4>H.A.L.O. · gestos con la cámara</h4>
            <Switch
              label="Mostrar la miniatura de la cámara con tu mano"
              checked={s.halo_preview}
              onChange={(v) => set("halo_preview", v)}
            />
            <ul className="hint halo-help">
              <li><b>Índice estirado</b>: mueve el cursor.</li>
              <li><b>Pellizco</b> (pulgar + índice): clic. Mantén y mueve para arrastrar ventanas o archivos.</li>
              <li><b>Pulgar + corazón</b>: clic derecho.</li>
              <li><b>Dos manos pellizcando</b>: sepáralas para maximizar, júntalas para restaurar, desplázalas a un lado para ocupar media pantalla.</li>
              <li><b>Palma abierta, deslizando rápido</b>: cambia de escritorio.</li>
              <li><b>Puño 1 segundo</b>: cierra la ventana activa (la de primer plano, no Nova).</li>
              <li><b>Palma quieta</b>: alto. Deja de escuchar y pausa los gestos (repite para seguir).</li>
            </ul>
            <p className="hint">Funciona en todo el ordenador, no solo en Nova. Se activa con el switch «Gestos» de la cabecera. La cámara se procesa en local y no se guarda nada.</p>
          </section>

          <section>
            <h4>Claude</h4>
            {hasKey ? (
              <div className="inline">
                <span className="ok-text">API key guardada en el llavero del sistema</span>
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
              <div className="inline">
                <input type="password" value={keyInput} placeholder="sk-ant-…" onChange={(e) => setKeyInput(e.target.value)} />
                <button className="btn primary" disabled={!keyInput} onClick={saveKey}>
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
            <Switch label="Escalar a Claude automáticamente cuando haga falta" checked={s.auto_escalate} onChange={(v) => set("auto_escalate", v)} />
            <p className="hint">Con el Modo Confidencial activo, Nova nunca contacta con Claude. También puedes decir «esto es confidencial».</p>
          </section>

          <section>
            <h4>Voz</h4>
            <Switch label="Leer en voz alta las respuestas a lo que pido por voz" checked={s.speak_replies} onChange={(v) => set("speak_replies", v)} />
            <div className="inline">
              <select value={s.voice_name} onChange={(e) => set("voice_name", e.target.value)}>
                <option value="">Automática (voz femenina en español)</option>
                {(spanish.length ? spanish : voices).map((v) => (
                  <option key={v.name} value={v.name}>
                    {v.name} · {v.language}
                  </option>
                ))}
              </select>
              <button className="btn" onClick={() => api.speak("Hola, soy Nova. ¿Me oyes bien?", s.voice_name)}>
                Probar
              </button>
            </div>
          </section>
        </div>

        <footer>
          <button className="btn" onClick={onClose}>
            Cancelar
          </button>
          <button className="btn primary" onClick={save}>
            Guardar
          </button>
        </footer>
      </div>
    </div>
  );
}

/** Aplica el tema a <html>. "system" sigue al sistema operativo. */
export function applyTheme(theme: Theme) {
  const dark = theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}
