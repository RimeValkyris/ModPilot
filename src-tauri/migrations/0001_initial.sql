-- Initial ModForge schema.
-- SQLite stores metadata only; Minecraft server files live on the filesystem.

CREATE TABLE IF NOT EXISTS instances (
    id                TEXT PRIMARY KEY NOT NULL,
    name              TEXT NOT NULL,
    minecraft_version TEXT,
    loader            TEXT NOT NULL DEFAULT 'unknown',
    loader_version    TEXT,
    java_installation_id TEXT REFERENCES java_installations(id) ON DELETE SET NULL,
    min_ram_mb        INTEGER NOT NULL DEFAULT 2048,
    max_ram_mb        INTEGER NOT NULL DEFAULT 4096,
    server_directory  TEXT NOT NULL,
    server_jar        TEXT,
    jvm_args          TEXT NOT NULL DEFAULT '[]',
    server_args       TEXT NOT NULL DEFAULT '[]',
    status            TEXT NOT NULL DEFAULT 'stopped',
    auto_start        INTEGER NOT NULL DEFAULT 0,
    auto_restart      INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL,
    last_launched_at  TEXT
);

CREATE TABLE IF NOT EXISTS java_installations (
    id           TEXT PRIMARY KEY NOT NULL,
    version      TEXT NOT NULL,
    vendor       TEXT,
    path         TEXT NOT NULL UNIQUE,
    architecture TEXT NOT NULL,
    is_default   INTEGER NOT NULL DEFAULT 0,
    detected_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS application_settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS launch_history (
    id          TEXT PRIMARY KEY NOT NULL,
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    started_at  TEXT NOT NULL,
    stopped_at  TEXT,
    exit_code   INTEGER,
    status      TEXT NOT NULL DEFAULT 'running'
);

CREATE INDEX IF NOT EXISTS idx_launch_history_instance_id ON launch_history(instance_id);
