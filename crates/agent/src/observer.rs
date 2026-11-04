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
    /// Last image sent per (Controller, display), to answer "unchanged".
    // shortcut: grows with every Controller that ever observed; fine for a handful of users, evict when that changes.
    last: HashMap<(String, u32), Vec<u8>>,
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
        let key = (controller.to_owned(), display);
        let unchanged = self.last.get(&key) == Some(&data);
        let mime = "image/jpeg".to_owned();
        if unchanged {
            return Ok(Image { display, width, height, unchanged, mime, data: vec![] });
        }
        self.last.insert(key, data.clone());
        Ok(Image { display, width, height, unchanged, mime, data })
    }
}

/// Captures until two frames in a row are equal, or `SETTLE_MAX` has passed.
pub fn settle(p: &dyn Platform, display: u32) -> anyhow::Result<Frame> {
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
