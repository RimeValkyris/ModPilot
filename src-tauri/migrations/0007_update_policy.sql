-- How an instance handles a newly published Modrinth version.
--
--   'off'    - never check automatically (the previous behavior, so
--              existing instances are unchanged by this migration)
--   'notify' - check on a schedule and tell the operator, change nothing
--   'auto'   - check, then back up and install it unattended
--
-- Default is deliberately 'off': silently mutating someone's server files
-- because they once linked a Modrinth project would be a nasty surprise.
ALTER TABLE instances ADD COLUMN update_policy TEXT NOT NULL DEFAULT 'off';
