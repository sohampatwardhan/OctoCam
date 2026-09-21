import { useQuery } from "@tanstack/react-query"

const CACHE_KEY = "octocam.motion-snapshot.v1"
type Snapshot = { src: string; capturedAt: number }

export function clearMotionSnapshot() {
  try { sessionStorage.removeItem(CACHE_KEY) } catch { /* Storage may be disabled. */ }
}

function readSnapshot(): Snapshot | undefined {
  try {
    const value = JSON.parse(sessionStorage.getItem(CACHE_KEY) ?? "null")
    if (typeof value?.src === "string" && value.src.startsWith("data:image/jpeg;base64,") &&
        Number.isFinite(value.capturedAt) && Date.now() - value.capturedAt < 86400000) {
      return value
    }
  } catch { /* A missing or invalid cache is a normal cold start. */ }
  return undefined
}

async function captureSnapshot(signal: AbortSignal): Promise<Snapshot> {
  const response = await fetch("/snapshot.jpg", { signal, cache: "no-store" })
  if (!response.ok || !response.headers.get("content-type")?.startsWith("image/jpeg")) {
    throw new Error("Camera snapshot unavailable")
  }
  const blob = await response.blob()
  const src = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result))
    reader.onerror = () => reject(new Error("Could not read snapshot"))
    reader.readAsDataURL(blob)
  })
  // Only replace the last good image after the new JPEG decodes successfully.
  const image = new Image()
  image.src = src
  await image.decode()
  signal.throwIfAborted()
  const snapshot = { src, capturedAt: Date.now() }
  try { sessionStorage.setItem(CACHE_KEY, JSON.stringify(snapshot)) } catch { /* Keep the in-memory cache. */ }
  return snapshot
}

/** Keep the scene stable while editing; refresh is explicit and failures retain it. */
export function useMotionSnapshot() {
  return useQuery({
    queryKey: ["motion-snapshot"],
    queryFn: ({ signal }) => captureSnapshot(signal),
    initialData: readSnapshot,
    staleTime: Infinity,
    gcTime: Infinity,
    retry: 2,
    retryDelay: 1500,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  })
}
