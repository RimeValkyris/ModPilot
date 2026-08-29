-- Links an instance to the FTB (Feed the Beast) modpack it was installed
-- from, so "Check for Updates" has something to check against - the FTB
-- counterpart to the Modrinth columns added in 0004.
--
-- FTB identifies packs and versions by integer IDs, not slugs, so these are
-- INTEGER rather than TEXT. All nullable: an instance imported from a ZIP,
-- a folder, or Modrinth has no FTB pack to point at.
ALTER TABLE instances ADD COLUMN ftb_pack_id INTEGER;
ALTER TABLE instances ADD COLUMN ftb_pack_name TEXT;
ALTER TABLE instances ADD COLUMN ftb_version_id INTEGER;
