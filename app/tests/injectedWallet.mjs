// ============================================================================
// injectedWallet.mjs — unit tests for the DIG Browser injected-wallet adapter
// ============================================================================
//
// Mirrors the repo's existing plain-Node test convention (node:assert, run with
// `node tests/injectedWallet.mjs`), with no Jest/Vitest dependency. The source
// under test is TypeScript, so we transpile it with the project's own
// `typescript` devDependency (transpileModule — faithful, not a regex strip)
// and evaluate the emitted JS as an ES module. The test is therefore pinned to
// the ACTUAL source: a logic regression in injectedWallet.ts fails it.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const ts = require("typescript");

const __dirname = dirname(fileURLToPath(import.meta.url));
const srcPath = join(__dirname, "..", "app", "lib", "injectedWallet.ts");

const source = readFileSync(srcPath, "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  fileName: "injectedWallet.ts",
});
const url = "data:text/javascript;base64," + Buffer.from(outputText).toString("base64");
const mod = await import(url);

// Provide a fake global `window` we can swap between tests.
function setWindow(w) {
  globalThis.window = w;
}

// ---------------------------------------------------------------------------
// isInjectedAvailable — detects ONLY on the unspoofable isDIG marker
// ---------------------------------------------------------------------------
setWindow(undefined);
assert.equal(mod.isInjectedAvailable(), false, "no window → not available");

setWindow({}); // window present, no chia
assert.equal(mod.isInjectedAvailable(), false, "no window.chia → not available");

setWindow({ chia: { request() {} } }); // chia present but not isDIG (other extension)
assert.equal(
  mod.isInjectedAvailable(),
  false,
  "window.chia without isDIG → not available (don't hijack other wallets)"
);

setWindow({ chia: { isDIG: true, request() {} } });
assert.equal(mod.isInjectedAvailable(), true, "window.chia.isDIG → available");

// ---------------------------------------------------------------------------
// injectedSessionSupports — exactly the demo's three RPCs, nothing else
// ---------------------------------------------------------------------------
assert.equal(mod.injectedSessionSupports("chia_getAddress"), true);
assert.equal(mod.injectedSessionSupports("chip0002_getAssetCoins"), true);
assert.equal(mod.injectedSessionSupports("chip0002_signCoinSpends"), true);
assert.equal(mod.injectedSessionSupports("chip0002_signMessage"), false, "unsupported RPC rejected");
assert.equal(mod.injectedSessionSupports("totally_unknown"), false);

// ---------------------------------------------------------------------------
// injectedRequest — forwards {method, params} and returns the provider's data
// ---------------------------------------------------------------------------
{
  const calls = [];
  setWindow({
    chia: {
      isDIG: true,
      request(args) {
        calls.push(args);
        return Promise.resolve({ address: "xch1example" });
      },
    },
  });
  const resp = await mod.injectedRequest("chia_getAddress", { foo: 1 });
  assert.deepEqual(resp, { address: "xch1example" }, "returns provider data");
  assert.deepEqual(
    calls[0],
    { method: "chia_getAddress", params: { foo: 1 } },
    "forwards exact {method, params} envelope"
  );
}

// injectedRequest rejects unsupported methods BEFORE hitting the provider
{
  let hit = false;
  setWindow({ chia: { isDIG: true, request() { hit = true; } } });
  await assert.rejects(
    () => mod.injectedRequest("chip0002_signMessage", {}),
    /does not support/,
    "unsupported method rejected up front"
  );
  assert.equal(hit, false, "provider not called for unsupported method");
}

// injectedRequest throws a clear error when the provider is absent
{
  setWindow({}); // no chia
  await assert.rejects(
    () => mod.injectedRequest("chia_getAddress", {}),
    /DIG Browser wallet is not available/,
    "absent provider → actionable error"
  );
}

// ---------------------------------------------------------------------------
// injectedConnect — calls provider.connect with the eager flag; tolerates none
// ---------------------------------------------------------------------------
{
  const args = [];
  setWindow({
    chia: {
      isDIG: true,
      request() {},
      connect(eager) {
        args.push(eager);
        return Promise.resolve(true);
      },
    },
  });
  await mod.injectedConnect(false);
  await mod.injectedConnect(true);
  assert.deepEqual(args, [false, true], "passes eager flag through to provider.connect");
}
{
  // Older provider with no connect() must not throw.
  setWindow({ chia: { isDIG: true, request() {} } });
  await mod.injectedConnect(false);
}
{
  setWindow({}); // absent provider
  await assert.rejects(() => mod.injectedConnect(), /not available/);
}

console.log("injectedWallet.mjs: all assertions passed");
