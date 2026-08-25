-- Modern Forge/NeoForge (1.17+) servers don't ship a directly-runnable
-- jar - they ship an installer that generates a launch-argfile-based
-- server layout instead (`run.bat`/`run.sh`, `@user_jvm_args.txt`, a
-- loader-specific `@win_args.txt`/`@unix_args.txt`). 'jar' (the existing
-- behavior: `java -jar <server_jar>`) stays the default for everything
-- else; 'argfile' repurposes `server_jar` to hold the path to that
-- loader-specific argfile instead of a jar.
ALTER TABLE instances ADD COLUMN launch_mode TEXT NOT NULL DEFAULT 'jar';
