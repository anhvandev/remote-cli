//! Coordinate Mapper: the only place that converts between Observation image space and real pixels.

use crate::platform::DisplayInfo;

/// Default long edge of an Observation image; smaller displays are not upscaled.
pub const LONG_EDGE: u32 = 1280;

/// Size of the Observation image of a `width` x `height` display.
pub fn observed_size(width: u32, height: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= LONG_EDGE {
        return (width, height);
    }
    let scale = |v: u32| ((v as u64 * LONG_EDGE as u64 / long as u64) as u32).max(1);
    (scale(width), scale(height))
}

/// Real pixel on `display` for image point (x, y); `None` if outside the image.
pub fn to_real(display: &DisplayInfo, x: u32, y: u32) -> Option<(u32, u32)> {
    let (iw, ih) = observed_size(display.width, display.height);
    if x >= iw || y >= ih {
        return None;
    }
    let map = |v: u32, real: u32, image: u32| (v as u64 * real as u64 / image as u64) as u32;
    Some((map(x, display.width, iw), map(y, display.height, ih)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_edge_is_scaled_to_1280_keeping_aspect() {
        assert_eq!(observed_size(2560, 1440), (1280, 720));
        assert_eq!(observed_size(1080, 1920), (720, 1280));
        assert_eq!(observed_size(1024, 768), (1024, 768));
    }
}
