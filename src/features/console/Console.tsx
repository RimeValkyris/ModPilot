import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { listen } from "@tauri-apps/api/event";
import { Send } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api } from "@/lib/tauri";
import { LOG_EVENT, type LogLinePayload } from "@/types/events";
import type { Instance } from "@/types/instance";

/** Hard cap on retained lines so a chatty server can't grow this unbounded. */
const MAX_LINES = 2000;

interface ConsoleLine {
  id: number;
  stream: "stdout" | "stderr";
  text: string;
}

export function Console({ instance }: { instance: Instance }) {
  const [lines, setLines] = useState<ConsoleLine[]>([]);
  const [command, setCommand] = useState("");
  const [isSending, setIsSending] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);
  const nextIdRef = useRef(0);

  function appendLine(stream: "stdout" | "stderr", text: string) {
    setLines((prev) => {
      const next = [...prev, { id: nextIdRef.current++, stream, text }];
      return next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next;
    });
  }

  // Seed with whatever was already logged to disk before this console was
  // opened, so it doesn't start blank for an already-running server.
  useEffect(() => {
    let cancelled = false;
    setLines([]);
    api
      .readLatestLog(instance.id)
      .then((content) => {
        if (cancelled || !content) return;
        const historic = content
          .split(/\r?\n/)
          .filter((line) => line.length > 0)
          .map((text) => ({ id: nextIdRef.current++, stream: "stdout" as const, text }));
        setLines(historic.slice(-MAX_LINES));
      })
      .catch(() => {
        // No log file yet (never started) - starting empty is correct.
      });
    return () => {
      cancelled = true;
    };
  }, [instance.id]);

  // Live tail: every stdout/stderr line the running process produces.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<LogLinePayload>(LOG_EVENT, (event) => {
      if (event.payload.instanceId !== instance.id) return;
      appendLine(event.payload.stream, event.payload.line);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instance.id]);

  useEffect(() => {
    if (stickToBottomRef.current && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [lines]);

  function handleScroll() {
    const el = scrollRef.current;
    if (!el) return;
    // Only keep auto-scrolling if the user is already near the bottom -
    // scrolling up to read history shouldn't get yanked back down.
    stickToBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const trimmed = command.trim();
    if (!trimmed) return;

    setIsSending(true);
    try {
      await api.sendConsoleCommand(instance.id, trimmed);
      setCommand("");
    } catch (err) {
      toast.error("Failed to send command", { description: String(err) });
    } finally {
      setIsSending(false);
    }
  }

  const isRunning = instance.status === "running";

  return (
    <div className="flex h-[calc(100vh-14rem)] flex-col gap-3">
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto rounded-lg border border-border bg-black/95 p-3 font-mono text-xs leading-relaxed"
      >
        {lines.length === 0 ? (
          <p className="text-zinc-500">No output yet.</p>
        ) : (
          lines.map((line) => (
            <div
              key={line.id}
              className={line.stream === "stderr" ? "text-red-400" : "text-zinc-300"}
            >
              {line.text}
            </div>
          ))
        )}
      </div>
      <form onSubmit={handleSubmit} className="flex gap-2">
        <Input
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          placeholder={isRunning ? "Type a command… (e.g. say Hello)" : "Instance is not running"}
          disabled={!isRunning || isSending}
          className="font-mono"
        />
        <Button type="submit" disabled={!isRunning || isSending || !command.trim()}>
          <Send />
          Send
        </Button>
      </form>
    </div>
  );
}
