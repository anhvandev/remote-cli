//! Observer: turns the screen into Observations (settle, downscale, compress, "unchanged").

use std::{
    collections::HashMap,
    io::Cursor,
    time::{Duration, Instant},
};

use image::{codecs::jpeg::JpegEncoder, imageops, RgbaImage};
use rcli_protocol::agent::{Image, Mark};

use crate::{
    coords, marks,
    platform::{Frame, Platform},
};

/// `shot` waits at most this long for the screen to stop changing.
const SETTLE_MAX: Duration = Duration::from_secs(1);
/// Two captures this far apart that are equal mean the screen has settled.
const SETTLE_POLL: Duration = Duration::from_millis(100);
const JPEG_QUALITY: u8 = 70;

#[derive(Default)]
pub struct Observer {
    /// Last image sent per Controller and display, to answer "unchanged".
    last: PerController<HashMap<u32, Vec<u8>>>,
}

/// Controllers whose per-Controller state (last images, Mark table) is kept.
const MAX_CONTROLLERS: usize = 64;

/// Per-Controller state for the `MAX_CONTROLLERS` most recently updated Controllers; older ones are forgotten
/// (their next shot is sent in full, old Mark references go stale).
// shortcut: evicts by a linear scan of at most 64 entries; switch to an LRU structure if the cap grows a lot.
pub struct PerController<V> {
    map: HashMap<String, (u64, V)>,
    tick: u64,
}

impl<V> Default for PerController<V> {
    fn default() -> Self {
        Self { map: HashMap::new(), tick: 0 }
    }
}

impl<V: Default> PerController<V> {
    pub fn get(&self, controller: &str) -> Option<&V> {
        self.map.get(controller).map(|(_, v)| v)
    }

    /// The Controller's state for updating; marks it most recent.
    pub fn entry(&mut self, controller: &str) -> &mut V {
        if !self.map.contains_key(controller) && self.map.len() >= MAX_CONTROLLERS {
            let oldest = self.map.iter().min_by_key(|(_, (t, _))| *t).map(|(k, _)| k.clone());
            self.map.remove(&oldest.unwrap_or_default());
        }
        self.tick += 1;
        let e = self.map.entry(controller.to_owned()).or_default();
        e.0 = self.tick;
        &mut e.1
    }
}

impl Observer {
    /// One settled, encoded image per display, as seen by `controller`. With `marks`, the images are annotated
    /// and the Marks of all displays are returned, numbered from 1.
    pub fn shot(&mut self, p: &dyn Platform, displays: &[u32], controller: &str, marks: bool) -> anyhow::Result<(Vec<Image>, Vec<Mark>)> {
        let mut found = vec![];
        let images = displays.iter().map(|&d| self.image(p, d, controller, marks.then_some(&mut found))).collect::<anyhow::Result<_>>()?;
        Ok((images, found))
    }

    fn image(&mut self, p: &dyn Platform, display: u32, controller: &str, marks: Option<&mut Vec<Mark>>) -> anyhow::Result<Image> {
        let frame = settle(p, display)?;
        let (width, height) = coords::observed_size(frame.width, frame.height);
        let new_marks = match marks {
            Some(all) => {
                let m = marks::detect(p, display, &frame, all.len() as u32 + 1)?;
                all.extend(m.iter().cloned());
                m
            }
            None => vec![],
        };
        let mut small = scale(frame, width, height)?;
        marks::annotate(&mut small, &new_marks);
        let data = jpeg(small)?;
        let unchanged = self.last.get(controller).and_then(|l| l.get(&display)) == Some(&data);
        let mime = "image/jpeg".to_owned();
        if unchanged {
            return Ok(Image { display, width, height, unchanged, mime, data: vec![], at_ms: 0, p2p: 0 });
        }
        self.last.entry(controller).insert(display, data.clone());
        Ok(Image { display, width, height, unchanged, mime, data, at_ms: 0, p2p: 0 })
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
                let data = jpeg(img)?;
                images.push(Image { display: track.display, width, height, unchanged: false, mime: "image/jpeg".into(), data, at_ms, p2p: 0 });
            }
            if let Some(last) = images.last().filter(|i| i.display == track.display) {
                // A later `shot` of this same screen can then answer "unchanged".
                self.last.entry(controller).insert(track.display, last.data.clone());
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
            let frame = p.capture(t.display)?;
            let at_ms = self.start.elapsed().as_millis() as u32;
            let (width, height) = coords::observed_size(frame.width, frame.height);
            let observed = scale(frame, width, height)?;
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

/// Captures until two frames in a row show no noticeable change (same rule as `frames`), or `SETTLE_MAX` has passed.
pub fn settle(p: &dyn Platform, display: u32) -> anyhow::Result<Frame> {
    let deadline = Instant::now() + SETTLE_MAX;
    let (_, mut prev) = compared(p.capture(display)?)?;
    loop {
        std::thread::sleep(SETTLE_POLL);
        let (cur, small) = compared(p.capture(display)?)?;
        if !noticeable(&prev, &small) || Instant::now() >= deadline {
            return Ok(cur);
        }
        prev = small;
    }
}

/// The frame and its small copy for change detection.
fn compared(frame: Frame) -> anyhow::Result<(Frame, RgbaImage)> {
    let (width, height) = (frame.width, frame.height);
    let full = RgbaImage::from_raw(width, height, frame.rgba).ok_or_else(|| anyhow::anyhow!("frame size does not match its pixels"))?;
    let small = shrink(&full, COMPARE_EDGE);
    Ok((Frame { width, height, rgba: full.into_raw() }, small))
}

fn scale(frame: Frame, width: u32, height: u32) -> anyhow::Result<RgbaImage> {
    let full = RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
        .ok_or_else(|| anyhow::anyhow!("frame size does not match its pixels"))?;
    Ok(if (width, height) == (frame.width, frame.height) { full } else { imageops::thumbnail(&full, width, height) })
}

fn jpeg(small: RgbaImage) -> anyhow::Result<Vec<u8>> {
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
    fn only_the_most_recent_controllers_are_remembered() {
        let mut t = PerController::<u32>::default();
        for i in 0..=MAX_CONTROLLERS {
            *t.entry(&format!("c{i}")) = 1;
            *t.entry("c0") = 2; // c0 stays recent
        }
        assert_eq!(t.get("c0"), Some(&2));
        assert_eq!(t.get("c1"), None, "the least recently updated is forgotten");
        assert_eq!(t.get(&format!("c{MAX_CONTROLLERS}")), Some(&1));
        assert_eq!(t.map.len(), MAX_CONTROLLERS);
    }

    #[test]
    fn a_screen_with_only_slight_flicker_counts_as_settled() {
        let p = crate::platform::FakePlatform::new("pc");
        p.update(|s| s.flicker = true);
        let started = Instant::now();
        settle(&p, 0).unwrap();
        assert!(started.elapsed() < SETTLE_MAX / 2, "waited {:?}", started.elapsed());
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
