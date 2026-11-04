//! Observer: turns the screen into Observations (settle, downscale, compress, "unchanged").

use std::{
    collections::HashMap,
    io::Cursor,
    time::{Duration, Instant},
};

use image::{codecs::jpeg::JpegEncoder, imageops, RgbaImage};
use rcli_protocol::agent::Image;

use crate::{
    coords,
    platform::{Frame, Platform},
};

/// `shot` waits at most this long for the screen to stop changing.
const SETTLE_MAX: Duration = Duration::from_secs(1);
/// Two captures this far apart that are equal mean the screen has settled.
const SETTLE_POLL: Duration = Duration::from_millis(100);
const JPEG_QUALITY: u8 = 70;

#[derive(Default)]
pub struct Observer {
    /// Last image sent per (Controller, display), to answer "unchanged".
    // shortcut: grows with every Controller that ever observed; fine for a handful of users, evict when that changes.
    last: HashMap<(String, u32), Vec<u8>>,
}

impl Observer {
    /// One settled, encoded image per display, as seen by `controller`.
    pub fn shot(&mut self, p: &dyn Platform, displays: &[u32], controller: &str) -> anyhow::Result<Vec<Image>> {
        displays.iter().map(|&d| self.image(p, d, controller)).collect()
    }

    fn image(&mut self, p: &dyn Platform, display: u32, controller: &str) -> anyhow::Result<Image> {
        let frame = settle(p, display)?;
        let (width, height) = coords::observed_size(frame.width, frame.height);
        let data = encode(&to_image(frame)?, width, height)?;
        let key = (controller.to_owned(), display);
        let unchanged = self.last.get(&key) == Some(&data);
        let mime = "image/jpeg".to_owned();
        if unchanged {
            return Ok(Image { display, width, height, unchanged, mime, data: vec![], at_ms: 0 });
        }
        self.last.insert(key, data.clone());
        Ok(Image { display, width, height, unchanged, mime, data, at_ms: 0 })
    }

    /// Finishes a `frames` recording: keeps recording until the screen settles (at most `SETTLE_MAX`),
    /// then returns the frames of `displays`.
    pub fn frames(&mut self, p: &dyn Platform, mut rec: Frames, displays: &[u32], controller: &str) -> anyhow::Result<Vec<Image>> {
        let deadline = Instant::now() + SETTLE_MAX;
        loop {
            std::thread::sleep(SETTLE_POLL);
            if !rec.sample(p)? || Instant::now() >= deadline {
                break;
            }
        }
        let mut images = vec![];
        for track in rec.tracks.into_iter().filter(|t| displays.contains(&t.display)) {
            for (at_ms, img) in track.frames {
                let (width, height) = img.dimensions();
                let data = encode(&img, width, height)?;
                images.push(Image { display: track.display, width, height, unchanged: false, mime: "image/jpeg".into(), data, at_ms });
            }
            if let Some(last) = images.last().filter(|i| i.display == track.display) {
                // A later `shot` of this same screen can then answer "unchanged".
                self.last.insert((controller.to_owned(), track.display), last.data.clone());
            }
        }
        Ok(images)
    }
}

/// Frames per display when the request does not say.
pub const MAX_FRAMES: u32 = 10;
/// Screens are compared at this long edge, so tiny pixel noise does not count as a change.
const COMPARE_EDGE: u32 = 256;
/// A compared pixel must differ by more than this in some channel to count as a change.
const NOTICEABLE: u8 = 24;

/// A `frames` recording: change-triggered, timestamped frames per display.
pub struct Frames {
    start: Instant,
    max: usize,
    tracks: Vec<Track>,
}

struct Track {
    display: u32,
    /// Small copy of the last kept frame, to compare new captures with.
    seen: Option<RgbaImage>,
    /// Kept frames at observed size; JPEG-encoded only at the end so replaced ones cost nothing.
    frames: Vec<(u32, RgbaImage)>,
}

impl Frames {
    pub fn new(displays: Vec<u32>, max: u32) -> Self {
        let max = if max == 0 { MAX_FRAMES } else { max } as usize;
        let tracks = displays.into_iter().map(|display| Track { display, seen: None, frames: vec![] }).collect();
        Frames { start: Instant::now(), max, tracks }
    }

    /// Samples now, then every `SETTLE_POLL` until `d` has passed.
    pub fn record(&mut self, p: &dyn Platform, d: Duration) -> anyhow::Result<()> {
        let until = Instant::now() + d;
        loop {
            self.sample(p)?;
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            std::thread::sleep(left.min(SETTLE_POLL));
        }
    }

    /// Captures every display once and keeps the ones that changed noticeably. When a display is at
    /// the cap, its newest frame is replaced so the final screen is always included.
    fn sample(&mut self, p: &dyn Platform) -> anyhow::Result<bool> {
        let mut changed = false;
        for t in &mut self.tracks {
            let full = to_image(p.capture(t.display)?)?;
            let at_ms = self.start.elapsed().as_millis() as u32;
            let (width, height) = coords::observed_size(full.width(), full.height());
            let observed = if (width, height) == full.dimensions() { full } else { imageops::thumbnail(&full, width, height) };
            let small = shrink(&observed, COMPARE_EDGE);
            if t.seen.as_ref().is_some_and(|seen| !noticeable(seen, &small)) {
                continue;
            }
            changed = true;
            t.seen = Some(small);
            if t.frames.len() >= self.max {
                t.frames.pop();
            }
            t.frames.push((at_ms, observed));
        }
        Ok(changed)
    }
}

fn shrink(img: &RgbaImage, edge: u32) -> RgbaImage {
    let long = img.width().max(img.height());
    if long <= edge {
        return img.clone();
    }
    let size = |n: u32| (n as u64 * edge as u64 / long as u64).max(1) as u32;
    imageops::thumbnail(img, size(img.width()), size(img.height()))
}

fn noticeable(a: &RgbaImage, b: &RgbaImage) -> bool {
    a.dimensions() != b.dimensions() || a.as_raw().iter().zip(b.as_raw()).any(|(x, y)| x.abs_diff(*y) > NOTICEABLE)
}

/// Captures until two frames in a row are equal, or `SETTLE_MAX` has passed.
fn settle(p: &dyn Platform, display: u32) -> anyhow::Result<Frame> {
    let deadline = Instant::now() + SETTLE_MAX;
    let mut prev = p.capture(display)?;
    loop {
        std::thread::sleep(SETTLE_POLL);
        let cur = p.capture(display)?;
        if cur == prev || Instant::now() >= deadline {
            return Ok(cur);
        }
        prev = cur;
    }
}

fn to_image(frame: Frame) -> anyhow::Result<RgbaImage> {
    RgbaImage::from_raw(frame.width, frame.height, frame.rgba).ok_or_else(|| anyhow::anyhow!("frame size does not match its pixels"))
}

fn encode(full: &RgbaImage, width: u32, height: u32) -> anyhow::Result<Vec<u8>> {
    let small = if (width, height) == full.dimensions() { full.clone() } else { imageops::thumbnail(full, width, height) };
    let rgb = image::DynamicImage::ImageRgba8(small).to_rgb8();
    let mut out = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).encode_image(&rgb)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(edit: impl Fn(&mut RgbaImage)) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(2560, 1440, image::Rgba([255, 255, 255, 255]));
        edit(&mut img);
        shrink(&img, COMPARE_EDGE)
    }

    #[test]
    fn a_typed_character_is_noticeable_but_pixel_noise_is_not() {
        let blank = screen(|_| {});
        let noise = screen(|img| img.put_pixel(100, 100, image::Rgba([200, 200, 200, 255])));
        let char_ = screen(|img| {
            for x in 100..112 {
                for y in 100..124 {
                    img.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                }
            }
        });
        assert!(!noticeable(&blank, &noise));
        assert!(noticeable(&blank, &char_));
    }
}
