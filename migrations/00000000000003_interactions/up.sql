CREATE TABLE reactions (key TEXT NOT NULL, account TEXT NOT NULL, reactor TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(key,reactor));
CREATE INDEX reactions_account ON reactions(account);
CREATE TABLE outgoing_mutations (key TEXT PRIMARY KEY NOT NULL, account TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL, UNIQUE(account,id));
PRAGMA user_version = 3;
