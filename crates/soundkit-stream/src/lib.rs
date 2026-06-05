use frame_header::{EncodingFlag, Endianness, FrameHeaderV2};
use libopus_rs::{Application, Encoder as OpusEncoder, CELT_FRAME_SIZES_48K};

pub const SOUNDKIT_INDEX_MAGIC: [u8; 8] = *b"SKIDX2\0\0";
pub const SOUNDKIT_INDEX_VERSION: u16 = 1;
pub const SOUNDKIT_INDEX_ENTRY_BYTES: u16 = 16;
pub const SOUNDKIT_INDEX_HEADER_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundKitIndexEntry {
    pub byte_offset: u64,
    pub start_frame: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundKitIndex {
    pub timescale: u32,
    pub duration_frames: u64,
    pub entries: Vec<SoundKitIndexEntry>,
}

#[derive(Debug, Clone)]
pub struct PcmOpusStreamOptions {
    pub sample_rate: u32,
    pub channels: u8,
    pub frame_size: u32,
    pub bitrate: u32,
    pub start_pts: u64,
    pub include_packet_crc32: bool,
}

impl Default for PcmOpusStreamOptions {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 2,
            frame_size: 960,
            bitrate: 128_000,
            start_pts: 0,
            include_packet_crc32: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EncodedSoundKitStream {
    pub stream: Vec<u8>,
    pub index: SoundKitIndex,
    pub packet_count: u64,
}

impl EncodedSoundKitStream {
    pub fn index_bytes(&self) -> Result<Vec<u8>, String> {
        encode_soundkit_index(&self.index)
    }
}

pub fn encode_interleaved_i16_to_opus_soundkit_stream(
    pcm: &[i16],
    options: PcmOpusStreamOptions,
) -> Result<EncodedSoundKitStream, String> {
    validate_pcm_opus_options(&options)?;

    let channels = options.channels as usize;
    if pcm.len() % channels != 0 {
        return Err("PCM sample count must be divisible by channel count".to_string());
    }

    let frame_size = options.frame_size as usize;
    let total_frames = pcm.len() / channels;
    let samples_per_packet = frame_size
        .checked_mul(channels)
        .ok_or_else(|| "Opus packet sample count overflow".to_string())?;

    let mut encoder = OpusEncoder::new(
        options.sample_rate as i32,
        channels,
        Application::RestrictedLowDelay,
    )
    .map_err(|error| error.to_string())?;
    encoder
        .set_bitrate(options.bitrate as i32)
        .map_err(|error| error.to_string())?;
    encoder.set_vbr(false).map_err(|error| error.to_string())?;

    let mut stream = Vec::new();
    let mut entries = Vec::new();
    let mut padded_packet = vec![0i16; samples_per_packet];
    let mut source_frame = 0usize;
    let mut packet_index = 0u64;

    while source_frame < total_frames {
        padded_packet.fill(0);
        let actual_frames = (total_frames - source_frame).min(frame_size);
        let input_offset = source_frame * channels;
        let input_len = actual_frames * channels;
        padded_packet[..input_len].copy_from_slice(&pcm[input_offset..input_offset + input_len]);

        let payload = encoder
            .encode_i16(&padded_packet, frame_size)
            .map_err(|error| error.to_string())?;

        let byte_offset = stream.len() as u64;
        let pts = options
            .start_pts
            .checked_add(source_frame as u64)
            .ok_or_else(|| "SoundKit PTS overflow".to_string())?;
        entries.push(SoundKitIndexEntry {
            byte_offset,
            start_frame: pts,
        });

        let mut header = FrameHeaderV2::new(
            EncodingFlag::Opus,
            payload.len() as u32,
            actual_frames as u32,
            options.sample_rate,
            options.channels,
            0,
            Endianness::LittleEndian,
            Some(packet_index),
            Some(pts),
            None,
        )?;
        if options.include_packet_crc32 {
            header = header.with_packet_crc32(&payload)?;
        }

        header
            .encode(&mut stream)
            .map_err(|error| error.to_string())?;
        stream.extend_from_slice(&payload);

        source_frame += actual_frames;
        packet_index = packet_index
            .checked_add(1)
            .ok_or_else(|| "SoundKit packet index overflow".to_string())?;
    }

    let duration_frames = options
        .start_pts
        .checked_add(total_frames as u64)
        .ok_or_else(|| "SoundKit duration overflow".to_string())?;

    Ok(EncodedSoundKitStream {
        stream,
        index: SoundKitIndex {
            timescale: options.sample_rate,
            duration_frames,
            entries,
        },
        packet_count: packet_index,
    })
}

pub fn encode_soundkit_index(index: &SoundKitIndex) -> Result<Vec<u8>, String> {
    validate_index(index)?;

    let byte_len = SOUNDKIT_INDEX_HEADER_BYTES
        .checked_add(
            index
                .entries
                .len()
                .checked_mul(SOUNDKIT_INDEX_ENTRY_BYTES as usize)
                .ok_or_else(|| "SoundKit index byte length overflow".to_string())?,
        )
        .ok_or_else(|| "SoundKit index byte length overflow".to_string())?;
    let mut output = vec![0u8; byte_len];
    output[..8].copy_from_slice(&SOUNDKIT_INDEX_MAGIC);
    write_u16_le(&mut output, 8, SOUNDKIT_INDEX_VERSION);
    write_u16_le(&mut output, 10, SOUNDKIT_INDEX_ENTRY_BYTES);
    write_u32_le(&mut output, 12, index.timescale);
    write_u64_le(&mut output, 16, index.entries.len() as u64);
    write_u64_le(&mut output, 24, index.duration_frames);

    let mut offset = SOUNDKIT_INDEX_HEADER_BYTES;
    for entry in &index.entries {
        write_u64_le(&mut output, offset, entry.byte_offset);
        write_u64_le(&mut output, offset + 8, entry.start_frame);
        offset += SOUNDKIT_INDEX_ENTRY_BYTES as usize;
    }

    Ok(output)
}

pub fn decode_soundkit_index(bytes: &[u8]) -> Result<SoundKitIndex, String> {
    if bytes.len() < SOUNDKIT_INDEX_HEADER_BYTES {
        return Err("SoundKit index is too small".to_string());
    }
    if bytes[..8] != SOUNDKIT_INDEX_MAGIC {
        return Err("Invalid SoundKit index magic".to_string());
    }

    let version = read_u16_le(bytes, 8)?;
    if version != SOUNDKIT_INDEX_VERSION {
        return Err(format!("Unsupported SoundKit index version {version}"));
    }
    let entry_bytes = read_u16_le(bytes, 10)?;
    if entry_bytes != SOUNDKIT_INDEX_ENTRY_BYTES {
        return Err(format!(
            "Unsupported SoundKit index entry size {entry_bytes}"
        ));
    }

    let timescale = read_u32_le(bytes, 12)?;
    let entry_count = read_u64_le(bytes, 16)?;
    let duration_frames = read_u64_le(bytes, 24)?;
    let entry_count_usize: usize = entry_count
        .try_into()
        .map_err(|_| "SoundKit index entry count is too large".to_string())?;
    let expected_bytes = SOUNDKIT_INDEX_HEADER_BYTES
        .checked_add(
            entry_count_usize
                .checked_mul(entry_bytes as usize)
                .ok_or_else(|| "SoundKit index byte length overflow".to_string())?,
        )
        .ok_or_else(|| "SoundKit index byte length overflow".to_string())?;
    if bytes.len() != expected_bytes {
        return Err("SoundKit index byte length does not match entry count".to_string());
    }

    let mut entries = Vec::with_capacity(entry_count_usize);
    let mut offset = SOUNDKIT_INDEX_HEADER_BYTES;
    for _ in 0..entry_count_usize {
        entries.push(SoundKitIndexEntry {
            byte_offset: read_u64_le(bytes, offset)?,
            start_frame: read_u64_le(bytes, offset + 8)?,
        });
        offset += entry_bytes as usize;
    }

    let index = SoundKitIndex {
        timescale,
        duration_frames,
        entries,
    };
    validate_index(&index)?;
    Ok(index)
}

pub fn seek_entry_for_frame(
    index: &SoundKitIndex,
    target_frame: u64,
) -> Option<&SoundKitIndexEntry> {
    if index.entries.is_empty() {
        return None;
    }

    let mut low = 0usize;
    let mut high = index.entries.len() - 1;
    let mut found = 0usize;
    while low <= high {
        let mid = low + ((high - low) / 2);
        if index.entries[mid].start_frame <= target_frame {
            found = mid;
            low = mid.saturating_add(1);
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }
    Some(&index.entries[found])
}

fn validate_pcm_opus_options(options: &PcmOpusStreamOptions) -> Result<(), String> {
    if options.sample_rate != 48_000 {
        return Err("The current Rust Opus encoder path requires 48 kHz PCM input".to_string());
    }
    if !(1..=2).contains(&options.channels) {
        return Err("The current Rust Opus encoder path supports 1 or 2 channels".to_string());
    }
    if !CELT_FRAME_SIZES_48K.contains(&(options.frame_size as usize)) {
        return Err(format!(
            "Opus frame_size must be one of {:?} at 48 kHz",
            CELT_FRAME_SIZES_48K
        ));
    }
    if options.bitrate == 0 || options.bitrate > 512_000 {
        return Err("Opus bitrate must be between 1 and 512000".to_string());
    }
    Ok(())
}

fn validate_index(index: &SoundKitIndex) -> Result<(), String> {
    if index.timescale == 0 {
        return Err("SoundKit index timescale must be non-zero".to_string());
    }
    let mut previous = None;
    for entry in &index.entries {
        if let Some(previous_start_frame) = previous {
            if entry.start_frame <= previous_start_frame {
                return Err("SoundKit index entries must be sorted by start_frame".to_string());
            }
        }
        previous = Some(entry.start_frame);
    }
    Ok(())
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let window = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "SoundKit index truncated while reading u16".to_string())?;
    Ok(u16::from_le_bytes([window[0], window[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let window = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "SoundKit index truncated while reading u32".to_string())?;
    Ok(u32::from_le_bytes([
        window[0], window[1], window[2], window[3],
    ]))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let window = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| "SoundKit index truncated while reading u64".to_string())?;
    Ok(u64::from_le_bytes([
        window[0], window[1], window[2], window[3], window[4], window[5], window[6], window[7],
    ]))
}

fn write_u16_le(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64_le(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_decodes_sidecar_index() {
        let index = SoundKitIndex {
            timescale: 48_000,
            duration_frames: 1920,
            entries: vec![
                SoundKitIndexEntry {
                    byte_offset: 0,
                    start_frame: 0,
                },
                SoundKitIndexEntry {
                    byte_offset: 123,
                    start_frame: 960,
                },
            ],
        };

        let bytes = encode_soundkit_index(&index).unwrap();
        assert_eq!(bytes.len(), SOUNDKIT_INDEX_HEADER_BYTES + 32);
        assert_eq!(decode_soundkit_index(&bytes).unwrap(), index);
        assert_eq!(seek_entry_for_frame(&index, 959).unwrap().byte_offset, 0);
        assert_eq!(seek_entry_for_frame(&index, 960).unwrap().byte_offset, 123);
    }

    #[test]
    fn encodes_pcm_to_soundkit_v2_opus_stream() {
        let pcm = vec![0i16; 960 * 2 * 2];
        let encoded = encode_interleaved_i16_to_opus_soundkit_stream(
            &pcm,
            PcmOpusStreamOptions {
                sample_rate: 48_000,
                channels: 2,
                frame_size: 960,
                bitrate: 128_000,
                start_pts: 0,
                include_packet_crc32: true,
            },
        )
        .unwrap();

        assert_eq!(encoded.packet_count, 2);
        assert_eq!(encoded.index.entries.len(), 2);
        assert_eq!(encoded.index.entries[0].byte_offset, 0);
        assert_eq!(encoded.index.entries[0].start_frame, 0);
        assert_eq!(encoded.index.entries[1].start_frame, 960);
        assert!(!encoded.stream.is_empty());
        let index_bytes = encoded.index_bytes().unwrap();
        assert_eq!(decode_soundkit_index(&index_bytes).unwrap(), encoded.index);
    }
}
