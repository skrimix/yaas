//! Minimal live Matroska (EBML) writer for the paced H.264 + PCM stream.

const TRACK_TYPE_VIDEO: u64 = 1;
const TRACK_TYPE_AUDIO: u64 = 2;

pub const VIDEO_TRACK: u64 = 1;
pub const AUDIO_TRACK: u64 = 2;

pub struct Tracks<'a> {
    pub video_config_record: &'a [u8],
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub display_width: u32,
    pub display_height: u32,
    pub video_default_duration_ns: Option<u64>,
    pub audio: bool,
    pub audio_sample_rate: u32,
    pub audio_channels: u16,
}

pub fn ebml_header() -> Vec<u8> {
    master(
        0x1A45_DFA3,
        [
            uint_element(0x4286, 1),
            uint_element(0x42F7, 1),
            uint_element(0x42F2, 4),
            uint_element(0x42F3, 8),
            string_element(0x4282, "matroska"),
            uint_element(0x4287, 4),
            uint_element(0x4285, 2),
        ]
        .concat(),
    )
}

pub fn info_element(duration_ms: Option<i64>) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(uint_element(0x002A_D7B1, 1_000_000));
    body.extend(string_element(0x4D80, "magic-cast"));
    body.extend(string_element(0x5741, "magic-cast"));
    if let Some(duration_ms) = duration_ms {
        body.extend(float_element(0x4489, duration_ms as f64));
    }
    body
}

pub fn tracks_element(config: &Tracks<'_>) -> Vec<u8> {
    let mut body = Vec::new();

    let mut video = Vec::new();
    video.extend(uint_element(0xD7, VIDEO_TRACK));
    video.extend(uint_element(0x73C5, VIDEO_TRACK));
    video.extend(uint_element(0x83, TRACK_TYPE_VIDEO));
    video.extend(uint_element(0x9C, 0));
    if let Some(duration_ns) = config.video_default_duration_ns {
        video.extend(uint_element(0x0023_E383, duration_ns));
    }
    video.extend(string_element(0x86, "V_MPEG4/ISO/AVC"));
    video.extend(binary_element(0x63A2, config.video_config_record));
    video.extend(master(
        0xE0,
        [
            uint_element(0xB0, u64::from(config.pixel_width)),
            uint_element(0xBA, u64::from(config.pixel_height)),
            uint_element(0x54B0, u64::from(config.display_width)),
            uint_element(0x54BA, u64::from(config.display_height)),
        ]
        .concat(),
    ));
    body.extend(master(0xAE, video));

    if config.audio {
        let mut audio = Vec::new();
        audio.extend(uint_element(0xD7, AUDIO_TRACK));
        audio.extend(uint_element(0x73C5, AUDIO_TRACK));
        audio.extend(uint_element(0x83, TRACK_TYPE_AUDIO));
        audio.extend(uint_element(0x9C, 0));
        audio.extend(string_element(0x86, "A_PCM/INT/LIT"));
        audio.extend(master(
            0xE1,
            [
                float_element(0xB5, f64::from(config.audio_sample_rate)),
                uint_element(0x9F, u64::from(config.audio_channels)),
                uint_element(0x6264, 16),
            ]
            .concat(),
        ));
        body.extend(master(0xAE, audio));
    }

    body
}

pub fn live_header(tracks: &Tracks<'_>) -> Vec<u8> {
    let mut output = ebml_header();
    output.extend(element_id(0x1853_8067));
    output.extend([0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    output.extend(master(0x1549_A966, info_element(None)));
    output.extend(master(0x1654_AE6B, tracks_element(tracks)));
    output
}

pub fn packet_cluster(track: u64, pts_ms: i64, keyframe: bool, data: &[u8]) -> Vec<u8> {
    let pts_ms = pts_ms.max(0);
    finish_cluster(
        pts_ms,
        [
            uint_element(0xE7, pts_ms as u64),
            simple_block(track, 0, keyframe, data),
        ]
        .concat(),
    )
}

pub fn finish_cluster(_time_ms: i64, body: Vec<u8>) -> Vec<u8> {
    master(0x1F43_B675, body)
}

pub fn simple_block(track: u64, timecode: i16, keyframe: bool, data: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(4 + data.len());
    body.extend(vint(track));
    body.extend(timecode.to_be_bytes());
    body.push(if keyframe { 0x80 } else { 0x00 });
    body.extend(data);
    binary_element(0xA3, &body)
}

pub fn master(id: u32, body: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend(element_id(id));
    out.extend(size_vint(body.len() as u64));
    out.extend(body);
    out
}

pub fn uint_element(id: u32, value: u64) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len() - 1);
    binary_element(id, &bytes[first..])
}

fn float_element(id: u32, value: f64) -> Vec<u8> {
    binary_element(id, &value.to_be_bytes())
}

fn string_element(id: u32, value: &str) -> Vec<u8> {
    binary_element(id, value.as_bytes())
}

fn binary_element(id: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + data.len());
    out.extend(element_id(id));
    out.extend(size_vint(data.len() as u64));
    out.extend(data);
    out
}

fn element_id(id: u32) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len() - 1);
    bytes[first..].to_vec()
}

fn size_vint(size: u64) -> Vec<u8> {
    vint(size)
}

fn vint(value: u64) -> Vec<u8> {
    for width in 1..=8 {
        let max = (1u64 << (7 * width)) - 2;
        if value <= max {
            let marker = 1u64 << (7 * width);
            let encoded = marker | value;
            let bytes = encoded.to_be_bytes();
            return bytes[8 - width..].to_vec();
        }
    }
    panic!("EBML vint too large: {value}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    use oxideav_core::{Error, MediaType, ReadSeek};
    use oxideav_mkv::avc::annexb_to_avcc;

    #[test]
    fn live_header_uses_unknown_segment_size() {
        let header = live_header(&Tracks {
            video_config_record: &[1, 2, 3],
            pixel_width: 1280,
            pixel_height: 720,
            display_width: 1280,
            display_height: 720,
            video_default_duration_ns: None,
            audio: true,
            audio_sample_rate: 48_000,
            audio_channels: 2,
        });
        let segment = element_id(0x1853_8067);
        let segment_offset = header
            .windows(segment.len())
            .position(|window| window == segment)
            .expect("segment");

        assert_eq!(
            &header[segment_offset + segment.len()..segment_offset + segment.len() + 8],
            &[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
        );
    }

    #[test]
    fn packet_cluster_carries_track_time_and_keyframe_flag() {
        let cluster = packet_cluster(VIDEO_TRACK, 42, true, &[7, 8, 9]);

        assert!(cluster.windows(3).any(|bytes| bytes == [0xE7, 0x81, 42]));
        assert!(
            cluster
                .windows(8)
                .any(|bytes| bytes == [0xA3, 0x87, 0x81, 0, 0, 0x80, 7, 8])
        );
    }

    #[test]
    fn live_stream_round_trips_tracks_packets_and_timestamps() {
        let (stream, video, audio) = test_live_stream();

        let input: Box<dyn ReadSeek> = Box::new(Cursor::new(stream));
        let mut demuxer = oxideav_mkv::demux::open(input, &oxideav_core::NullCodecResolver)
            .expect("open live Matroska");
        assert_eq!(demuxer.streams().len(), 2);
        assert!(
            demuxer
                .streams()
                .iter()
                .any(|stream| stream.params.media_type == MediaType::Video)
        );
        assert!(
            demuxer
                .streams()
                .iter()
                .any(|stream| stream.params.media_type == MediaType::Audio)
        );

        let first = demuxer.next_packet().expect("video packet");
        let second = demuxer.next_packet().expect("audio packet");
        assert_eq!(first.pts, Some(42));
        assert_eq!(first.data, video);
        assert!(first.flags.keyframe);
        assert_eq!(second.pts, Some(80));
        assert_eq!(second.data, audio);
        assert!(matches!(demuxer.next_packet(), Err(Error::Eof)));
    }

    #[test]
    fn ffprobe_accepts_live_stream_layout() {
        if std::process::Command::new("ffprobe")
            .arg("-version")
            .output()
            .is_err()
        {
            return;
        }

        let (stream, _, _) = test_live_stream();
        let path =
            std::env::temp_dir().join(format!("magic-cast-live-mkv-{}.mkv", uuid::Uuid::new_v4()));
        std::fs::write(&path, stream).expect("write live Matroska fixture");
        let output = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "default=nw=1",
            ])
            .arg(&path)
            .output()
            .expect("run ffprobe");
        let _ = std::fs::remove_file(path);

        assert!(
            output.status.success(),
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("codec_name=h264"));
        assert!(stdout.contains("codec_name=pcm_s16le"));
    }

    fn test_live_stream() -> (Vec<u8>, [u8; 5], [u8; 4]) {
        let annex_b = [
            0, 0, 0, 1, 0x67, 0x64, 0, 0x28, 0xDE, 0xAD, 0, 0, 0, 1, 0x68, 0xEE, 0x3C, 0x80,
        ];
        let config_record = annexb_to_avcc(&annex_b).config_record;
        let mut stream = live_header(&Tracks {
            video_config_record: &config_record,
            pixel_width: 1280,
            pixel_height: 720,
            display_width: 1280,
            display_height: 720,
            video_default_duration_ns: None,
            audio: true,
            audio_sample_rate: 48_000,
            audio_channels: 2,
        });
        let video = [0, 0, 0, 1, 0x65];
        let audio = [1, 2, 3, 4];
        stream.extend(packet_cluster(VIDEO_TRACK, 42, true, &video));
        stream.extend(packet_cluster(AUDIO_TRACK, 80, true, &audio));
        (stream, video, audio)
    }
}
