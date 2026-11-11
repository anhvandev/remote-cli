//! H.264 for Live View: a hardware encoder through RustDesk's `hwcodec` when one starts, openh264 otherwise.
//! This is the one place that picks the encoder; each OS lists its hardware encoders in `HARDWARE`.

use image::RgbaImage;
use openh264::{
    encoder::{BitRate, Encoder, EncoderConfig, FrameRate, RateControlMode},
    formats::{RgbaSliceU8, YUVBuffer},
    OpenH264API,
};

/// `hwcodec` encoders to try, best first. An OS without an entry encodes in software.
#[cfg(target_os = "macos")]
const HARDWARE: &[&str] = &["h264_videotoolbox"];

/// An H.264 encoder for one stream at a fixed size, bitrate and frame rate.
pub struct H264 {
    inner: Inner,
    width: u32,
    height: u32,
    bps: u32,
    fps: f32,
}

enum Inner {
    Software(Box<Encoder>),
    #[cfg(target_os = "macos")]
    Hardware(Box<hardware::Encoder>),
}

impl H264 {
    /// Opens a hardware encoder if `hardware` allows and one starts, else openh264. `width` and `height` are even.
    pub fn open(width: u32, height: u32, bps: u32, fps: f32, hardware: bool) -> anyhow::Result<Self> {
        #[cfg(target_os = "macos")]
        if hardware {
            for name in HARDWARE {
                match hardware::Encoder::open(name, width, height, bps, fps) {
                    Ok(e) => return Ok(Self { inner: Inner::Hardware(Box::new(e)), width, height, bps, fps }),
                    Err(()) => tracing::info!("hardware encoder {name} did not start"),
                }
            }
        }
        let _ = hardware;
        Ok(Self { inner: Inner::Software(Box::new(software(bps, fps)?)), width, height, bps, fps })
    }

    /// The encoder in use, for the log: a `hwcodec` encoder name or "openh264".
    pub fn name(&self) -> &'static str {
        match &self.inner {
            Inner::Software(_) => "openh264",
            #[cfg(target_os = "macos")]
            Inner::Hardware(e) => e.name,
        }
    }

    /// Size this encoder was opened for.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Encodes one frame of the opened size into Annex B H.264; empty when the encoder has nothing to send yet.
    /// A hardware encoder that fails mid-stream is replaced by openh264 for the rest of the stream.
    pub fn encode(&mut self, rgba: &RgbaImage) -> anyhow::Result<Vec<u8>> {
        let yuv = YUVBuffer::from_rgba8_source(RgbaSliceU8::new(rgba, (self.width as usize, self.height as usize)));
        match &mut self.inner {
            Inner::Software(e) => Ok(e.encode(&yuv)?.to_vec()),
            #[cfg(target_os = "macos")]
            Inner::Hardware(e) => match e.encode(&yuv) {
                Ok(data) => Ok(data),
                Err(code) => {
                    tracing::warn!("hardware encoder {} failed ({code}), switching to openh264", e.name);
                    self.inner = Inner::Software(Box::new(software(self.bps, self.fps)?));
                    self.encode(rgba)
                }
            },
        }
    }
}

fn software(bps: u32, fps: f32) -> anyhow::Result<Encoder> {
    // Camera usage, not screen content: openh264's screen-content rate control never spends more than its
    // starting quality, so Quality would look like Balanced. No frame skipping: the level's frame rate holds.
    let config = EncoderConfig::new()
        .rate_control_mode(RateControlMode::Bitrate)
        .bitrate(BitRate::from_bps(bps))
        .skip_frames(false)
        .max_frame_rate(FrameRate::from_hz(fps));
    Ok(Encoder::with_api_config(OpenH264API::from_source(), config)?)
}

#[cfg(target_os = "macos")]
mod hardware {
    use std::time::{Duration, Instant};

    use hwcodec::{
        common::{Quality, RateControl},
        ffmpeg::AVPixelFormat,
        ffmpeg_ram::encode::{EncodeContext, Encoder as Hw},
    };
    use openh264::formats::{YUVBuffer, YUVSource};

    pub struct Encoder {
        pub name: &'static str,
        hw: Hw,
        /// The frame in the encoder's I420 layout, reused.
        buf: Vec<u8>,
        started: Instant,
        /// Whether it has produced any output yet.
        produced: bool,
    }

    impl Encoder {
        pub fn open(name: &'static str, width: u32, height: u32, bps: u32, fps: f32) -> Result<Self, ()> {
            let hw = Hw::new(EncodeContext {
                name: name.into(),
                mc_name: None,
                width: width as i32,
                height: height as i32,
                pixfmt: AVPixelFormat::AV_PIX_FMT_YUV420P,
                align: 0,
                fps: fps.round() as i32,
                // One keyframe at the start; a viewer that needs another gets a new encoder.
                gop: i32::MAX,
                rc: RateControl::RC_DEFAULT,
                quality: Quality::Quality_Default,
                kbs: (bps / 1000) as i32,
                q: -1,
                thread_count: 1,
            })?;
            let buf = vec![0; hw.length as usize];
            Ok(Self { name, hw, buf, started: Instant::now(), produced: false })
        }

        /// `Err` = the encoder is broken (its error code). Empty = no output for this frame yet: VideoToolbox
        /// encodes asynchronously, which `hwcodec` reports as -1, so only a first second with no output at all counts.
        pub fn encode(&mut self, yuv: &YUVBuffer) -> Result<Vec<u8>, i32> {
            let (w, h) = yuv.dimensions();
            let (ys, us, vs) = yuv.strides();
            let planes = [(yuv.y(), ys, h, 0), (yuv.u(), us, h / 2, self.hw.offset[0]), (yuv.v(), vs, h / 2, self.hw.offset[1])];
            for (i, (src, stride, rows, at)) in planes.into_iter().enumerate() {
                let (line, width) = (self.hw.linesize[i] as usize, if i == 0 { w } else { w / 2 });
                for r in 0..rows {
                    let to = at as usize + r * line;
                    self.buf[to..to + width].copy_from_slice(&src[r * stride..r * stride + width]);
                }
            }
            match self.hw.encode(&self.buf, self.started.elapsed().as_millis() as i64) {
                Ok(frames) => {
                    self.produced = true;
                    Ok(frames.iter().flat_map(|f| f.data.iter().copied()).collect())
                }
                Err(-1) if self.produced || self.started.elapsed() < Duration::from_secs(1) => Ok(vec![]),
                Err(code) => Err(code),
            }
        }
    }
}

