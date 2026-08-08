import { useEffect, useState } from "react"
import { Aperture, Pause, Play } from "lucide-react"
import { useStatus } from "@/hooks/useStatus"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

type StreamKey = "main" | "sub"

interface PreviewPrefs {
  activeStream: StreamKey
  playing: boolean
}

const CACHE_KEY = "octocam.streamPreview"

function readCache(): PreviewPrefs | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<PreviewPrefs>
    if (parsed.activeStream !== "main" && parsed.activeStream !== "sub") return null
    if (typeof parsed.playing !== "boolean") return null
    return { activeStream: parsed.activeStream, playing: parsed.playing }
  } catch {
    return null
  }
}

function writeCache(prefs: PreviewPrefs) {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(prefs))
  } catch {
    // localStorage unavailable (private mode, quota) — preference just won't persist
  }
}

// Default STOPPED — auto-starting a WebRTC preview on every dashboard load
// would tax the Pi and claim a viewer slot nobody asked for. Mirrors the
// Askama dashboard's `initial_stream` pick: prefer the lighter SD path when
// it exists, since that's the version most likely to be waiting for someone.
export function StreamPreview() {
  const { data: status, isLoading } = useStatus()
  const hasSub = status?.browser_stream_urls.has_sub ?? false

  const [prefs, setPrefs] = useState<PreviewPrefs | null>(() => readCache())
  const [capacityNote, setCapacityNote] = useState(false)

  // First time we learn has_sub from the server (no saved preference yet),
  // pick the real default instead of the "sub unknown" guess.
  useEffect(() => {
    if (prefs === null && status) {
      setPrefs({ activeStream: hasSub ? "sub" : "main", playing: false })
    }
  }, [status, hasSub, prefs])

  useEffect(() => {
    if (prefs) writeCache(prefs)
  }, [prefs])

  const activeStream = prefs?.activeStream ?? (hasSub ? "sub" : "main")
  const playing = prefs?.playing ?? false

  const mainIsFull = Boolean(
    status?.viewers?.main &&
      status.viewers.main.browser + status.viewers.main.rtsp >= status.viewers.main.capacity
  )

  function selectStream(next: StreamKey) {
    if (next === "sub" && !hasSub) return
    if (next === "main" && mainIsFull && hasSub) {
      setCapacityNote(true)
      setPrefs((prev) => ({ activeStream: "sub", playing: prev?.playing ?? false }))
      return
    }
    setCapacityNote(false)
    setPrefs((prev) => ({ activeStream: next, playing: prev?.playing ?? false }))
  }

  function togglePlaying() {
    setPrefs((prev) => ({ activeStream: prev?.activeStream ?? activeStream, playing: !(prev?.playing ?? false) }))
  }

  const urls = status?.browser_stream_urls
  const activeUrl = activeStream === "sub" ? urls?.sub : urls?.main
  const src = playing && activeUrl ? activeUrl : "about:blank"

  return (
    <div className="flex flex-col overflow-hidden rounded-xl bg-card ring-1 ring-foreground/10">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border bg-muted/60 px-3 py-2.5">
        <div
          className="inline-flex overflow-hidden rounded-lg border border-border bg-input"
          role="group"
          aria-label="Preview stream"
        >
          <button
            type="button"
            onClick={() => selectStream("main")}
            aria-pressed={activeStream === "main"}
            className={cn(
              "px-3.5 py-1.5 text-xs font-semibold tracking-wide transition-colors",
              activeStream === "main"
                ? "bg-foreground text-background"
                : "text-muted-foreground hover:bg-card hover:text-foreground"
            )}
          >
            HD
          </button>
          <button
            type="button"
            disabled={!hasSub}
            onClick={() => selectStream("sub")}
            aria-pressed={activeStream === "sub"}
            className={cn(
              "border-l border-border px-3.5 py-1.5 text-xs font-semibold tracking-wide transition-colors disabled:cursor-not-allowed disabled:opacity-40",
              activeStream === "sub"
                ? "bg-foreground text-background"
                : "text-muted-foreground hover:bg-card hover:text-foreground"
            )}
          >
            SD
          </button>
        </div>

        <div className="flex items-center gap-2">
          <Button variant="secondary" size="sm" nativeButton={false} render={<a href="/settings/stream" />}>
            RTSP
          </Button>
          <Button variant="secondary" size="sm" onClick={togglePlaying} aria-pressed={playing}>
            {playing ? <Pause className="size-3.5" /> : <Play className="size-3.5" />}
            {playing ? "Stop" : "Start"}
          </Button>
        </div>
      </div>

      {capacityNote && (
        <p className="border-b border-accent bg-accent px-3 py-2 text-xs font-semibold text-primary">
          HD stream is at capacity — showing the SD stream instead.
        </p>
      )}

      <div className="relative aspect-video bg-black">
        <iframe
          className="size-full"
          src={src}
          title="Live OctoCam stream"
          allow="autoplay; fullscreen"
        />
        {!playing && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 text-white/60">
            <Aperture className={cn("size-7", isLoading && "animate-spin [animation-duration:2.5s]")} />
            <span className="text-sm font-medium">Preview stopped</span>
          </div>
        )}
      </div>
    </div>
  )
}
