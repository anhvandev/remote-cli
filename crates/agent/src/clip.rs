//! `clip` Observation: records one display as H.264 (OpenH264) in an MP4 while Actions run, until the screen settles.

use std::{
    io::Cursor,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use image::{imageops, RgbaImage};
use openh264::{
    encoder::{Encoder, EncoderConfig, FrameType},
    formats::{RgbSliceU8, YUVBuffer},
    OpenH264API,
};
use rcli_protocol::agent::Clip;

use crate::{coords, platform::Platform};

/// Capture rate of a clip (10 fps).
const FRAME_EVERY: Duration = Duration::from_millis(100);
/// Longest clip; keeps the reply well inside the Server's 10 s request timeout.
pub const MAX_CLIP: Duration = Duration::from_secs(5);

struct Sample {
    /// Capture time in ms since the clip started.
    at: u64,
    sync: bool,
    /// NAL units, each prefixed with its 4-byte length (MP4 "avcC" layout).
    data: Vec<u8>,
}

/// Records `display` until `done` is set and two captures in a row are equal, or `MAX_CLIP` has passed.
pub fn record(p: &dyn Platform, display: u32, done: &AtomicBool) -> anyhow::Result<Clip> {
    let start = Instant::now();
    let config = EncoderConfig::new().skip_frames(false);
    let mut encoder = Encoder::with_api_config(OpenH264API::from_source(), config)?;
    let (mut sps, mut pps, mut samples) = (vec![], vec![], vec![]);
    let mut size = None;
    let mut prev = None;
    loop {
        let at = start.elapsed();
        let finishing = done.load(Ordering::SeqCst);
        let frame = p.capture(display)?;
        // H.264 4:2:0 needs even dimensions; the first frame fixes the clip size.
        let (w, h) = *size.get_or_insert_with(|| {
            let (w, h) = coords::observed_size(frame.width, frame.height);
            (w.max(2) & !1, h.max(2) & !1)
        });
        let full = RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
            .ok_or_else(|| anyhow::anyhow!("frame size does not match its pixels"))?;
        let rgb = image::DynamicImage::ImageRgba8(imageops::thumbnail(&full, w, h)).to_rgb8();
        let yuv = YUVBuffer::from_rgb8_source(RgbSliceU8::new(&rgb, (w as usize, h as usize)));
        let bits = encoder.encode(&yuv)?;
        let mut data = vec![];
        for layer in (0..bits.num_layers()).filter_map(|i| bits.layer(i)) {
            for nal in (0..layer.nal_count()).filter_map(|i| layer.nal_unit(i)) {
                let nal = strip_start_code(nal);
                match nal.first().map(|b| b & 0x1f) {
                    Some(7) => sps = nal.to_vec(),
                    Some(8) => pps = nal.to_vec(),
                    Some(_) => {
                        data.extend((nal.len() as u32).to_be_bytes());
                        data.extend(nal);
                    }
                    None => {}
                }
            }
        }
        samples.push(Sample { at: at.as_millis() as u64, sync: bits.frame_type() == FrameType::IDR, data });
        let settled = finishing && prev.as_ref() == Some(&frame);
        if settled || at >= MAX_CLIP {
            break;
        }
        prev = Some(frame);
        std::thread::sleep((start + FRAME_EVERY * samples.len() as u32).saturating_duration_since(Instant::now()));
    }
    let (width, height) = size.unwrap();
    let end = samples.last().unwrap().at + FRAME_EVERY.as_millis() as u64;
    let data = mux(&samples, width, height, sps, pps, end)?;
    let mime = "video/mp4".into();
    Ok(Clip { display, width, height, mime, data, duration_ms: end as u32 })
}

fn strip_start_code(nal: &[u8]) -> &[u8] {
    let zeros = nal.iter().take_while(|&&b| b == 0).count();
    if zeros >= 2 && nal.get(zeros) == Some(&1) {
        &nal[zeros + 1..]
    } else {
        nal
    }
}

/// One H.264 track in an MP4, timed by capture time (ms).
fn mux(samples: &[Sample], width: u32, height: u32, sps: Vec<u8>, pps: Vec<u8>, end: u64) -> anyhow::Result<Vec<u8>> {
    let config = mp4::Mp4Config {
        major_brand: "isom".parse()?,
        minor_version: 512,
        compatible_brands: vec!["isom".parse()?, "iso2".parse()?, "avc1".parse()?, "mp41".parse()?],
        timescale: 1000,
    };
    let mut w = mp4::Mp4Writer::write_start(Cursor::new(Vec::new()), &config)?;
    let avc = mp4::AvcConfig { width: width as u16, height: height as u16, seq_param_set: sps, pic_param_set: pps };
    w.add_track(&mp4::TrackConfig::from(mp4::MediaConfig::AvcConfig(avc)))?;
    for (i, s) in samples.iter().enumerate() {
        let next = samples.get(i + 1).map_or(end, |n| n.at);
        let sample = mp4::Mp4Sample {
            start_time: s.at,
            duration: (next - s.at) as u32,
            rendering_offset: 0,
            is_sync: s.sync,
            bytes: s.data.clone().into(),
        };
        w.write_sample(1, &sample)?;
    }
    w.write_end()?;
    Ok(w.into_writer().into_inner())
}
