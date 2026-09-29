CREATE TABLE IF NOT EXISTS chats (account TEXT NOT NULL, chat TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(account, chat));
CREATE TABLE IF NOT EXISTS messages (key TEXT PRIMARY KEY NOT NULL, account TEXT NOT NULL, chat TEXT NOT NULL, sender TEXT NOT NULL, message_id TEXT NOT NULL, from_me INTEGER NOT NULL, created_at_ms INTEGER NOT NULL, data TEXT NOT NULL, unread INTEGER NOT NULL DEFAULT 0, UNIQUE(account,chat,sender,message_id,from_me));
CREATE INDEX IF NOT EXISTS timeline ON messages(account,chat,created_at_ms,key);
CREATE TABLE IF NOT EXISTS drafts (account TEXT NOT NULL, chat TEXT NOT NULL, revision INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY(account,chat));
CREATE TABLE IF NOT EXISTS receipts (key TEXT NOT NULL, account TEXT NOT NULL, chat TEXT NOT NULL, recipient TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(key,recipient));
CREATE TABLE IF NOT EXISTS mutations (key TEXT NOT NULL, account TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(key));
CREATE TABLE IF NOT EXISTS aliases (account TEXT NOT NULL, alias TEXT NOT NULL, canonical TEXT NOT NULL, PRIMARY KEY(account,alias));
CREATE TABLE IF NOT EXISTS read_state (account TEXT NOT NULL, chat TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(account,chat));
PRAGMA user_version = 1;
