import { useEffect, useState } from "react"

/**
 * Live motion state from the server's SSE stream.
 *
 * `/api/status` carries `motion_detected` too, but it is polled every 5s and a
 * motion event can start and clear inside that window. The server already
 * pushes every transition, so subscribe instead of sampling. Returns the
 * polled value until the stream delivers its first frame, and falls back to it
 * again if the stream drops.
 */
export function useMotionEvents(polled: boolean | undefined): boolean {
  const [streamed, setStreamed] = useState<boolean | null>(null)

  useEffect(() => {
    const source = new EventSource("/api/motion/events")

    source.onmessage = (event) => {
      try {
        const payload = JSON.parse(event.data) as { motion_detected?: unknown }
        if (typeof payload.motion_detected === "boolean") {
          setStreamed(payload.motion_detected)
        }
      } catch {
        // Keepalive comments and malformed frames are not state changes.
      }
    }

    // EventSource reconnects on its own; drop back to the polled value in the
    // meantime rather than showing a stale reading as if it were live.
    source.onerror = () => setStreamed(null)

    return () => source.close()
  }, [])

  return streamed ?? polled ?? false
}
