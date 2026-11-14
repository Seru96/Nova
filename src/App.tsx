import { useCallback, useEffect, useRef, useState } from "react";
import { api, on, toSpeech, type Engine, type EngineStatus, type Settings, type VoicePhase } from "./lib/api";
import { currentConversation, loadMessages, newConversation, saveMessage, type StoredMessage } from "./lib/db";
import { Header } from "./components/Header";
import { ChatView, type Pending } from "./components/ChatView";
import { Composer } from "./components/Composer";
import { SettingsPanel } from "./components/SettingsPanel";
import { EngineBanner } from "./components/EngineBanner";

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [engine, setEngine] = useState<EngineStatus>({ phase: "idle", detail: "", progress: null });
  const [voice, setVoice] = useState<{ phase: VoicePhase; error: string | null }>({ phase: "off", error: null });
  const [heard, setHeard] = useState<string>("");
  const [messages, setMessages] = useState<StoredMessage[]>([]);
  const [pending, setPending] = useState<Pending | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  const conversationRef = useRef<string | null>(null);
  const messagesRef = useRef<StoredMessage[]>([]);
  const pendingRef = useRef<Pending | null>(null);
  const settingsRef = useRef<Settings | null>(null);
  messagesRef.current = messages;
  pendingRef.current = pending;
  settingsRef.current = settings;

  const flash = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 4000);
  }, []);

  const addMessage = useCallback(async (m: StoredMessage) => {
    setMessages((prev) => [...prev, m]);
    if (conversationRef.current) await saveMessage(conversationRef.current, m);
  }, []);

  const send = useCallback(
    async (text: string, viaVoice: boolean) => {
      const clean = text.trim();
      if (!clean) return;
      const history = messagesRef.current
        .filter((m) => m.role !== "system")
        .map((m) => ({ role: m.role as "user" | "assistant", content: m.content }));
      await addMessage({ id: crypto.randomUUID(), role: "user", content: clean, engine: null, created_at: Date.now() });
      const id = crypto.randomUUID();
      setPending({ id, engine: "local", text: "", note: null });
      await api.chatSend(id, history, clean, viaVoice);
    },
    [addMessage],
  );
  const sendRef = useRef(send);
  sendRef.current = send;

  // Carga inicial + suscripción a eventos del backend.
  useEffect(() => {
    let alive = true;
    const unsubs: Promise<() => void>[] = [];

    (async () => {
      const [s, st] = await Promise.all([api.getSettings(), api.engineStatus()]);
      if (!alive) return;
      setSettings(s);
      setEngine(st);
      const conv = await currentConversation();
      conversationRef.current = conv;
      setMessages(await loadMessages(conv));
    })().catch((e) => flash(String(e)));

    unsubs.push(on("engine://status", setEngine));
    unsubs.push(on("settings://changed", setSettings));
    unsubs.push(on("voice://state", setVoice));
    unsubs.push(on("voice://transcript", (t) => setHeard(t)));
    unsubs.push(on("voice://command", (t) => void sendRef.current(t, true)));
    unsubs.push(
      on("chat://route", ({ id, engine, note }) => {
        setPending((p) => (p && p.id === id ? { ...p, engine, note: note ?? p.note, text: "" } : p));
      }),
    );
    unsubs.push(
      on("chat://delta", ({ id, text }) => {
        setPending((p) => (p && p.id === id ? { ...p, text: p.text + text } : p));
      }),
    );
    unsubs.push(
      on("chat://done", ({ id, engine, text, error, via_voice }) => {
        if (pendingRef.current?.id !== id) return;
        setPending(null);
        if (error) {
          void addMessage({ id, role: "system", content: error, engine, created_at: Date.now() });
          return;
        }
        void addMessage({ id, role: "assistant", content: text, engine, created_at: Date.now() });
        if (via_voice && settingsRef.current?.speak_replies) void api.speak(toSpeech(text));
      }),
    );

    return () => {
      alive = false;
      unsubs.forEach((u) => void u.then((f) => f()));
    };
  }, [addMessage, flash]);

  const toggleListening = async (on: boolean) => {
    try {
      setSettings(await api.setListening(on));
    } catch (e) {
      flash(String(e));
    }
  };

  const toggleConfidential = async (on: boolean) => setSettings(await api.setConfidential(on));

  const resetConversation = async () => {
    api.chatCancel();
    setPending(null);
    conversationRef.current = await newConversation();
    setMessages([]);
  };

  const engineLabel: Record<Engine, string> = { local: "Local", claude: "Claude", system: "Nova" };

  return (
    <div className="flex h-screen flex-col bg-[var(--bg)] text-[var(--fg)]">
      <Header
        settings={settings}
        voice={voice.phase}
        engineReady={engine.phase === "ready"}
        onListening={toggleListening}
        onConfidential={toggleConfidential}
        onSettings={() => setShowSettings(true)}
        onNew={resetConversation}
      />
      <EngineBanner status={engine} onRetry={() => api.engineSetup()} />
      <ChatView messages={messages} pending={pending} engineLabel={engineLabel} />
      <Composer
        disabled={engine.phase !== "ready"}
        busy={pending !== null}
        listening={settings?.listening ?? false}
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
      {voice.error && <div className="px-4 pb-2 text-xs text-red-500">{voice.error}</div>}
      {toast && (
        <div className="fixed bottom-24 left-1/2 max-w-[90%] -translate-x-1/2 rounded-lg bg-black/80 px-4 py-2 text-sm text-white shadow-lg">
          {toast}
        </div>
      )}
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
