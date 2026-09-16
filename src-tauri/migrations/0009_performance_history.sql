-- Persistent performance history.
--
-- Until now the only history was the frontend's in-memory ring buffer: ~90
-- samples at the 2s dashboard poll, so about three minutes, thrown away on
-- every app restart. That is enough to watch a GC sawtooth live and useless
-- for the question operators actually ask - "when did this start?", "is it
-- worse than last week?", "does it degrade over the first six hours?".
--
-- Written at a much slower cadence than the dashboard polls. The dashboard
-- wants instant feedback; history wants a trend, and one row per minute per
-- running instance is ~1,440 rows/day/server - small enough that a year of
-- a single server is still only a few hundred thousand narrow rows, which
-- SQLite handles without complaint.
CREATE TABLE IF NOT EXISTS performance_samples (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    instance_id  TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    recorded_at  TEXT NOT NULL,

    cpu_percent  REAL NOT NULL,
    memory_mb    REAL NOT NULL,

    -- Every metric below is nullable, and null means "not measurable at
    -- that moment" rather than zero. A server whose loader has no tick-rate
    -- command never reports TPS at all; one that is still booting does not
    -- answer a ping. Storing a convincing zero for either would make the
    -- history lie in exactly the places it matters most.
    tps          REAL,
    mspt         REAL,
    players      INTEGER,
    ping_ms      INTEGER
);

-- Every query this table serves is "one instance, recent first", so the
-- index carries both columns and the sort falls out of the index order.
CREATE INDEX IF NOT EXISTS idx_performance_samples_instance_time
    ON performance_samples(instance_id, recorded_at DESC);
