import path from "path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// Dev-only: `npm run dev` serves the SPA locally but has no backend of its own,
// so proxy the API + backend routes to a real octocam-web instance (the Pi by
// default, reached through its nginx on 443 — the plain :8080 isn't exposed to
// the LAN). Override with OCTOCAM_DEV_BACKEND, e.g. a local `cargo run` at
// http://127.0.0.1:8080. `secure: false` accepts the Pi's self-signed cert;
// `changeOrigin` sets Host/SNI to the target. The session cookie is
// SameSite=Lax with no Secure flag, so it stores fine on http://localhost —
// login works through the proxy. This block only affects the dev server; the
// production build embeds nothing of it.
const backend = process.env.OCTOCAM_DEV_BACKEND ?? "https://octocam.local"
const proxy = Object.fromEntries(
  ["/api", "/snapshot.jpg", "/backup"].map((p) => [
    p,
    { target: backend, changeOrigin: true, secure: false },
  ]),
)

export default defineConfig({
  base: "/",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(import.meta.dirname, "./src"),
    },
  },
  server: { proxy },
})
