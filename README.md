# SoundKit Stream

Standalone SoundKit v2 frame-stream tooling.

This repo owns the durable web format we want for source-audio caches:

- `*.soundkit` is a continuous SoundKit v2 frame stream.
- `*.soundkit.idx` is a compact sidecar byte-offset index.
- `*.soundkit.json` is optional metadata for object stores and players.

The player fetches the sidecar index first, binary-searches by target PCM frame,
then range-fetches the stream from the indexed byte offset. To seek to an exact
time, decode from the nearest earlier frame boundary and trim decoded samples to
the requested target frame.

## Packages

- `packages/soundkit-stream-js`: JavaScript parser, frame scanner, sidecar index
  utilities, and WASM loader helpers.
- `crates/soundkit-stream`: Rust core encoder and index codec.
- `crates/soundkit-stream-wasm`: `wasm-bindgen` wrapper around the Rust encoder.

## Current Encoder Contract

The first Rust encoder path accepts interleaved signed 16-bit PCM and writes:

- Opus payloads encoded with the sibling `libopus-rs` crate.
- SoundKit v2 headers from the sibling `frame-header` crate.
- One sidecar index entry per SoundKit frame.
- `u64le` byte offsets and `u64le` start PCM frames in the sidecar index.

The current Opus path intentionally requires 48 kHz, 1-2 channel PCM. Bitneedle's
current shared source-audio worker already normalizes to this shape before Opus
encoding, so this is the right first extraction point.

## JavaScript Usage

```js
import {
  decodeSoundKitIndex,
  frameForTimeSeconds,
  soundKitRangeRequestForFrame
} from "@wavey/soundkit-stream";

const indexBytes = new Uint8Array(await (await fetch("/audio.soundkit.idx")).arrayBuffer());
const index = decodeSoundKitIndex(indexBytes);
const targetFrame = frameForTimeSeconds(42.25, index.timescale);
const request = soundKitRangeRequestForFrame(index, targetFrame);

const streamResponse = await fetch("/audio.soundkit", {
  headers: request.headers
});
```

## WASM Usage

After building the WASM package:

```js
import init, * as wasm from "./pkg/soundkit_stream_wasm.js";
import { encodePcmI16ToSoundKitOpusStreamWithWasm } from "@wavey/soundkit-stream/wasm";

await init();

const encoded = encodePcmI16ToSoundKitOpusStreamWithWasm(wasm, pcmInt16Array, {
  sampleRate: 48_000,
  channels: 2,
  bitrate: 128_000,
  frameSize: 960
});

const streamBytes = encoded.stream;
const indexBytes = encoded.index;
const metadata = JSON.parse(encoded.metadataJson());
```

For arbitrary source files, compose the existing SoundKit decoder WASM with this
repo's encoder WASM:

```js
import {
  transcodeSourceToSoundKitOpusStreamWithWasm
} from "@wavey/soundkit-stream/transcode";

const result = await transcodeSourceToSoundKitOpusStreamWithWasm({
  decoderModule: soundkitDecoderWasm,
  encoderModule: soundkitStreamWasm,
  chunks: [sourceFileBytes],
  sampleRate: 48_000,
  bitrate: 128_000,
  frameSize: 960
});
```

The current transcode helper expects the decoder to emit 48 kHz 16-bit PCM.
Bitneedle already has this normalization step. Moving that resampler into this
repo is the next extraction step.

## Development

This checkout is wired to sibling local Wavey crates:

- `../frame-header`
- `../libopus-rs`

For a clean GitHub-only clone, switch those crate dependencies to their Wavey
Git URLs or add a Cargo patch file in the consuming workspace.

Run:

```sh
npm run test:js
cargo test --workspace
cargo build -p soundkit-stream-wasm --target wasm32-unknown-unknown
```

`wasm-pack` packaging is exposed as:

```sh
npm run build:wasm
```
