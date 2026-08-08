import { useEffect, useRef, useState } from "react"
import { Check, Copy } from "lucide-react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

const COPIED_DURATION_MS = 1600

// Falls back to a hidden textarea + execCommand("copy") when
// navigator.clipboard is unavailable OR rejects — not just absent. The API
// can exist but still fail (denied clipboard-write permission, non-HTTPS
// contexts, older WebKit on the Pi's own kiosk browser), so a rejection
// needs the same fallback as a missing API, not a bubbled error.
async function copyToClipboard(value: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value)
      return
    } catch {
      // fall through to the execCommand fallback below
    }
  }
  return new Promise((resolve, reject) => {
    const textarea = document.createElement("textarea")
    textarea.value = value
    textarea.style.position = "fixed"
    textarea.style.opacity = "0"
    document.body.appendChild(textarea)
    textarea.focus()
    textarea.select()
    try {
      const ok = document.execCommand("copy")
      ok ? resolve() : reject(new Error("execCommand(copy) failed"))
    } catch (error) {
      reject(error)
    } finally {
      document.body.removeChild(textarea)
    }
  })
}

// Copies `value` to the clipboard and shows a brief "Copied" state. Used
// alongside the RTSP stream URLs, which are meant to be pasted into an
// NVR/VLC rather than read.
export function CopyButton({ value, label = "Copy" }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false)
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => {
    return () => clearTimeout(timeoutRef.current)
  }, [])

  async function handleClick() {
    try {
      await copyToClipboard(value)
      setCopied(true)
      clearTimeout(timeoutRef.current)
      timeoutRef.current = setTimeout(() => setCopied(false), COPIED_DURATION_MS)
    } catch {
      // Clipboard access denied or unsupported — leave the field selectable
      // so the user can still copy it manually.
    }
  }

  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      onClick={handleClick}
      aria-label={copied ? "Copied" : label}
      className={cn(copied && "text-success")}
    >
      {copied ? <Check /> : <Copy />}
      {copied ? "Copied" : label}
    </Button>
  )
}
