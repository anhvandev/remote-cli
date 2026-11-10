//! Input on a Wayland session: the root service creates a virtual pointer and keyboard on `/dev/uinput`
//! (EV_KEY/EV_REL/EV_ABS) and writes evdev events to it, because there is no XTest to borrow.
//!
//! The pointer is absolute: the desktop's bounding box (all displays, `displays()`) maps onto the
//! `0..=ABS_MAX` range that libinput/compositors transform back to screen pixels. Keys are Linux evdev
//! codes, so the compositor's current keyboard layout applies (typing is US-layout only; see `text`).

use evdev::{
    uinput::VirtualDevice, AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, PropType,
    RelativeAxisCode, UinputAbsSetup,
};

use crate::platform::{Button, DisplayInfo, Input};

/// The absolute axis range a virtual pointer reports. Every compositor (mutter, kwin, wlroots) transforms
/// this range onto the desktop, so the Agent moves the pointer to a real pixel by mapping it here.
pub const ABS_MAX: i32 = 32767;

/// One key press a typed character needs: the evdev key, and whether Shift must be held for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stroke {
    pub key: KeyCode,
    pub shift: bool,
}

/// The events the virtual device writes for one `Input`. Missing displays are an error; everything else is
/// resolved here, so the mapping is testable without `/dev/uinput`.
pub fn events(input: &Input, displays: &[DisplayInfo]) -> anyhow::Result<Vec<InputEvent>> {
    let mut out = vec![];
    match input {
        Input::Move { display, x, y } => {
            let (ax, ay) = abs_position(displays, *display, *x, *y)?;
            out.push(abs(AbsoluteAxisCode::ABS_X, ax));
            out.push(abs(AbsoluteAxisCode::ABS_Y, ay));
        }
        Input::Press { button, down } => out.push(key(button_code(*button), *down)),
        Input::Scroll { dx, dy } => {
            if *dy != 0 {
                out.push(rel(RelativeAxisCode::REL_WHEEL, -*dy));
            }
            if *dx != 0 {
                out.push(rel(RelativeAxisCode::REL_HWHEEL, *dx));
            }
        }
        Input::Text(text) => {
            for stroke in text_strokes(text)? {
                push_stroke(&mut out, stroke, true);
            }
        }
        Input::Key(name) => push_stroke(&mut out, named(name)?, true),
        Input::KeyDown(name) => {
            let stroke = named(name)?;
            push_shift(&mut out, stroke.shift, true);
            out.push(key(stroke.key, true));
        }
        Input::KeyUp(name) => {
            let stroke = named(name)?;
            out.push(key(stroke.key, false));
            push_shift(&mut out, stroke.shift, false);
        }
    }
    Ok(out)
}

/// The evdev key a named key stands for (`"enter"`, `"a"`, `"f5"`), and whether Shift is part of it.
pub fn named(name: &str) -> anyhow::Result<Stroke> {
    let lower = name.to_ascii_lowercase();
    let code = match lower.as_str() {
        "enter" | "return" => KeyCode::KEY_ENTER,
        "tab" => KeyCode::KEY_TAB,
        "esc" | "escape" => KeyCode::KEY_ESC,
        "backspace" => KeyCode::KEY_BACKSPACE,
        "delete" | "del" => KeyCode::KEY_DELETE,
        "insert" => KeyCode::KEY_INSERT,
        "space" => KeyCode::KEY_SPACE,
        "up" => KeyCode::KEY_UP,
        "down" => KeyCode::KEY_DOWN,
        "left" => KeyCode::KEY_LEFT,
        "right" => KeyCode::KEY_RIGHT,
        "home" => KeyCode::KEY_HOME,
        "end" => KeyCode::KEY_END,
        "pageup" => KeyCode::KEY_PAGEUP,
        "pagedown" => KeyCode::KEY_PAGEDOWN,
        "capslock" => KeyCode::KEY_CAPSLOCK,
        "ctrl" | "control" => KeyCode::KEY_LEFTCTRL,
        "shift" => KeyCode::KEY_LEFTSHIFT,
        "alt" => KeyCode::KEY_LEFTALT,
        "super" | "meta" | "win" | "cmd" => KeyCode::KEY_LEFTMETA,
        // F11 and F12 are not next to F1..F10 in evdev's scan-code order.
        f if f.len() > 1 && f.starts_with('f') => {
            let n: u16 = f[1..].parse().map_err(|_| anyhow::anyhow!("unknown key {name:?}"))?;
            match n {
                1 => KeyCode::KEY_F1, 2 => KeyCode::KEY_F2, 3 => KeyCode::KEY_F3, 4 => KeyCode::KEY_F4,
                5 => KeyCode::KEY_F5, 6 => KeyCode::KEY_F6, 7 => KeyCode::KEY_F7, 8 => KeyCode::KEY_F8,
                9 => KeyCode::KEY_F9, 10 => KeyCode::KEY_F10, 11 => KeyCode::KEY_F11, 12 => KeyCode::KEY_F12,
                _ => anyhow::bail!("unknown key {name:?}"),
            }
        }
        // A single character: the key that types it (letters keep their case, so Shift is part of it).
        _ if name.chars().count() == 1 => {
            return char_stroke(name.chars().next().unwrap_or_default()).ok_or_else(|| anyhow::anyhow!("unknown key {name:?}"))
        }
        _ => anyhow::bail!("unknown key {name:?}"),
    };
    Ok(Stroke { key: code, shift: false })
}

/// The strokes that type `text` on a US keyboard layout (the compositor's layout is what applies).
pub fn text_strokes(text: &str) -> anyhow::Result<Vec<Stroke>> {
    text.chars()
        .map(|c| char_stroke(c).ok_or_else(|| anyhow::anyhow!("cannot type {c:?} on the virtual keyboard (US layout only)")))
        .collect()
}

/// The key and Shift state that type `c` on a US keyboard layout; `None` for characters without one.
fn char_stroke(c: char) -> Option<Stroke> {
    let stroke = |key, shift| Some(Stroke { key, shift });
    // Evdev follows the hardware scan-code order, so neither letters nor digits are contiguous.
    let letters = [
        ('a', KeyCode::KEY_A), ('b', KeyCode::KEY_B), ('c', KeyCode::KEY_C), ('d', KeyCode::KEY_D),
        ('e', KeyCode::KEY_E), ('f', KeyCode::KEY_F), ('g', KeyCode::KEY_G), ('h', KeyCode::KEY_H),
        ('i', KeyCode::KEY_I), ('j', KeyCode::KEY_J), ('k', KeyCode::KEY_K), ('l', KeyCode::KEY_L),
        ('m', KeyCode::KEY_M), ('n', KeyCode::KEY_N), ('o', KeyCode::KEY_O), ('p', KeyCode::KEY_P),
        ('q', KeyCode::KEY_Q), ('r', KeyCode::KEY_R), ('s', KeyCode::KEY_S), ('t', KeyCode::KEY_T),
        ('u', KeyCode::KEY_U), ('v', KeyCode::KEY_V), ('w', KeyCode::KEY_W), ('x', KeyCode::KEY_X),
        ('y', KeyCode::KEY_Y), ('z', KeyCode::KEY_Z),
    ];
    let digits = [
        ('1', KeyCode::KEY_1), ('2', KeyCode::KEY_2), ('3', KeyCode::KEY_3), ('4', KeyCode::KEY_4),
        ('5', KeyCode::KEY_5), ('6', KeyCode::KEY_6), ('7', KeyCode::KEY_7), ('8', KeyCode::KEY_8),
        ('9', KeyCode::KEY_9), ('0', KeyCode::KEY_0),
    ];
    if let Some((_, key)) = letters.iter().find(|(letter, _)| *letter == c.to_ascii_lowercase()) {
        return stroke(*key, c.is_ascii_uppercase());
    }
    if let Some((_, key)) = digits.iter().find(|(digit, _)| *digit == c) {
        return stroke(*key, false);
    }
    match c {
        ' ' => stroke(KeyCode::KEY_SPACE, false),
        '\n' => stroke(KeyCode::KEY_ENTER, false),
        '\t' => stroke(KeyCode::KEY_TAB, false),
        '`' => stroke(KeyCode::KEY_GRAVE, false),
        '~' => stroke(KeyCode::KEY_GRAVE, true),
        '-' => stroke(KeyCode::KEY_MINUS, false),
        '_' => stroke(KeyCode::KEY_MINUS, true),
        '=' => stroke(KeyCode::KEY_EQUAL, false),
        '+' => stroke(KeyCode::KEY_EQUAL, true),
        '[' => stroke(KeyCode::KEY_LEFTBRACE, false),
        '{' => stroke(KeyCode::KEY_LEFTBRACE, true),
        ']' => stroke(KeyCode::KEY_RIGHTBRACE, false),
        '}' => stroke(KeyCode::KEY_RIGHTBRACE, true),
        '\\' => stroke(KeyCode::KEY_BACKSLASH, false),
        '|' => stroke(KeyCode::KEY_BACKSLASH, true),
        ';' => stroke(KeyCode::KEY_SEMICOLON, false),
        ':' => stroke(KeyCode::KEY_SEMICOLON, true),
        '\'' => stroke(KeyCode::KEY_APOSTROPHE, false),
        '"' => stroke(KeyCode::KEY_APOSTROPHE, true),
        ',' => stroke(KeyCode::KEY_COMMA, false),
        '<' => stroke(KeyCode::KEY_COMMA, true),
        '.' => stroke(KeyCode::KEY_DOT, false),
        '>' => stroke(KeyCode::KEY_DOT, true),
        '/' => stroke(KeyCode::KEY_SLASH, false),
        '?' => stroke(KeyCode::KEY_SLASH, true),
        '!' => stroke(KeyCode::KEY_1, true),
        '@' => stroke(KeyCode::KEY_2, true),
        '#' => stroke(KeyCode::KEY_3, true),
        '$' => stroke(KeyCode::KEY_4, true),
        '%' => stroke(KeyCode::KEY_5, true),
        '^' => stroke(KeyCode::KEY_6, true),
        '&' => stroke(KeyCode::KEY_7, true),
        '*' => stroke(KeyCode::KEY_8, true),
        '(' => stroke(KeyCode::KEY_9, true),
        ')' => stroke(KeyCode::KEY_0, true),
        _ => None,
    }
}

/// The absolute `(x, y)` for real pixel `(x, y)` of `display`, across the bounding box of all displays.
pub fn abs_position(displays: &[DisplayInfo], display: u32, x: u32, y: u32) -> anyhow::Result<(i32, i32)> {
    let d = displays.iter().find(|d| d.index == display).ok_or_else(|| anyhow::anyhow!("no display {display}"))?;
    let min_x = displays.iter().map(|d| d.x).min().unwrap_or(d.x);
    let min_y = displays.iter().map(|d| d.y).min().unwrap_or(d.y);
    let max_x = displays.iter().map(|d| d.x.saturating_add(d.width as i32)).max().unwrap_or(d.x + d.width as i32);
    let max_y = displays.iter().map(|d| d.y.saturating_add(d.height as i32)).max().unwrap_or(d.y + d.height as i32);
    // A pixel past the edge still means the edge of that display, not a wrapping pointer.
    let gx = d.x.saturating_add(x.min(d.width.saturating_sub(1)) as i32);
    let gy = d.y.saturating_add(y.min(d.height.saturating_sub(1)) as i32);
    Ok((spread(gx, min_x, max_x), spread(gy, min_y, max_y)))
}

/// Maps `value` in `min..max` (max exclusive) onto `0..=ABS_MAX`; a one-pixel desktop has nothing to spread.
fn spread(value: i32, min: i32, max: i32) -> i32 {
    let span = max.saturating_sub(min);
    if span <= 1 {
        return 0;
    }
    let value = value.clamp(min, max - 1);
    ((i64::from(value - min) * i64::from(ABS_MAX)) / i64::from(span - 1)) as i32
}

/// A virtual pointer and keyboard on `/dev/uinput`. Needs root; a user-run Agent cannot create one.
pub struct VirtualInput {
    device: VirtualDevice,
}

impl VirtualInput {
    /// Creates the device: buttons, wheel and the absolute X/Y axes, plus every key the adapter can send.
    pub fn create() -> anyhow::Result<Self> {
        let keys = key_capabilities()?;
        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        axes.insert(RelativeAxisCode::REL_WHEEL);
        axes.insert(RelativeAxisCode::REL_HWHEEL);
        let mut props = AttributeSet::<PropType>::new();
        // A pointer, not a touchscreen: libinput and the compositors then handle the absolute axes as a cursor.
        props.insert(PropType::POINTER);
        let device = VirtualDevice::builder()
            .map_err(|e| anyhow::anyhow!("Wayland input needs /dev/uinput writable by root: {e}"))?
            .name("rcli virtual pointer and keyboard")
            .with_keys(&keys)?
            .with_relative_axes(&axes)?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, AbsInfo::new(0, 0, ABS_MAX, 0, 0, 0)))?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, AbsInfo::new(0, 0, ABS_MAX, 0, 0, 0)))?
            .with_properties(&props)?
            .build()
            .map_err(|e| anyhow::anyhow!("cannot create the virtual input device on /dev/uinput: {e}"))?;
        Ok(Self { device })
    }

    /// Emits the events for `input`, with one SYN_REPORT at the end.
    pub fn input(&mut self, input: Input, displays: &[DisplayInfo]) -> anyhow::Result<()> {
        let events = events(&input, displays)?;
        anyhow::ensure!(!events.is_empty(), "nothing to send for {input:?}");
        self.device.emit(&events).map_err(|e| anyhow::anyhow!("cannot write to /dev/uinput: {e}"))
    }
}

/// Every key the adapter may send: the named keys, `f1`..`f12`, all printable ASCII and the mouse buttons.
fn key_capabilities() -> anyhow::Result<AttributeSet<KeyCode>> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for name in [
        "enter", "tab", "esc", "backspace", "delete", "insert", "space", "up", "down", "left", "right", "home", "end",
        "pageup", "pagedown", "capslock", "ctrl", "shift", "alt", "super",
    ] {
        keys.insert(named(name)?.key);
    }
    for n in 1..=12 {
        keys.insert(named(&format!("f{n}"))?.key);
    }
    for c in (' '..='~').chain(['\n', '\t']) {
        if let Some(stroke) = char_stroke(c) {
            keys.insert(stroke.key);
        }
    }
    for button in [Button::Left, Button::Right, Button::Middle] {
        keys.insert(button_code(button));
    }
    Ok(keys)
}

fn button_code(button: Button) -> KeyCode {
    match button {
        Button::Left => KeyCode::BTN_LEFT,
        Button::Right => KeyCode::BTN_RIGHT,
        Button::Middle => KeyCode::BTN_MIDDLE,
    }
}

fn key(code: KeyCode, down: bool) -> InputEvent {
    InputEvent::new(EventType::KEY.0, code.0, i32::from(down))
}

fn abs(axis: AbsoluteAxisCode, value: i32) -> InputEvent {
    InputEvent::new(EventType::ABSOLUTE.0, axis.0, value)
}

fn rel(axis: RelativeAxisCode, value: i32) -> InputEvent {
    InputEvent::new(EventType::RELATIVE.0, axis.0, value)
}

fn push_shift(out: &mut Vec<InputEvent>, shift: bool, down: bool) {
    if shift {
        out.push(key(KeyCode::KEY_LEFTSHIFT, down));
    }
}

/// A press (`down`) or a full tap of one stroke, with Shift held around it when it needs it.
fn push_stroke(out: &mut Vec<InputEvent>, stroke: Stroke, down: bool) {
    push_shift(out, stroke.shift, true);
    out.push(key(stroke.key, true));
    if down {
        out.push(key(stroke.key, false));
        push_shift(out, stroke.shift, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(index: u32, x: i32, y: i32, width: u32, height: u32) -> DisplayInfo {
        DisplayInfo { index, x, y, width, height, scale: 1.0, primary: index == 0 }
    }

    fn desktop() -> Vec<DisplayInfo> {
        vec![display(0, 0, 0, 100, 100), display(1, 100, 0, 100, 100)]
    }

    #[test]
    fn letters_digits_and_named_keys_map_to_evdev_keys() {
        assert_eq!(named("a").unwrap(), Stroke { key: KeyCode::KEY_A, shift: false });
        assert_eq!(named("A").unwrap(), Stroke { key: KeyCode::KEY_A, shift: true });
        assert_eq!(named("5").unwrap(), Stroke { key: KeyCode::KEY_5, shift: false });
        assert_eq!(named(" ").unwrap(), Stroke { key: KeyCode::KEY_SPACE, shift: false });
        assert_eq!(named("enter").unwrap(), Stroke { key: KeyCode::KEY_ENTER, shift: false });
        assert_eq!(named("Return").unwrap(), Stroke { key: KeyCode::KEY_ENTER, shift: false });
        assert_eq!(named("esc").unwrap(), Stroke { key: KeyCode::KEY_ESC, shift: false });
        assert_eq!(named("pageup").unwrap(), Stroke { key: KeyCode::KEY_PAGEUP, shift: false });
        assert_eq!(named("ctrl").unwrap(), Stroke { key: KeyCode::KEY_LEFTCTRL, shift: false });
        assert_eq!(named("shift").unwrap(), Stroke { key: KeyCode::KEY_LEFTSHIFT, shift: false });
        assert_eq!(named("super").unwrap(), Stroke { key: KeyCode::KEY_LEFTMETA, shift: false });
        assert_eq!(named("F12").unwrap(), Stroke { key: KeyCode::KEY_F12, shift: false });
        assert!(named("hyper").is_err(), "an unknown key name is an error, not a guess");
        assert!(named("f13").is_err());
    }

    #[test]
    fn typing_holds_shift_only_for_the_characters_that_need_it() {
        assert_eq!(
            text_strokes("Hi!").unwrap(),
            vec![
                Stroke { key: KeyCode::KEY_H, shift: true },
                Stroke { key: KeyCode::KEY_I, shift: false },
                Stroke { key: KeyCode::KEY_1, shift: true },
            ]
        );
        assert_eq!(text_strokes("\n\t").unwrap(), vec![
            Stroke { key: KeyCode::KEY_ENTER, shift: false },
            Stroke { key: KeyCode::KEY_TAB, shift: false },
        ]);
        assert_eq!(
            text_strokes("_+{}|:\"<>?~").unwrap().iter().map(|s| (s.key, s.shift)).collect::<Vec<_>>(),
            vec![
                (KeyCode::KEY_MINUS, true),
                (KeyCode::KEY_EQUAL, true),
                (KeyCode::KEY_LEFTBRACE, true),
                (KeyCode::KEY_RIGHTBRACE, true),
                (KeyCode::KEY_BACKSLASH, true),
                (KeyCode::KEY_SEMICOLON, true),
                (KeyCode::KEY_APOSTROPHE, true),
                (KeyCode::KEY_COMMA, true),
                (KeyCode::KEY_DOT, true),
                (KeyCode::KEY_SLASH, true),
                (KeyCode::KEY_GRAVE, true),
            ]
        );
        // Not a US-layout character: a clear error rather than a wrong key.
        assert!(text_strokes("héllo").unwrap_err().to_string().contains("cannot type"));
    }

    #[test]
    fn a_move_maps_the_whole_desktop_onto_the_absolute_range() {
        let d = desktop();
        assert_eq!(abs_position(&d, 0, 0, 0).unwrap(), (0, 0));
        assert_eq!(abs_position(&d, 1, 99, 99).unwrap(), (ABS_MAX, ABS_MAX));
        // 100 px wide: x = 50 is halfway, 50 * 32767 / 199 = 8232.
        assert_eq!(abs_position(&d, 0, 50, 0).unwrap(), (8232, 0));
        // Display 1 starts at x = 100: x = 50 is 150 across 200 px, 150 * 32767 / 199 = 24698.
        assert_eq!(abs_position(&d, 1, 50, 0).unwrap(), (24698, 0));
        assert!(abs_position(&d, 2, 0, 0).unwrap_err().to_string().contains("no display 2"));
    }

    #[test]
    fn a_move_past_the_display_clamps_to_its_edge() {
        let d = desktop();
        assert_eq!(abs_position(&d, 0, 5000, 5000).unwrap(), (abs_position(&d, 0, 99, 99).unwrap()));
    }

    #[test]
    fn one_pixel_desktops_map_to_the_origin_instead_of_dividing_by_zero() {
        let d = vec![display(0, 0, 0, 1, 1)];
        assert_eq!(abs_position(&d, 0, 0, 0).unwrap(), (0, 0));
    }

    #[test]
    fn a_move_emits_absolute_axes_and_a_click_emits_the_button() {
        let d = desktop();
        let moved = events(&Input::Move { display: 0, x: 0, y: 99 }, &d).unwrap();
        assert_eq!(moved, vec![abs(AbsoluteAxisCode::ABS_X, 0), abs(AbsoluteAxisCode::ABS_Y, ABS_MAX)]);
        assert_eq!(
            events(&Input::Press { button: Button::Right, down: true }, &d).unwrap(),
            vec![InputEvent::new(EventType::KEY.0, KeyCode::BTN_RIGHT.0, 1)]
        );
        // Positive dy scrolls down: the wheel event is negative.
        assert_eq!(
            events(&Input::Scroll { dx: 0, dy: 3 }, &d).unwrap(),
            vec![rel(RelativeAxisCode::REL_WHEEL, -3)]
        );
        assert_eq!(
            events(&Input::Scroll { dx: -2, dy: 0 }, &d).unwrap(),
            vec![rel(RelativeAxisCode::REL_HWHEEL, -2)]
        );
    }

    #[test]
    fn a_typed_character_is_shift_key_press_release_shift_release() {
        assert_eq!(
            events(&Input::Text("A".into()), &desktop()).unwrap(),
            vec![
                key(KeyCode::KEY_LEFTSHIFT, true),
                key(KeyCode::KEY_A, true),
                key(KeyCode::KEY_A, false),
                key(KeyCode::KEY_LEFTSHIFT, false),
            ]
        );
        assert_eq!(
            events(&Input::Text("a".into()), &desktop()).unwrap(),
            vec![key(KeyCode::KEY_A, true), key(KeyCode::KEY_A, false)]
        );
        // KeyDown/KeyUp keep Shift held across the calls, for drags and combinations.
        assert_eq!(
            events(&Input::KeyDown("A".into()), &desktop()).unwrap(),
            vec![key(KeyCode::KEY_LEFTSHIFT, true), key(KeyCode::KEY_A, true)]
        );
        assert_eq!(
            events(&Input::KeyUp("A".into()), &desktop()).unwrap(),
            vec![key(KeyCode::KEY_A, false), key(KeyCode::KEY_LEFTSHIFT, false)]
        );
    }
}
