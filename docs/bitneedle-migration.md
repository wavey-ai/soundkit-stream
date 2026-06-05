# Bitneedle Migration Notes

Bitneedle's recent shared source-audio work is the right extraction source, but
the durable format should change while extracting it.

## Current Bitneedle Shape

Relevant files in the Bitneedle repo:

- `apps/press/press-wasm-paths.js` centralizes WASM asset URLs across release
  tool routes including `cut`, `library`, and `plant`.
- `apps/press/source-audio-worker.js` decodes source audio and encodes Opus in a
  worker with `libopus-rs` WASM.
- `apps/press/press-source-audio-cache.js` describes the cache payload as
  `soundkit_opus_packets`.
- `apps/press/press-source-audio-local-store.js` stores one packet per
  IndexedDB row.

That path wraps each Opus packet in an older packet header shape and returns an
array of packet buffers. It is route-shared, not package-shared.

## Target Shape

This repo should become the shared module:

- JS parser and range-seek helpers come from `packages/soundkit-stream-js`.
- Browser encode/transcode calls go through `crates/soundkit-stream-wasm`.
- Durable cache writes `{ stream, index, metadata }` rather than `packets[]`.

## Extraction Steps

1. Replace Bitneedle's local JS frame header builder/parser with imports from
   `@wavey/soundkit-stream`.
2. Keep the existing source decode/resample path until the full transcoder is
   moved behind this repo's WASM API.
3. Replace the worker's packet-array encode result with:
   - `streamBytes`
   - `indexBytes`
   - metadata JSON
4. Replace IndexedDB packet rows with one stream blob and one index blob.
5. Remote cache uploads should persist the stream object and sidecar index as
   separate byte-range addressable objects.

