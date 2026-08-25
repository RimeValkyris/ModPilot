-- Links an instance to a Modrinth project so its "Check for Updates"
-- button has something to check against. All nullable: linking is opt-in,
-- and an instance imported from a plain ZIP/folder has no natural project
-- to link to on its own.
ALTER TABLE instances ADD COLUMN modrinth_project_id TEXT;
ALTER TABLE instances ADD COLUMN modrinth_project_title TEXT;
ALTER TABLE instances ADD COLUMN modrinth_version_id TEXT;
