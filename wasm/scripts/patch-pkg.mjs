// patch-pkg.mjs — post-build fixup for wasm-pack bundler output.
//
// wasm-pack --target bundler emits `"type": "module"` in pkg/package.json,
// which is correct for ESM bundlers.  This script is a hook for any
// additional tweaks; currently it is a no-op placeholder.
//
// Run automatically by: npm run build:bundler
