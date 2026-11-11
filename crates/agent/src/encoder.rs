//! H.264 for Live View: a hardware encoder through RustDesk's `hwcodec` when one starts, openh264 otherwise.
//! This is the one place that picks the encoder; each OS lists its hardware encoders in `HARDWARE`. Without the
//! `hwcodec` feature every build encodes with openh264.

use image::RgbaImage;
use openh264::{
    encoder::{BitRate, Encoder, EncoderConfig, FrameRate, RateControlMode},
    formats::{RgbaSliceU8, YUVBuffer},
    OpenH264API,
};

/// `hwcodec` encoders to try, best first. An OS without an entry encodes in software.
#[cfg(all(feature = "hwcodec", target_os = "macos"))]
const HARDWARE: &[&str] = &["h264_videotoolbox"];
/// Windows: the GPU vendor's encoder (NVIDIA, AMD, Intel), then Media Foundation. RustDesk's FFmpeg build has no
/// `h264_mf`, so that one starts only with an FFmpeg configured `--enable-mediafoundation --enable-encoder=h264_mf`.
#[cfg(all(feature = "hwcodec", windows))]
const HARDWARE: &[&str] = &["h264_nvenc", "h264_amf", "h264_qsv", "h264_mf"];

/// An H.264 encoder for one stream at a fixed size, bitrate and frame rate.
pub struct H264 {
    inner: Inner,
    width: u32,
    height: u32,
}

enum Inner {
    Software(Box<Encoder>),
    #[cfg(all(feature = "hwcodec", any(target_os = "macos", windows)))]
    Hardware(Box<hardware::Encoder>),
}

impl H264 {
    /// Opens a hardware encoder if `hardware` allows and one starts, else openh264. `width` and `height` are even.
    pub fn open(width: u32, height: u32, bps: u32, fps: f32, hardware: bool) -> anyhow::Result<Self> {
        #[cfg(all(feature = "hwcodec", any(target_os = "macos", windows)))]
        if hardware {
            for name in HARDWARE {
                match hardware::Encoder::open(name, width, height, bps, fps) {
                    Ok(e) => return Ok(Self { inner: Inner::Hardware(Box::new(e)), width, height }),
                    Err(()) => tracing::info!("hardware encoder {name} did not start"),
                }
            }
        }
        let _ = hardware;
        Ok(Self { inner: Inner::Software(Box::new(software(bps, fps)?)), width, height })
    }

    /// The encoder in use, for the log: a `hwcodec` encoder name or "openh264".
    pub fn name(&self) -> &'static str {
        match &self.inner {
            Inner::Software(_) => "openh264",
            #[cfg(all(feature = "hwcodec", any(target_os = "macos", windows)))]
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
            #[cfg(all(feature = "hwcodec", any(target_os = "macos", windows)))]
            Inner::Hardware(e) => match e.encode(&yuv) {
                Ok(data) => Ok(data),
                Err(code) => {
                    tracing::warn!("hardware encoder {} failed ({code}), switching to openh264", e.name);
                    self.inner = Inner::Software(Box::new(software(e.bps, e.fps)?));
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

#[cfg(all(feature = "hwcodec", any(target_os = "macos", windows)))]
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
        /// What it was opened with, for the openh264 that replaces it if it fails.
        pub bps: u32,
        pub fps: f32,
        hw: Hw,
        /// The frame in the encoder's NV12 layout (Y plane, then interleaved U and V), reused.
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
                // NV12, not I420: Windows' encoders take frames on D3D11 surfaces (NVENC) or only NV12 (QSV).
                pixfmt: AVPixelFormat::AV_PIX_FMT_NV12,
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
            Ok(Self { name, bps, fps, hw, buf, started: Instant::now(), produced: false })
        }

        /// `Err` = the encoder is broken (its error code). Empty = no output for this frame yet: VideoToolbox
        /// encodes asynchronously, which `hwcodec` reports as -1, so only a first second with no output at all counts.
        pub fn encode(&mut self, yuv: &YUVBuffer) -> Result<Vec<u8>, i32> {
            let (w, h) = yuv.dimensions();
            let (ys, us, vs) = yuv.strides();
            let (y_line, uv_line, uv_at) = (self.hw.linesize[0] as usize, self.hw.linesize[1] as usize, self.hw.offset[0] as usize);
            for r in 0..h {
                self.buf[r * y_line..r * y_line + w].copy_from_slice(&yuv.y()[r * ys..r * ys + w]);
            }
            for r in 0..h / 2 {
                let row = &mut self.buf[uv_at + r * uv_line..uv_at + r * uv_line + w];
                for (c, uv) in row.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                    *uv = [yuv.u()[r * us + c], yuv.v()[r * vs + c]];
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use openh264::decoder::Decoder;

    use super::*;

    /// Whichever encoder starts here (a hardware one with `--features hwcodec` on a machine that has it, openh264
    /// otherwise), what it sends decodes back to the colours it was given: the frame reached it in the right layout.
    #[test]
    fn encoded_frames_decode_back_to_their_colours() {
        let colours = [[220, 40, 40], [40, 200, 60], [50, 60, 210], [230, 220, 60]];
        let (w, h) = (320, 240);
        let frame = RgbaImage::from_fn(w, h, |x, y| {
            let [r, g, b] = colours[(x >= w / 2) as usize + 2 * (y >= h / 2) as usize];
            image::Rgba([r, g, b, 255])
        });
        let mut encoder = H264::open(w, h, 2_000_000, 10.0, true).unwrap();
        let name = encoder.name();
        let mut decoder = Decoder::new().unwrap();
        let mut rgb = vec![0; (w * h * 3) as usize];
        let mut decoded = false;
        for _ in 0..20 {
            let data = encoder.encode(&frame).unwrap();
            if !data.is_empty() {
                if let Some(yuv) = decoder.decode(&data).unwrap() {
                    yuv.write_rgb8(&mut rgb);
                    decoded = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(decoded, "{name} sent no picture");
        assert_eq!(encoder.name(), name, "{name} failed mid-stream");
        for (i, c) in colours.iter().enumerate() {
            let (x, y) = (w / 4 + (i as u32 % 2) * w / 2, h / 4 + (i as u32 / 2) * h / 2);
            let at = ((y * w + x) * 3) as usize;
            let got = &rgb[at..at + 3];
            assert!(got.iter().zip(c).all(|(g, c)| g.abs_diff(*c) < 25), "{name}: quadrant {i} came back {got:?}, sent {c:?}");
        }
    }
}

