import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import { runInNewContext } from "node:vm"
import ts from "typescript"
import { QueryClient, QueryObserver } from "@tanstack/react-query"

// Exercise the hook's query options with a real QueryObserver and browser I/O doubles.
test("snapshot persists across reload, survives failed refresh, and clears on logout", async () => {
  const storage = new Map()
  let fail = false
  let requests = 0
  const exports = {}
  runInNewContext(ts.transpileModule(readFileSync(new URL("./useMotionSnapshot.ts", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText, {
    exports,
    require: () => ({ useQuery: (options) => options }),
    sessionStorage: {
      getItem: (key) => storage.get(key),
      setItem: (key, value) => storage.set(key, value),
      removeItem: (key) => storage.delete(key),
    },
    fetch: async () => {
      requests++
      return { ok: !fail, headers: { get: () => "image/jpeg" }, blob: async () => ({}) }
    },
    FileReader: class {
      readAsDataURL() { this.result = "data:image/jpeg;base64,/9j/"; this.onload() }
    },
    Image: class { async decode() {} },
  })
  const client = new QueryClient()
  const options = exports.useMotionSnapshot()
  const observer = new QueryObserver(client, { ...options, retry: false })
  const unsubscribe = observer.subscribe(() => {})
  const first = await observer.refetch()
  assert.ok(first.data.src.startsWith("data:image/jpeg"))
  assert.equal(requests, 1)
  assert.equal(storage.size, 1)
  fail = true
  const failed = await observer.refetch()
  assert.equal(failed.isRefetchError, true)
  assert.equal(failed.data, first.data)
  unsubscribe()
  client.clear()
  const reloaded = new QueryClient()
  const restored = new QueryObserver(reloaded, exports.useMotionSnapshot())
  const stop = restored.subscribe(() => {})
  assert.equal(restored.getCurrentResult().data.src, first.data.src)
  assert.equal(requests, 2, "reload must not request another capture")
  exports.clearMotionSnapshot()
  assert.equal(storage.size, 0)
  stop()
  reloaded.clear()
})
