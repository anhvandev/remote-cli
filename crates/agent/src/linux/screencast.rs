//! Wayland screen capture: the XDG desktop portal's `org.freedesktop.portal.ScreenCast` hands out a PipeWire
//! stream of the monitors a person at the Device approved; a dedicated thread consumes it and keeps the newest
//! frame of every stream. The portal lives in the user's session bus, so in the installed setup this runs in the
//! console-session helper (`linux/session.rs`), never in the root service.
//!
//! The first approval is remembered: the portal returns a *restore token*, kept in the Device state dir (0600),
//! and `SelectSources` sends it back on every later session, so nobody is asked twice.

use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Condvar, Mutex},
    time::Duration,
};

use crate::platform::{DisplayInfo, Frame};

/// The state file the portal's restore token lives in, beside `credentials.json`.
pub const TOKEN_FILE: &str = "screencast.restore_token";
/// The environment variable the root service uses to hand the stored token to the session helper (the helper
/// runs as the session user, who cannot read the root-only state dir).
pub const TOKEN_ENV: &str = "RCLI_SCREENCAST_RESTORE_TOKEN";

/// How long the portal may take before we give up: the person at the Device has to approve a dialog once.
const PORTAL_TIMEOUT: Duration = Duration::from_secs(60);
/// A stream that negotiated but has not delivered a frame yet.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(10);
/// A later capture: the compositor sends frames only when the screen changes, so one is normally waiting.
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// The portal restore token in the Device state dir: one approval at the Device, reused across Agent restarts.
#[derive(Clone)]
pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The stored token; blank lines and missing files read as no token (the portal asks again).
    pub fn load(&self) -> Option<String> {
        let token = std::fs::read_to_string(&self.path).ok()?;
        let token = token.trim();
        (!token.is_empty()).then(|| token.to_owned())
    }

    /// Writes the token root-only, through a temporary file so a crash cannot leave half a token. A blank token
    /// is ignored: the portal did not offer persistence.
    pub fn save(&self, token: &str) -> anyhow::Result<()> {
        let token = token.trim();
        if token.is_empty() {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = self.path.with_extension("tmp");
        std::fs::write(&temp, format!("{token}\n"))?;
        #[cfg(unix)]
        std::fs::set_permissions(&temp, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        std::fs::rename(&temp, &self.path)?;
        Ok(())
    }
}

/// The pixel layouts the portal's stream can hand over, converted to the RGBA `Frame` the Agent wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba,
    Rgbx,
    Bgrx,
    Bgra,
    Rgb,
    Bgr,
}

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba | Self::Rgbx | Self::Bgrx | Self::Bgra => 4,
            Self::Rgb | Self::Bgr => 3,
        }
    }

    /// The SPA video format number from a `param_changed`; `None` for formats we do not convert (YUV and
    /// packed-16 layouts), which then fail with a clear error instead of showing garbage.
    pub fn from_spa(raw: u32) -> Option<Self> {
        use pipewire::spa::param::video::VideoFormat;
        match VideoFormat::from_raw(raw) {
            VideoFormat::RGBA => Some(Self::Rgba),
            VideoFormat::RGBx => Some(Self::Rgbx),
            VideoFormat::BGRx => Some(Self::Bgrx),
            VideoFormat::BGRA => Some(Self::Bgra),
            VideoFormat::RGB => Some(Self::Rgb),
            VideoFormat::BGR => Some(Self::Bgr),
            _ => None,
        }
    }
}

/// The restore token the last portal session returned, for the session helper's wire (the helper cannot write the
/// root-only state dir, so the daemon persists what the helper publishes here).
static PUBLISHED_TOKEN: Mutex<Option<String>> = Mutex::new(None);

pub fn publish_token(token: &str) {
    *PUBLISHED_TOKEN.lock().unwrap() = Some(token.to_owned());
}

pub fn published_token() -> Option<String> {
    PUBLISHED_TOKEN.lock().unwrap().clone()
}

/// One stream buffer as RGBA pixels. `stride` is the row length in bytes (0 = tightly packed); rows may carry
/// padding the compositor added for its GPU.
pub fn rgba(data: &[u8], width: u32, height: u32, stride: i32, format: PixelFormat) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(width > 0 && height > 0, "empty screen cast frame ({width}x{height})");
    anyhow::ensure!(stride >= 0, "negative stride {stride}");
    let bpp = format.bytes_per_pixel();
    let row = if stride == 0 { width as usize * bpp } else { stride as usize };
    anyhow::ensure!(row >= width as usize * bpp, "stride {row} is shorter than a {width} px row of {bpp} byte pixels");
    let needed = row * (height as usize - 1) + width as usize * bpp;
    anyhow::ensure!(data.len() >= needed, "the frame buffer holds {} bytes, {} are needed", data.len(), needed);
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    let swap = matches!(format, PixelFormat::Bgra | PixelFormat::Bgrx | PixelFormat::Bgr);
    // Row by row: the last row often carries no trailing padding, so `chunks_exact` would drop it.
    for y in 0..height as usize {
        let start = y * row;
        let end = start + width as usize * bpp;
        anyhow::ensure!(end <= data.len(), "the frame buffer holds {} bytes, {end} are needed", data.len());
        for pixel in data[start..end].chunks_exact(bpp) {
            if swap {
                out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], if bpp == 4 { pixel[3] } else { 255 }]);
            } else {
                out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], if bpp == 4 { pixel[3] } else { 255 }]);
            }
        }
    }
    Ok(out)
}

/// What the Agent asks the portal for: monitor sources, as many as the person selects, in a persistent session
/// that reuses the stored restore token so the approval is asked once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Select {
    pub monitors: bool,
    pub multiple: bool,
    pub persistent: bool,
    pub cursor_embedded: bool,
    pub restore_token: Option<String>,
}

impl Select {
    pub fn new(restore_token: Option<String>) -> Self {
        Self { monitors: true, multiple: true, persistent: true, cursor_embedded: true, restore_token }
    }

    /// The `SelectSources` options this policy means.
    pub fn options(&self) -> ashpd::desktop::screencast::SelectSourcesOptions {
        use ashpd::desktop::{
            screencast::{CursorMode, SelectSourcesOptions, SourceType},
            PersistMode,
        };
        let mut options = SelectSourcesOptions::default().set_multiple(self.multiple);
        if self.monitors {
            options = options.set_sources(SourceType::MONITOR);
        }
        if self.cursor_embedded {
            options = options.set_cursor_mode(CursorMode::EMBEDDED);
        }
        if self.persistent {
            // ashpd names the portal's XDP_PERSIST_MODE_PERSISTENT `ExplicitlyRevoked`: it lasts until revoked.
            options = options.set_persist_mode(PersistMode::ExplicitlyRevoked);
        }
        options.set_restore_token(self.restore_token.as_deref())
    }
}

/// One monitor as the portal described it, with the slot its frames arrive in.
struct Stream {
    position: (i32, i32),
    /// The compositor's size for the monitor; the stream's real pixels can differ under fractional scaling.
    logical: (u32, u32),
    slot: Arc<Slot>,
}

/// The newest frame of one stream, or why there will not be one.
#[derive(Default)]
struct Slot {
    frame: Mutex<Option<Frame>>,
    ready: Condvar,
    error: Mutex<Option<String>>,
}

impl Slot {
    fn put(&self, frame: Frame) {
        *self.frame.lock().unwrap() = Some(frame);
        self.ready.notify_all();
    }

    fn fail(&self, message: impl Into<String>) {
        *self.error.lock().unwrap() = Some(message.into());
        self.ready.notify_all();
    }

    fn wait(&self, timeout: Duration) -> anyhow::Result<Frame> {
        let deadline = std::time::Instant::now() + timeout;
        let mut frame = self.frame.lock().unwrap();
        loop {
            if let Some(frame) = frame.as_ref() {
                return Ok(frame.clone());
            }
            if let Some(error) = self.error.lock().unwrap().as_ref() {
                anyhow::bail!("{error}");
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            anyhow::ensure!(!left.is_zero(), "the screen cast stream sent no frame within {} s", timeout.as_secs());
            frame = self.ready.wait_timeout(frame, left).unwrap().0;
        }
    }
}

/// An open ScreenCast session: one display per approved monitor, fed by a PipeWire thread.
pub struct Screen {
    streams: Vec<Stream>,
    /// The portal's new restore token (the same one when the approval was reused).
    token: Option<String>,
    displays: Mutex<Option<Vec<DisplayInfo>>>,
}

impl Screen {
    /// Creates a portal session and starts consuming its streams. Blocks until the portal answered (the person
    /// at the Device approves one dialog) and the first frames arrive.
    pub fn open(restore_token: Option<String>, token_sink: impl Fn(&str) + Send + 'static) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("rcli-screencast".into())
            .spawn(move || open_then_consume(restore_token, &tx, token_sink))
            .map_err(|e| anyhow::anyhow!("cannot start the screen cast thread: {e}"))?;
        match rx.recv_timeout(PORTAL_TIMEOUT) {
            Ok(Ok(screen)) => Ok(screen),
            Ok(Err(e)) => Err(anyhow::anyhow!("{e}")),
            Err(_) => anyhow::bail!(
                "the desktop portal did not answer the ScreenCast request: approve the screen sharing dialog at the Device"
            ),
        }
    }

    /// The token the portal returned, for the Device state dir.
    pub fn restore_token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// One entry per approved monitor, in real pixels (the stream's size, not the compositor's logical size).
    pub fn displays(&self) -> anyhow::Result<Vec<DisplayInfo>> {
        if let Some(displays) = self.displays.lock().unwrap().clone() {
            return Ok(displays);
        }
        let mut out = Vec::with_capacity(self.streams.len());
        for (i, stream) in self.streams.iter().enumerate() {
            let frame = stream
                .slot
                .wait(FIRST_FRAME_TIMEOUT)
                .map_err(|e| anyhow::anyhow!("display {i}: {e:#}"))?;
            // Fractional scaling: the compositor reports logical pixels, the stream real ones.
            let scale = match stream.logical.0 {
                0 => 1.0,
                logical => f64::from(frame.width) / f64::from(logical),
            };
            out.push(DisplayInfo {
                index: i as u32,
                x: (f64::from(stream.position.0) * scale).round() as i32,
                y: (f64::from(stream.position.1) * scale).round() as i32,
                width: frame.width,
                height: frame.height,
                scale,
                primary: i == 0,
            });
        }
        *self.displays.lock().unwrap() = Some(out.clone());
        Ok(out)
    }

    /// The newest frame of `display` (the index `displays()` named).
    pub fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        let stream = self
            .streams
            .get(display as usize)
            .ok_or_else(|| anyhow::anyhow!("no display {display}"))?;
        stream.slot.wait(FRAME_TIMEOUT)
    }
}

struct StreamInfo {
    node_id: u32,
    position: (i32, i32),
    logical: (u32, u32),
}

struct Opened {
    fd: std::os::fd::OwnedFd,
    streams: Vec<StreamInfo>,
    token: Option<String>,
    /// Dropping the session closes the portal session, which stops the streams.
    _session: ashpd::desktop::Session<ashpd::desktop::screencast::Screencast>,
    /// The portal's D-Bus connection is async: its worker keeps polling for as long as the streams run.
    _runtime: tokio::runtime::Runtime,
}

/// The portal session before its runtime is attached.
struct OpenedSession {
    fd: std::os::fd::OwnedFd,
    streams: Vec<StreamInfo>,
    token: Option<String>,
    session: ashpd::desktop::Session<ashpd::desktop::screencast::Screencast>,
}

async fn open_portal(restore_token: Option<&str>) -> anyhow::Result<OpenedSession> {
    use ashpd::desktop::screencast::{Screencast, StartCastOptions};
    let proxy = Screencast::new().await.map_err(|e| {
        anyhow::anyhow!(
            "screen capture on Wayland needs the XDG desktop portal's ScreenCast (GNOME, KDE, or xdg-desktop-portal-wlr; \
             some wlroots setups lack it): {e}"
        )
    })?;
    let session = proxy.create_session(Default::default()).await?;
    proxy.select_sources(&session, Select::new(restore_token.map(str::to_owned)).options()).await?;
    let response = proxy
        .start(&session, None, StartCastOptions::default())
        .await?
        .response()
        .map_err(|e| anyhow::anyhow!("the ScreenCast request was refused or cancelled at the Device: {e}"))?;
    let streams: Vec<StreamInfo> = response
        .streams()
        .iter()
        .map(|stream| StreamInfo {
            node_id: stream.pipe_wire_node_id(),
            position: stream.position().unwrap_or((0, 0)),
            // The compositor's coordinates can differ from pixels; 0 means "unknown", scaled as 1.
            logical: stream.size().map(|(w, h)| (w.max(0) as u32, h.max(0) as u32)).unwrap_or((0, 0)),
        })
        .collect();
    anyhow::ensure!(!streams.is_empty(), "the portal approved no monitor to share");
    let fd = proxy.open_pipe_wire_remote(&session, Default::default()).await?;
    Ok(OpenedSession { fd, streams, token: response.restore_token().map(str::to_owned), session })
}

/// The PipeWire side of one stream: the negotiated layout, and the slot its frames go to.
struct Sink {
    format: Option<PixelFormat>,
    size: (u32, u32),
    slot: Arc<Slot>,
}

impl Sink {
    fn fail(&self, message: impl Into<String>) {
        self.slot.fail(message);
    }
}

fn open_then_consume(restore_token: Option<String>, tx: &mpsc::Sender<Result<Screen, String>>, token_sink: impl Fn(&str)) {
    let opened = match open_with_runtime(restore_token) {
        Ok(opened) => opened,
        Err(e) => {
            let _ = tx.send(Err(format!("{e:#}")));
            return;
        }
    };
    if let Some(token) = opened.token.as_deref() {
        token_sink(token);
    }
    let slots: Vec<Arc<Slot>> = opened.streams.iter().map(|_| Arc::new(Slot::default())).collect();
    let screen = Screen {
        streams: opened
            .streams
            .iter()
            .zip(slots.iter())
            .map(|(info, slot)| Stream { position: info.position, logical: info.logical, slot: slot.clone() })
            .collect(),
        token: opened.token.clone(),
        displays: Mutex::new(None),
    };
    if tx.send(Ok(screen)).is_err() {
        return;
    }
    if let Err(e) = consume(opened, &slots) {
        let message = format!("the screen cast stream stopped: {e:#}");
        for slot in &slots {
            slot.fail(message.clone());
        }
        tracing::warn!("{message}");
    }
}

/// The portal calls are async; their D-Bus connection has to stay polled while PipeWire runs on this thread,
/// so the runtime has its own worker thread.
fn open_with_runtime(restore_token: Option<String>) -> anyhow::Result<Opened> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|e| anyhow::anyhow!("cannot start the portal runtime: {e}"))?;
    let opened = runtime.block_on(open_portal(restore_token.as_deref()))?;
    // Keep the runtime (and with it the session bus connection) alive for as long as the streams run.
    Ok(Opened {
        fd: opened.fd,
        streams: opened.streams,
        token: opened.token,
        _session: opened.session,
        _runtime: runtime,
    })
}

fn consume(opened: Opened, slots: &[Arc<Slot>]) -> anyhow::Result<()> {
    use pipewire as pw;
    let mainloop = pw::main_loop::MainLoopBox::new(None)?;
    let context = pw::context::ContextBox::new(mainloop.loop_(), None)?;
    let core = context.connect_fd(opened.fd, None)?;
    let mut streams = Vec::with_capacity(opened.streams.len());
    // One serialized format pod serves every stream; `connect` needs its own mutable slice per stream.
    let params = format_params();
    for (info, slot) in opened.streams.into_iter().zip(slots.iter().cloned()) {
        let stream = pw::stream::StreamBox::new(
            &core,
            "rcli-screencast",
            pw::properties::properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            },
        )?;
        let listener = stream
            .add_local_listener_with_user_data(Sink { format: None, size: (0, 0), slot })
            .param_changed(|_, sink, id, param| {
                let Some(param) = param else { return };
                if id != pw::spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let Ok((media_type, media_subtype)) = pw::spa::param::format_utils::parse_format(param) else {
                    return;
                };
                if media_type != pw::spa::param::format::MediaType::Video
                    || media_subtype != pw::spa::param::format::MediaSubtype::Raw
                {
                    return;
                }
                let mut video = pw::spa::param::video::VideoInfoRaw::new();
                if video.parse(param).is_err() {
                    return;
                }
                let size = video.size();
                sink.size = (size.width, size.height);
                sink.format = PixelFormat::from_spa(video.format().as_raw());
                if sink.format.is_none() {
                    sink.fail(format!(
                        "the compositor's screen cast format {:?} is not convertible to RGBA ({}x{})",
                        video.format(),
                        size.width,
                        size.height
                    ));
                }
            })
            .process(|stream, sink| {
                let Some(mut buffer) = stream.dequeue_buffer() else { return };
                let Some(data) = buffer.datas_mut().first_mut() else { return };
                let (offset, size, stride) = {
                    let chunk = data.chunk();
                    (chunk.offset() as usize, chunk.size() as usize, chunk.stride())
                };
                let (Some(format), (width, height)) = (sink.format, sink.size) else { return };
                let Some(bytes) = data.data() else {
                    sink.fail("the PipeWire buffer is not mapped");
                    return;
                };
                let end = offset.saturating_add(size).min(bytes.len());
                match rgba(&bytes[offset.min(end)..end], width, height, stride, format) {
                    Ok(rgba) => sink.slot.put(Frame { width, height, rgba }),
                    Err(e) => sink.fail(format!("{e:#}")),
                }
            })
            .register()?;
        stream.connect(
            pw::spa::utils::Direction::Input,
            Some(info.node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut { params },
        )?;
        streams.push((stream, listener));
    }
    mainloop.run();
    Ok(())
}

/// The formats we accept: packed RGB(A) only, so no colour conversion is needed. YUV is declined.
fn format_params() -> [&'static pipewire::spa::pod::Pod; 1] {
    use pipewire::spa::{
        param::{format::FormatProperties, video::VideoFormat, ParamType},
        pod::{object, property, serialize::PodSerializer, Value},
        utils::{Fraction, Rectangle, SpaTypes},
    };
    let format = VideoFormat::BGRx;
    let pod = Value::Object(object!(
        SpaTypes::ObjectParamFormat,
        ParamType::EnumFormat,
        property!(FormatProperties::MediaType, Id, pipewire::spa::param::format::MediaType::Video),
        property!(FormatProperties::MediaSubtype, Id, pipewire::spa::param::format::MediaSubtype::Raw),
        property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            format,
            format,
            VideoFormat::RGBA,
            VideoFormat::RGBx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGB,
            VideoFormat::BGR
        ),
        property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            Rectangle { width: 8, height: 8 },
            Rectangle { width: 1, height: 1 },
            Rectangle { width: 16384, height: 16384 }
        ),
        property!(
            FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            Fraction { num: 30, denom: 1 },
            Fraction { num: 0, denom: 1 },
            Fraction { num: 144, denom: 1 }
        ),
    ));
    let bytes = PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &pod).expect("serialize the format pod").0.into_inner();
    // The pod has to outlive the call to `connect`; one leak of a few hundred bytes per stream is fine.
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    [pipewire::spa::pod::Pod::from_bytes(bytes).expect("a serialized pod is valid")]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rcli-screencast-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_restore_token_round_trips_through_the_state_dir_and_is_root_only() {
        let dir = dir("token");
        let store = TokenStore::new(dir.join(TOKEN_FILE));
        assert_eq!(store.load(), None, "no token before the first approval");
        store.save("tok-1\n").unwrap();
        assert_eq!(store.load().as_deref(), Some("tok-1"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(TOKEN_FILE)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        store.save("tok-2").unwrap();
        assert_eq!(store.load().as_deref(), Some("tok-2"), "a new token replaces the old one");
        store.save("  \n").unwrap();
        assert_eq!(store.load().as_deref(), Some("tok-2"), "a blank token does not erase the approval");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_blank_or_missing_token_file_reads_as_no_token() {
        let dir = dir("blank");
        let store = TokenStore::new(dir.join("missing"));
        assert_eq!(store.load(), None);
        std::fs::write(dir.join("blank"), "\n \n").unwrap();
        assert_eq!(TokenStore::new(dir.join("blank")).load(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rgba_and_bgrx_buffers_become_rgba_frames() {
        // 2x2: red, green / blue, white.
        let rgba_pixels: Vec<u8> = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
        assert_eq!(rgba(&rgba_pixels, 2, 2, 0, PixelFormat::Rgba).unwrap(), rgba_pixels);
        let bgrx_pixels: Vec<u8> = vec![0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255];
        assert_eq!(rgba(&bgrx_pixels, 2, 2, 0, PixelFormat::Bgrx).unwrap(), rgba_pixels);
        // 24-bit BGR has no alpha: frames get an opaque one.
        let bgr_pixels: Vec<u8> = vec![0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255];
        assert_eq!(rgba(&bgr_pixels, 2, 2, 0, PixelFormat::Bgr).unwrap(), rgba_pixels);
    }

    #[test]
    fn padded_rows_are_read_with_their_stride() {
        // One 2x2 RGB frame, each row followed by 6 bytes of GPU padding.
        let mut data = vec![1, 2, 3, 4, 5, 6];
        data.extend_from_slice(&[0xff; 6]);
        data.extend_from_slice(&[7, 8, 9, 10, 11, 12]);
        data.extend_from_slice(&[0xff; 6]);
        let converted = rgba(&data, 2, 2, 12, PixelFormat::Rgb).unwrap();
        assert_eq!(converted, vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]);
        // The last row usually has no trailing padding in `spa_chunk.size`.
        let mut data = vec![1, 2, 3, 4, 5, 6];
        data.extend_from_slice(&[0xff; 6]);
        data.extend_from_slice(&[7, 8, 9, 10, 11, 12]);
        assert_eq!(
            rgba(&data, 2, 2, 12, PixelFormat::Rgb).unwrap(),
            vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
        );
        // A buffer that is too small, or a stride shorter than a row, is an error, not a skew.
        assert!(rgba(&data[..10], 2, 2, 12, PixelFormat::Rgb).unwrap_err().to_string().contains("needed"));
        assert!(rgba(&data, 2, 2, 3, PixelFormat::Rgb).unwrap_err().to_string().contains("shorter"));
    }

    #[test]
    fn the_spa_formats_we_convert_are_the_packed_rgb_ones() {
        use pipewire::spa::param::video::VideoFormat;
        for (spa, pixel) in [
            (VideoFormat::RGBA, PixelFormat::Rgba),
            (VideoFormat::RGBx, PixelFormat::Rgbx),
            (VideoFormat::BGRx, PixelFormat::Bgrx),
            (VideoFormat::BGRA, PixelFormat::Bgra),
            (VideoFormat::RGB, PixelFormat::Rgb),
            (VideoFormat::BGR, PixelFormat::Bgr),
        ] {
            assert_eq!(PixelFormat::from_spa(spa.as_raw()), Some(pixel));
        }
        assert_eq!(PixelFormat::from_spa(VideoFormat::I420.as_raw()), None, "YUV is declined, not garbled");
    }

    #[test]
    fn the_portal_is_asked_for_monitors_in_a_persistent_session_with_the_stored_token() {
        let select = Select::new(Some("tok-1".into()));
        assert!(select.monitors && select.multiple && select.persistent && select.cursor_embedded);
        let options = format!("{:?}", select.options());
        assert!(options.contains("restore_token: Some(\"tok-1\")"), "{options}");
        assert!(options.contains("ExplicitlyRevoked"), "XDP_PERSIST_MODE_PERSISTENT: {options}");
        assert!(options.contains("multiple: Some(true)"), "{options}");
        assert!(options.contains("types: Some(SourceType(1))"), "MONITOR only: {options}");
        // The first session has no token to reuse.
        assert!(format!("{:?}", Select::new(None).options()).contains("restore_token: None"));
    }

    #[test]
    fn the_helper_publishes_the_token_the_daemon_persists() {
        assert_eq!(published_token(), None);
        publish_token("tok-wire");
        assert_eq!(published_token().as_deref(), Some("tok-wire"));
    }

    #[test]
    fn a_waiting_capture_reports_why_a_stream_failed() {
        let slot = Slot::default();
        slot.fail("the compositor's screen cast format I420 is not convertible to RGBA");
        let err = slot.wait(Duration::from_millis(10)).unwrap_err().to_string();
        assert!(err.contains("not convertible"), "{err}");
        let slot = Slot::default();
        slot.put(Frame { width: 1, height: 1, rgba: vec![0, 0, 0, 255] });
        assert_eq!(slot.wait(Duration::from_millis(10)).unwrap().width, 1);
    }
}
