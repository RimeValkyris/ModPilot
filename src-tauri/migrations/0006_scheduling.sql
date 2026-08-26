-- Automated restarts, backups, and backup retention.
--
-- Schedules are stored as a small text form rather than separate
-- mode/value columns so one nullable column expresses everything:
--   NULL          - disabled
--   "every:6"     - every 6 hours
--   "daily:04:00" - every day at 04:00 local time
-- Both forms are parsed by `server::scheduler`.
ALTER TABLE instances ADD COLUMN restart_schedule TEXT;
ALTER TABLE instances ADD COLUMN backup_schedule TEXT;

-- How many world backups to keep when a scheduled backup runs. 0 keeps
-- everything (the previous behavior, so existing instances are unchanged).
ALTER TABLE instances ADD COLUMN backup_keep_last INTEGER NOT NULL DEFAULT 0;
