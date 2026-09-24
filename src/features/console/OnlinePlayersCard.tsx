import { useEffect, useState } from "react";
import { Users } from "lucide-react";
import { api, listenWithCleanup } from "@/lib/tauri";
import { PLAYERS_EVENT, type PlayersChangedPayload } from "@/types/events";
import type { Instance } from "@/types/instance";

/**
 * Who is connected right now, derived from the server's own console output
 * (see Rust's `server::players`) rather than polling the server with
 * `list` - so it costs nothing and updates the instant someone joins.
 */
export function OnlinePlayersCard({ instance }: { instance: Instance }) {
  const [players, setPlayers] = useState<string[]>([]);

  useEffect(() => {
    let cancelled = false;
    api
      .listOnlinePlayers(instance.id)
      .then((list) => {
        if (!cancelled) setPlayers(list);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [instance.id]);

  useEffect(() => {
    return listenWithCleanup<PlayersChangedPayload>(PLAYERS_EVENT, (event) => {
      if (event.payload.instanceId !== instance.id) return;
      setPlayers(event.payload.players);
    });
  }, [instance.id]);

  const isRunning = instance.status === "running";

  return (
    <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h2 className="flex items-center gap-1.5 text-sm font-medium">
            <Users className="size-4" />
            Online Now
          </h2>
          <p className="text-xs text-muted-foreground">
            Read from the server console as players join and leave.
          </p>
        </div>
        {isRunning && (
          <span className="shrink-0 text-sm font-medium">
            {players.length} online
          </span>
        )}
      </div>

      {!isRunning ? (
        <p className="text-sm text-muted-foreground">Server isn't running.</p>
      ) : players.length === 0 ? (
        <p className="text-sm text-muted-foreground">Nobody is online right now.</p>
      ) : (
        <ul className="flex flex-wrap gap-2">
          {players.map((name) => (
            <li
              key={name}
              className="flex items-center gap-1.5 rounded-lg border border-border px-2 py-1 text-sm"
            >
              <span className="size-2 rounded-full bg-primary" />
              {name}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
