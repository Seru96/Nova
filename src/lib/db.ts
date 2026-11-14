// Persistencia local de conversaciones en SQLite (tauri-plugin-sql).
import Database from "@tauri-apps/plugin-sql";
import type { Engine } from "./api";

export interface StoredMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  engine: Engine | null;
  created_at: number;
}

let dbPromise: Promise<Database> | null = null;
function db() {
  dbPromise ??= Database.load("sqlite:nova.db");
  return dbPromise;
}

/** Devuelve la conversación más reciente o crea una nueva. */
export async function currentConversation(): Promise<string> {
  const d = await db();
  const rows = await d.select<{ id: string }[]>(
    "SELECT id FROM conversations ORDER BY created_at DESC LIMIT 1",
  );
  return rows[0]?.id ?? newConversation();
}

export async function newConversation(): Promise<string> {
  const d = await db();
  const id = crypto.randomUUID();
  await d.execute("INSERT INTO conversations (id, created_at) VALUES ($1, $2)", [id, Date.now()]);
  return id;
}

export interface Conversation {
  id: string;
  title: string;
  created_at: number;
  updated_at: number;
}

/** Conversaciones con mensajes, de la más reciente a la más antigua. */
export async function listConversations(): Promise<Conversation[]> {
  const d = await db();
  return d.select<Conversation[]>(
    `SELECT c.id, c.title, c.created_at, COALESCE(MAX(m.created_at), c.created_at) AS updated_at
     FROM conversations c JOIN messages m ON m.conversation_id = c.id
     GROUP BY c.id ORDER BY updated_at DESC`,
  );
}

export async function renameConversation(id: string, title: string): Promise<void> {
  const d = await db();
  await d.execute("UPDATE conversations SET title = $1 WHERE id = $2", [title, id]);
}

export async function deleteConversation(id: string): Promise<void> {
  const d = await db();
  await d.execute("DELETE FROM messages WHERE conversation_id = $1", [id]);
  await d.execute("DELETE FROM conversations WHERE id = $1", [id]);
}

/** Título a partir del primer mensaje: primeras palabras, sin «Oye Nova». */
export function titleFrom(text: string): string {
  const clean = text.replace(/^\s*(oye,?\s+)?nova[,.:!]?\s*/i, "").trim() || text.trim();
  const short = clean.length > 48 ? `${clean.slice(0, 46).trimEnd()}…` : clean;
  return short.charAt(0).toUpperCase() + short.slice(1);
}

export async function loadMessages(conversationId: string): Promise<StoredMessage[]> {
  const d = await db();
  return d.select<StoredMessage[]>(
    "SELECT id, role, content, engine, created_at FROM messages WHERE conversation_id = $1 ORDER BY created_at ASC",
    [conversationId],
  );
}

export async function saveMessage(conversationId: string, m: StoredMessage): Promise<void> {
  const d = await db();
  await d.execute(
    "INSERT OR REPLACE INTO messages (id, conversation_id, role, content, engine, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
    [m.id, conversationId, m.role, m.content, m.engine, m.created_at],
  );
}
