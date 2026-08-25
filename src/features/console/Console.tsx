import { memo, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, Send } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api } from "@/lib/tauri";
import { useAppSetting, useBoolAppSetting } from "@/hooks/useAppSetting";
import { LOG_EVENT, type LogLinePayload } from "@/types/events";
import type { Instance } from "@/types/instance";

/** How long a "starting" instance can go without printing a single new
 * console line before it's flagged as possibly stuck. Real mod-heavy
 * packs can legitimately be quiet for a while during CPU-bound init work,
 * but a *network* hang (a mod's update checker stuck on a dead
 * connection, no timeout set) produces total silence with idle CPU for as
 * long as the operator lets it sit - this is long enough to avoid flagging
 * a merely-slow pack, short enough to catch a real hang far before an
 * operator has to notice it themselves after 30 minutes of nothing. */
const STUCK_STARTING_THRESHOLD_MS = 3 * 60 * 1000;

interface ConsoleLine {
  id: number;
  stream: "stdout" | "stderr";
  text: string;
}

const FONT_SIZE_CLASS: Record<string, string> = {
  xs: "text-xs",
  sm: "text-sm",
  base: "text-base",
};

/**
 * Memoized so appending one new line doesn't re-render every previous line -
 * `lines` grows via `[...prev, newItem]`, which keeps the same object
 * reference for every existing entry, so this only actually re-renders the
 * row(s) whose props changed (i.e. the new one, or all of them if
 * `wordWrap` itself changes). Without this, React would reconcile up to
 * `maxLines` DOM nodes on every single incoming console line.
 */
const ConsoleLineRow = memo(function ConsoleLineRow({
  line,
  wordWrap,
}: {
  line: ConsoleLine;
  wordWrap: boolean;
}) {
  return (
    <div
      className={`${line.stream === "stderr" ? "text-red-400" : "text-zinc-300"} ${
        wordWrap ? "whitespace-pre-wrap wrap-break-word" : "whitespace-pre"
      }`}
    >
      {line.text}
    </div>
  );
});

export function Console({ instance }: { instance: Instance }) {
  const [lines, setLines] = useState<ConsoleLine[]>([]);
  const [command, setCommand] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [isStuck, setIsStuck] = useState(false);
  const [isForceStoppingStuck, setIsForceStoppingStuck] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);
  const nextIdRef = useRef(0);
  const lastActivityAtRef = useRef(Date.now());

  const { value: maxLinesSetting } = useAppSetting("console_max_lines", "2000");
  const { value: fontSize } = useAppSetting("console_font_size", "xs");
  const { value: wordWrap } = useBoolAppSetting("console_word_wrap", false);
  const maxLines = Number(maxLinesSetting) || 2000;

  function appendLine(stream: "stdout" | "stderr", text: string) {
    lastActivityAtRef.current = Date.now();
    setIsStuck(false);
    setLines((prev) => {
      const next = [...prev, { id: nextIdRef.current++, stream, text }];
      return next.length > maxLines ? next.slice(next.length - maxLines) : next;
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
        setLines(historic.slice(-maxLines));
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

  // Watchdog: a "starting" instance that's gone quiet for too long is
  // flagged so the operator finds out from ModpackPilot instead of by
  // staring at a frozen console themselves - this is what would have
  // caught a mod's hung update-checker call within minutes instead of
  // after 30 of silence.
  useEffect(() => {
    if (instance.status !== "starting") {
      setIsStuck(false);
      return;
    }
    lastActivityAtRef.current = Date.now();
    setIsStuck(false);

    const interval = setInterval(() => {
      setIsStuck(Date.now() - lastActivityAtRef.current > STUCK_STARTING_THRESHOLD_MS);
    }, 10_000);
    return () => clearInterval(interval);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instance.status, instance.id]);

  async function handleForceStopStuck() {
    setIsForceStoppingStuck(true);
    try {
      await api.forceStopInstance(instance.id);
    } catch (err) {
      toast.error("Failed to force stop instance", { description: String(err) });
    } finally {
      setIsForceStoppingStuck(false);
    }
  }

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
      {isStuck && (
        <div className="flex items-center justify-between gap-3 rounded-lg border border-amber-500/30 bg-amber-500/5 p-3">
          <p className="flex items-center gap-1.5 text-sm text-amber-600 dark:text-amber-400">
            <AlertTriangle className="size-4 shrink-0" />
            No console output for {Math.floor(STUCK_STARTING_THRESHOLD_MS / 60_000)}+ minutes -
            this may be stuck (e.g. a mod's update checker hanging on a dead network
            connection), not just slow.
          </p>
          <Button
            variant="outline"
            size="sm"
            className="shrink-0"
            disabled={isForceStoppingStuck}
            onClick={handleForceStopStuck}
          >
            {isForceStoppingStuck ? "Stopping…" : "Force Stop"}
          </Button>
        </div>
      )}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className={`console-scrollbar flex-1 overflow-x-auto overflow-y-auto rounded-lg border border-border bg-black/95 p-3 font-mono leading-relaxed ${FONT_SIZE_CLASS[fontSize] ?? "text-xs"}`}
      >
        {lines.length === 0 ? (
          <p className="text-zinc-500">No output yet.</p>
        ) : (
          lines.map((line) => <ConsoleLineRow key={line.id} line={line} wordWrap={wordWrap} />)
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
