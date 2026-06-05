use soundkit_stream::{
    decode_soundkit_index, encode_interleaved_i16_to_opus_soundkit_stream, seek_entry_for_frame,
    PcmOpusStreamOptions,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmEncodedSoundKitStream {
    stream: Vec<u8>,
    index: Vec<u8>,
    metadata_json: String,
    packet_count: u64,
}

#[wasm_bindgen]
impl WasmEncodedSoundKitStream {
    #[wasm_bindgen(getter)]
    pub fn stream(&self) -> Vec<u8> {
        self.stream.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn index(&self) -> Vec<u8> {
        self.index.clone()
    }

    #[wasm_bindgen(js_name = metadataJson)]
    pub fn metadata_json(&self) -> String {
        self.metadata_json.clone()
    }

    #[wasm_bindgen(js_name = packetCount)]
    pub fn packet_count(&self) -> String {
        self.packet_count.to_string()
    }
}

#[wasm_bindgen(js_name = encodePcmI16ToSoundKitOpusStream)]
pub fn encode_pcm_i16_to_soundkit_opus_stream(
    pcm: &[i16],
    sample_rate: u32,
    channels: u8,
    bitrate: u32,
    frame_size: u32,
) -> Result<WasmEncodedSoundKitStream, JsValue> {
    let encoded = encode_interleaved_i16_to_opus_soundkit_stream(
        pcm,
        PcmOpusStreamOptions {
            sample_rate,
            channels,
            frame_size,
            bitrate,
            start_pts: 0,
            include_packet_crc32: true,
        },
    )
    .map_err(js_error)?;
    let index = encoded.index_bytes().map_err(js_error)?;
    let metadata_json = format!(
        concat!(
            "{{",
            "\"format\":\"soundkit-v2-opus-frame-stream\",",
            "\"indexFormat\":\"soundkit-v2-sidecar-index\",",
            "\"codec\":\"opus\",",
            "\"sampleRate\":{},",
            "\"channels\":{},",
            "\"frameSize\":{},",
            "\"bitrate\":{},",
            "\"durationFrames\":\"{}\",",
            "\"packetCount\":\"{}\",",
            "\"streamBytes\":{},",
            "\"indexBytes\":{}",
            "}}"
        ),
        encoded.index.timescale,
        channels,
        frame_size,
        bitrate,
        encoded.index.duration_frames,
        encoded.packet_count,
        encoded.stream.len(),
        index.len()
    );

    Ok(WasmEncodedSoundKitStream {
        stream: encoded.stream,
        index,
        metadata_json,
        packet_count: encoded.packet_count,
    })
}

#[wasm_bindgen(js_name = decodeSoundKitIndexMetadata)]
pub fn decode_soundkit_index_metadata(index_bytes: &[u8]) -> Result<String, JsValue> {
    let index = decode_soundkit_index(index_bytes).map_err(js_error)?;
    Ok(format!(
        concat!(
            "{{",
            "\"version\":1,",
            "\"timescale\":{},",
            "\"durationFrames\":\"{}\",",
            "\"entryCount\":\"{}\"",
            "}}"
        ),
        index.timescale,
        index.duration_frames,
        index.entries.len()
    ))
}

#[wasm_bindgen(js_name = seekByteOffsetForFrame)]
pub fn seek_byte_offset_for_frame(
    index_bytes: &[u8],
    target_frame: &str,
) -> Result<String, JsValue> {
    let index = decode_soundkit_index(index_bytes).map_err(js_error)?;
    let target_frame = target_frame
        .parse::<u64>()
        .map_err(|_| js_error("targetFrame must be a decimal u64 string".to_string()))?;
    let entry = seek_entry_for_frame(&index, target_frame)
        .ok_or_else(|| js_error("SoundKit index has no entries".to_string()))?;
    Ok(entry.byte_offset.to_string())
}

#[wasm_bindgen(js_name = soundKitStreamWasmVersion)]
pub fn soundkit_stream_wasm_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn js_error(message: String) -> JsValue {
    JsValue::from_str(&message)
}
