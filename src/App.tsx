import { useCallback, useEffect, useRef, useState } from "react";
import { api, on, toSpeech, type EngineStatus, type Settings, type VoicePhase } from "./lib/api";
import {
  deleteConversation,
  listConversations,
  loadMessages,
  newConversation,
  renameConversation,
  saveMessage,
  titleFrom,
  type Conversation,
  type StoredMessage,
} from "./lib/db";
import { Sidebar } from "./components/Sidebar";
import { ChatView, type Pending } from "./components/ChatView";
import { Composer } from "./components/Composer";
import { SettingsPanel, applyTheme } from "./components/SettingsPanel";
import { Icon } from "./components/Icon";

document.documentElement.dataset.os = navigator.userAgent.includes("Mac") ? "mac" : "win";

const voiceText: Record<VoicePhase, string> = {
  off: "Escucha desactivada",
  loading: "Cargando el oído…",
  idle: "Di «Oye Nova»",
  awake: "Te escucho…",
  transcribing: "Entendiendo…",
};

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [engine, setEngine] = useState<EngineStatus>({ phase: "idle", detail: "", progress: null });
  const [voice, setVoice] = useState<{ phase: VoicePhase; error: string | null }>({ phase: "off", error: null });
  const [heard, setHeard] = useState("");
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [currentId, setCurrentId] = useState<string | null>(null);
  const [messages, setMessages] = useState<StoredMessage[]>([]);
  const [pending, setPending] = useState<Pending | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  const currentRef = useRef<string | null>(null);
  const messagesRef = useRef<StoredMessage[]>([]);
  const pendingRef = useRef<Pending | null>(null);
  const settingsRef = useRef<Settings | null>(null);
  currentRef.current = currentId;
  messagesRef.current = messages;
  pendingRef.current = pending;
  settingsRef.current = settings;

  const flash = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 4500);
  }, []);

  const refreshList = useCallback(async () => setConversations(await listConversations()), []);

  /** Guarda un mensaje en la conversación indicada (o la actual) y lo pinta si sigue abierta. */
  const addMessage = useCallback(async (m: StoredMessage, conv?: string) => {
    const target = conv ?? currentRef.current;
    if (!target) return;
    if (target === currentRef.current) setMessages((prev) => [...prev, m]);
    await saveMessage(target, m);
  }, []);

  // La conversación a la que pertenece la respuesta en curso.
  const pendingConvRef = useRef<string | null>(null);

  const send = useCallback(
    async (text: string, viaVoice: boolean) => {
      const clean = text.trim();
      if (!clean) return;
      let conv = currentRef.current;
      const isFirst = !conv || messagesRef.current.length === 0;
      if (!conv) {
        conv = await newConversation();
        currentRef.current = conv;
        setCurrentId(conv);
        setMessages([]);
        messagesRef.current = [];
      }
      const history = messagesRef.current
        .filter((m) => m.role !== "system")
        .map((m) => ({ role: m.role as "user" | "assistant", content: m.content }));
      await addMessage({ id: crypto.randomUUID(), role: "user", content: clean, engine: null, created_at: Date.now() }, conv);
      if (isFirst) await renameConversation(conv, titleFrom(clean));
      void refreshList();
      const id = crypto.randomUUID();
      pendingConvRef.current = conv;
      setPending({ id, engine: "local", text: "", note: null, actions: [] });
      await api.chatSend(id, history, clean, viaVoice);
    },
    [addMessage, refreshList],
  );
  const sendRef = useRef(send);
  sendRef.current = send;

  useEffect(() => {
    let alive = true;
    const unsubs: Promise<() => void>[] = [];

    (async () => {
      const [s, st] = await Promise.all([api.getSettings(), api.engineStatus()]);
      if (!alive) return;
      setSettings(s);
      applyTheme(s.theme);
      setEngine(st);
      const list = await listConversations();
      setConversations(list);
      if (list[0]) {
        setCurrentId(list[0].id);
        setMessages(await loadMessages(list[0].id));
      }
    })().catch((e) => flash(String(e)));

    unsubs.push(on("engine://status", setEngine));
    unsubs.push(
      on("settings://changed", (s) => {
        setSettings(s);
        applyTheme(s.theme);
      }),
    );
    unsubs.push(on("voice://state", setVoice));
    unsubs.push(on("voice://transcript", setHeard));
    unsubs.push(on("voice://command", (t) => void sendRef.current(t, true)));
    unsubs.push(
      on("chat://route", ({ id, engine, note }) =>
        setPending((p) => (p && p.id === id ? { ...p, engine, note: note ?? p.note, text: "" } : p)),
      ),
    );
    unsubs.push(
      on("chat://action", ({ id, text }) =>
        setPending((p) => (p && p.id === id ? { ...p, actions: [...p.actions, text], text: "" } : p)),
      ),
    );
    unsubs.push(
      on("chat://delta", ({ id, text }) => setPending((p) => (p && p.id === id ? { ...p, text: p.text + text } : p))),
    );
    unsubs.push(
      on("chat://done", ({ id, engine, text, error, via_voice }) => {
        const p = pendingRef.current;
        if (p?.id !== id) return;
        setPending(null);
        const conv = pendingConvRef.current ?? undefined;
        if (error) {
          void addMessage({ id, role: "system", content: error, engine, created_at: Date.now() }, conv);
          return;
        }
        const actions = p.actions.length ? p.actions.map((a) => `› ${a}`).join("\n") + "\n\n" : "";
        void addMessage({ id, role: "assistant", content: actions + text, engine, created_at: Date.now() }, conv).then(
          refreshList,
        );
        if (via_voice && settingsRef.current?.speak_replies) void api.speak(toSpeech(text));
      }),
    );

    return () => {
      alive = false;
      unsubs.forEach((u) => void u.then((f) => f()));
    };
  }, [addMessage, flash, refreshList]);

  const openConversation = async (id: string) => {
    if (id === currentId) return;
    setCurrentId(id);
    setMessages(await loadMessages(id));
  };

  const startNew = () => {
    setCurrentId(null);
    setMessages([]);
  };

  const removeConversation = async (id: string) => {
    await deleteConversation(id);
    if (id === currentId) startNew();
    await refreshList();
  };

  const toggleListening = async () => {
    if (!settings) return;
    try {
      setSettings(await api.setListening(!settings.listening));
    } catch (e) {
      flash(String(e));
    }
  };
  const toggleHalo = async () => {
    if (!settings) return;
    try {
      setSettings(await api.haloSet(!settings.halo));
    } catch (e) {
      flash(String(e));
    }
  };
  const toggleConfidential = async () => settings && setSettings(await api.setConfidential(!settings.confidential));

  const title = conversations.find((c) => c.id === currentId)?.title ?? "Nueva conversación";
  const listening = settings?.listening ?? false;
  const voiceLine = voice.phase === "off" && listening ? "Preparando la escucha…" : voiceText[voice.phase];
  const pct = engine.progress != null ? Math.round(engine.progress * 100) : null;

  return (
    <div className="app">
      <Sidebar
        conversations={conversations}
        currentId={currentId}
        engine={engine}
        voice={voice.phase}
        onNew={startNew}
        onOpen={openConversation}
        onDelete={removeConversation}
        onSettings={() => setShowSettings(true)}
      />

      <main className="main">
        <div className="toolbar" data-tauri-drag-region>
          <div className="title" data-tauri-drag-region>
            <strong>{title}</strong>
            <span className={voice.phase === "awake" || voice.phase === "transcribing" ? "live" : ""}>{voiceLine}</span>
          </div>
          <label className={`switch ${listening ? "on" : ""}`}>
            <button role="switch" aria-checked={listening} disabled={!settings} onClick={toggleListening} />
            <span>Escucha</span>
          </label>
          <label className={`switch ${settings?.halo ? "on" : ""}`} title="H.A.L.O.: controla el ordenador con gestos de la mano">
            <button role="switch" aria-checked={settings?.halo} disabled={!settings} onClick={toggleHalo} />
            <span>Gestos</span>
          </label>
          <label className={`switch amber ${settings?.confidential ? "on" : ""}`}>
            <button role="switch" aria-checked={settings?.confidential} disabled={!settings} onClick={toggleConfidential} />
            <span>Confidencial</span>
          </label>
        </div>

        {settings?.confidential && (
          <div className="confidential-bar">Modo Confidencial: nada sale de este ordenador.</div>
        )}

        {engine.phase !== "ready" &&
          (engine.phase === "error" ? (
            <div className="banner error">
              <div className="grow">No he podido arrancar el modelo local: {engine.detail}</div>
              <button className="btn" onClick={() => api.engineSetup()}>
                Reintentar
              </button>
            </div>
          ) : (
            <div className="banner">
              <div className="grow">
                {engine.detail || "Preparando a Nova…"}
                {engine.phase === "downloading" && <span className="muted"> · solo la primera vez</span>}
                <div className={`progress ${pct == null ? "indet" : ""}`}>
                  <div style={pct != null ? { width: `${pct}%` } : undefined} />
                </div>
              </div>
            </div>
          ))}

        <ChatView
          messages={messages}
          pending={pending}
          userName={settings?.user_name ?? ""}
          ready={engine.phase === "ready"}
          onSuggestion={(t) => void send(t, false)}
        />

        <Composer
          disabled={engine.phase !== "ready"}
          busy={pending !== null}
          listening={listening}
          voice={voice.phase}
          heard={heard}
          onSend={(t) => void send(t, false)}
          onCancel={() => {
            api.chatCancel();
            setPending(null);
          }}
          onTalk={() => api.pushToTalk().catch((e) => flash(String(e)))}
          onStopSpeaking={() => api.stopSpeaking()}
        />
        {voice.error && (
          <div className="sys-msg" style={{ paddingBottom: 8 }}>
            <Icon name="mic" size={14} /> {voice.error}
          </div>
        )}
      </main>

      {toast && <div className="toast">{toast}</div>}
      {showSettings && settings && (
        <SettingsPanel
          settings={settings}
          onClose={() => setShowSettings(false)}
          onSaved={(s) => {
            setSettings(s);
            flash("Ajustes guardados");
          }}
          onError={flash}
        />
      )}
    </div>
  );
}
