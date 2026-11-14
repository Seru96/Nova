// Puente tipado con el backend de Rust (comandos y eventos de Tauri).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Personality = "humor" | "profesional" | "amable" | "sarcastica" | "personalizada";

export interface Settings {
  user_name: string;
  personality: Personality;
  personality_custom: string;
  confidential: boolean;
  auto_escalate: boolean;
  claude_model: string;
  listening: boolean;
  speak_replies: boolean;
  voice_name: string;
  app_control: boolean;
  theme: Theme;
}

export type Theme = "dark" | "light" | "system";

export type EnginePhase = "idle" | "downloading" | "starting" | "ready" | "error";
export interface EngineStatus {
  phase: EnginePhase;
  detail: string;
  progress: number | null;
}

export type Engine = "local" | "claude" | "system";
export type VoicePhase = "off" | "loading" | "idle" | "awake" | "transcribing";

export interface ChatTurn {
  role: "user" | "assistant";
  content: string;
}

export interface VoiceInfo {
  name: string;
  language: string;
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  setConfidential: (on: boolean) => invoke<Settings>("set_confidential", { on }),
  setListening: (on: boolean) => invoke<Settings>("set_listening", { on }),
  hasApiKey: () => invoke<boolean>("has_api_key"),
  setApiKey: (key: string) => invoke<void>("set_api_key", { key }),
  clearApiKey: () => invoke<void>("clear_api_key"),
  engineStatus: () => invoke<EngineStatus>("engine_status"),
  engineSetup: () => invoke<void>("engine_setup"),
  chatSend: (id: string, history: ChatTurn[], text: string, viaVoice: boolean) =>
    invoke<void>("chat_send", { id, history, text, viaVoice }),
  chatCancel: () => invoke<void>("chat_cancel"),
  pushToTalk: () => invoke<void>("voice_push_to_talk"),
  speak: (text: string, voice?: string) => invoke<void>("speak", { text, voice: voice ?? null }),
  stopSpeaking: () => invoke<void>("stop_speaking"),
  listVoices: () => invoke<VoiceInfo[]>("list_voices"),
};

export interface Events {
  "engine://status": EngineStatus;
  "settings://changed": Settings;
  "chat://route": { id: string; engine: Engine; note: string | null };
  "chat://delta": { id: string; text: string };
  "chat://action": { id: string; text: string };
  "chat://done": { id: string; engine: Engine; text: string; error: string | null; via_voice: boolean };
  "voice://state": { phase: VoicePhase; error: string | null };
  "voice://transcript": string;
  "voice://command": string;
  "voice://wake": null;
}

export function on<K extends keyof Events>(event: K, cb: (payload: Events[K]) => void): Promise<UnlistenFn> {
  return listen<Events[K]>(event, (e) => cb(e.payload));
}

/** Limpia Markdown para leerlo en voz alta. */
export function toSpeech(text: string): string {
  return text
    .replace(/```[\s\S]*?```/g, " (te dejo el código en pantalla) ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/[*_~>|]/g, "")
    .replace(/^\s*[-+]\s+/gm, "")
    .replace(/\s+/g, " ")
    .trim();
}
