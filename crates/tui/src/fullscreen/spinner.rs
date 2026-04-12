//! Animated spinner with seasonal character cycling, shimmer sweep, and stall detection.
//!
//! Renders a single status line with:
//! - Bounce-cycling glyphs (flowers for thinking, snowflakes for requesting)
//! - Per-character shimmer highlight sweeping left-to-right
//! - Smooth color transition to red on stall (no activity for >3s)

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::vibewords;

// ---------------------------------------------------------------------------
// RGB color and interpolation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb {
    const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    fn fg_escape(self) -> String {
        format!("\x1b[38;2;{};{};{}m", self.r, self.g, self.b)
    }
}

fn lerp_color(from: Rgb, to: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    Rgb {
        r: (from.r as f32 + (to.r as f32 - from.r as f32) * t).round() as u8,
        g: (from.g as f32 + (to.g as f32 - from.g as f32) * t).round() as u8,
        b: (from.b as f32 + (to.b as f32 - from.b as f32) * t).round() as u8,
    }
}

fn format_vibe_status(vibe: &str) -> String {
    format!("{vibe}...")
}

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Palette {
    base: Rgb,
    shimmer: Rgb,
    error: Rgb,
}

// ---------------------------------------------------------------------------
// Color themes — user-block background + spinner palette, unified
// ---------------------------------------------------------------------------

/// A named color theme controlling user input block background and spinner colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorTheme {
    Pink,
    Lavender,
    Ocean,
    Mint,
    Sunset,
    #[default]
    Slate,
}

impl ColorTheme {
    pub const ALL: &[ColorTheme] = &[
        ColorTheme::Pink,
        ColorTheme::Lavender,
        ColorTheme::Ocean,
        ColorTheme::Mint,
        ColorTheme::Sunset,
        ColorTheme::Slate,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ColorTheme::Pink => "pink",
            ColorTheme::Lavender => "lavender",
            ColorTheme::Ocean => "ocean",
            ColorTheme::Mint => "mint",
            ColorTheme::Sunset => "sunset",
            ColorTheme::Slate => "slate",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "pink" => Some(ColorTheme::Pink),
            "lavender" => Some(ColorTheme::Lavender),
            "ocean" => Some(ColorTheme::Ocean),
            "mint" => Some(ColorTheme::Mint),
            "sunset" => Some(ColorTheme::Sunset),
            "slate" => Some(ColorTheme::Slate),
            _ => None,
        }
    }

    /// The accent/base RGB for this theme (used for spinner, borders, etc).
    fn base_rgb(self) -> Rgb {
        match self {
            ColorTheme::Pink => Rgb::new(255, 182, 193),
            ColorTheme::Lavender => Rgb::new(200, 180, 230),
            ColorTheme::Ocean => Rgb::new(130, 190, 220),
            ColorTheme::Mint => Rgb::new(150, 220, 200),
            ColorTheme::Sunset => Rgb::new(240, 180, 140),
            ColorTheme::Slate => Rgb::new(138, 190, 183),
        }
    }

    fn palette(self) -> Palette {
        let base = self.base_rgb();
        let shimmer = match self {
            ColorTheme::Pink => Rgb::new(255, 218, 233),
            ColorTheme::Lavender => Rgb::new(230, 215, 255),
            ColorTheme::Ocean => Rgb::new(180, 220, 245),
            ColorTheme::Mint => Rgb::new(200, 245, 230),
            ColorTheme::Sunset => Rgb::new(255, 220, 195),
            ColorTheme::Slate => Rgb::new(0, 215, 255),
        };
        Palette {
            base,
            shimmer,
            error: Rgb::new(171, 43, 63),
        }
    }

    pub(crate) fn title_escape(self) -> String {
        if crate::theme::theme().colors_enabled() {
            self.base_rgb().fg_escape()
        } else {
            String::new()
        }
    }

    pub(crate) fn border_escape(self) -> String {
        if crate::theme::theme().colors_enabled() {
            let palette = self.palette();
            lerp_color(palette.base, palette.shimmer, 0.45).fg_escape()
        } else {
            String::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Season / character sets
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Season {
    Spring,
    Winter,
}

impl Season {
    fn chars(self) -> &'static [&'static str] {
        if crate::theme::compatibility_mode_enabled() {
            return match self {
                Season::Spring => &[".", "o", "O", "o", "."],
                Season::Winter => &[".", "*", "+", "*", "."],
            };
        }
        match self {
            Season::Spring => &["·", "❀", "❁", "✿", "❁", "❀"],
            Season::Winter => &["·", "❅", "❆", "❄", "❆", "❅"],
        }
    }

    /// Build bounce sequence: forward + reverse.
    fn bounce(self) -> Vec<&'static str> {
        let chars = self.chars();
        let mut seq: Vec<&str> = chars.to_vec();
        for &c in chars.iter().rev().skip(1) {
            seq.push(c);
        }
        seq
    }
}

// ---------------------------------------------------------------------------
// Spinner mode (public)
// ---------------------------------------------------------------------------

/// Controls which season (glyph set) and shimmer speed to use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpinnerMode {
    /// Flowers, slow shimmer — agent is thinking / processing.
    Thinking,
    /// Snowflakes, fast shimmer — waiting on API response.
    Requesting,
}

impl SpinnerMode {
    fn season(self) -> Season {
        match self {
            SpinnerMode::Thinking => Season::Spring,
            SpinnerMode::Requesting => Season::Winter,
        }
    }

    /// Shimmer advance interval in milliseconds.
    fn shimmer_speed_ms(self) -> u32 {
        match self {
            SpinnerMode::Thinking => 200,
            SpinnerMode::Requesting => 50,
        }
    }
}

// ---------------------------------------------------------------------------
// SpinnerState — the stateful renderer
// ---------------------------------------------------------------------------

const TICK_MS: u32 = 80; // matches fullscreen TUI tick interval
const FRAME_INTERVAL_MS: u32 = 120;
const STALL_THRESHOLD_MS: u64 = 3000;
const STALL_FADE_MS: f32 = 2000.0;
const SMOOTHING_FACTOR: f32 = 0.1;
const VIBE_FADE_IN_MS: u32 = 320;
const VIBE_INTERVAL_MIN_MS: u32 = 3000;
const VIBE_INTERVAL_MAX_MS: u32 = 5000;
const RESET: &str = "\x1b[0m";

/// Maintains all animation state for the spinner. Call `tick()` each frame,
/// then `render()` to get the ANSI-colored status string.
#[derive(Clone, Debug)]
pub struct SpinnerState {
    mode: SpinnerMode,
    color_theme: ColorTheme,
    bounce: Vec<&'static str>,
    tick_count: u64,
    // shimmer
    shimmer_accum_ms: u32,
    shimmer_pos: u32,
    // vibe words
    current_vibe: &'static str,
    next_vibe_change_ms: u32,
    vibe_elapsed_ms: u32,
    vibe_intro_ms: u32,
    // stall
    ticks_since_activity: u64,
    stall_intensity: f32,
}

impl SpinnerState {
    pub fn new(mode: SpinnerMode) -> Self {
        Self {
            mode,
            color_theme: ColorTheme::default(),
            bounce: mode.season().bounce(),
            tick_count: 0,
            shimmer_accum_ms: 0,
            shimmer_pos: 0,
            current_vibe: vibewords::random_vibe(),
            next_vibe_change_ms: Self::random_vibe_interval_ms(),
            vibe_elapsed_ms: 0,
            vibe_intro_ms: VIBE_FADE_IN_MS,
            ticks_since_activity: 0,
            stall_intensity: 0.0,
        }
    }

    pub fn set_mode(&mut self, mode: SpinnerMode) {
        if self.mode != mode {
            self.mode = mode;
            self.bounce = mode.season().bounce();
            self.shimmer_accum_ms = 0;
        }
    }

    pub fn set_color_theme(&mut self, theme: ColorTheme) {
        self.color_theme = theme;
    }

    pub fn notify_activity(&mut self) {
        self.ticks_since_activity = 0;
    }

    fn random_vibe_interval_ms() -> u32 {
        rand::Rng::gen_range(
            &mut rand::thread_rng(),
            VIBE_INTERVAL_MIN_MS..=VIBE_INTERVAL_MAX_MS,
        )
    }

    fn rotate_vibe(&mut self) {
        self.current_vibe = vibewords::random_vibe_excluding(Some(self.current_vibe));
        self.next_vibe_change_ms = Self::random_vibe_interval_ms();
        self.vibe_elapsed_ms = 0;
        self.vibe_intro_ms = 0;
    }

    /// Advance animation by one tick. Call once per TUI tick (~80ms).
    pub fn tick(&mut self) {
        self.tick_count += 1;
        self.ticks_since_activity += 1;

        // Advance shimmer position based on mode speed
        self.shimmer_accum_ms += TICK_MS;
        let speed = self.mode.shimmer_speed_ms();
        while self.shimmer_accum_ms >= speed {
            self.shimmer_accum_ms -= speed;
            self.shimmer_pos = self.shimmer_pos.wrapping_add(1);
        }

        self.vibe_elapsed_ms = self.vibe_elapsed_ms.saturating_add(TICK_MS);
        self.vibe_intro_ms = (self.vibe_intro_ms + TICK_MS).min(VIBE_FADE_IN_MS);
        if self.vibe_elapsed_ms >= self.next_vibe_change_ms {
            self.rotate_vibe();
        }

        // Update stall intensity with exponential smoothing
        let elapsed_ms = self.ticks_since_activity * TICK_MS as u64;
        let target = if elapsed_ms <= STALL_THRESHOLD_MS {
            0.0
        } else {
            ((elapsed_ms - STALL_THRESHOLD_MS) as f32 / STALL_FADE_MS).min(1.0)
        };
        self.stall_intensity += (target - self.stall_intensity) * SMOOTHING_FACTOR;
        if self.stall_intensity < 0.001 {
            self.stall_intensity = 0.0;
        }
    }

    /// Render the spinner + message as an ANSI-colored string.
    pub fn render(&self, message: &str, max_width: usize) -> String {
        let palette = &self.color_theme.palette();

        // Current glyph from bounce cycle
        let frame_idx = ((self.tick_count * TICK_MS as u64) / FRAME_INTERVAL_MS as u64) as usize;
        let glyph = self.bounce[frame_idx % self.bounce.len()];

        // Glyph color
        let glyph_color = if self.stall_intensity > 0.0 {
            lerp_color(palette.base, palette.error, self.stall_intensity)
        } else {
            palette.base
        };

        let mut out = String::with_capacity(256);

        out.push_str(&glyph_color.fg_escape());
        out.push_str(glyph);
        out.push_str(RESET);
        out.push(' ');

        let available = max_width.saturating_sub(3);
        // Give the status text and the rotating vibe word a clearer visual break
        // so lines like `Compacting session... • 27.0s · stitching...` feel less cramped.
        let separator = "  ·  ";
        let separator_width = UnicodeWidthStr::width(separator);
        let vibe_status = format_vibe_status(self.current_vibe);
        let vibe_width = UnicodeWidthStr::width(vibe_status.as_str());
        let has_message = !message.trim().is_empty();

        if !has_message {
            self.render_vibe_segment(&mut out, &vibe_status, available, palette);
            out.push_str(RESET);
            return out;
        }

        let reserve_for_vibe = if available > vibe_width + separator_width + 12 {
            vibe_width + separator_width
        } else {
            0
        };
        let message_width = available.saturating_sub(reserve_for_vibe);

        self.render_message_segment(&mut out, message, message_width, palette);
        if reserve_for_vibe > 0 {
            out.push_str(&palette.base.fg_escape());
            out.push_str(separator);
            out.push_str(RESET);
            self.render_vibe_segment(&mut out, &vibe_status, vibe_width, palette);
        }
        out.push_str(RESET);

        out
    }

    fn render_message_segment(
        &self,
        out: &mut String,
        message: &str,
        max_width: usize,
        palette: &Palette,
    ) {
        let msg_width = UnicodeWidthStr::width(message);
        let cycle_len = (msg_width + 20) as i32;
        let glimmer_center = (self.shimmer_pos as i32 % cycle_len) - 10;

        let mut col: i32 = 0;
        for grapheme in message.graphemes(true) {
            let gw = UnicodeWidthStr::width(grapheme) as i32;
            if (col + gw) as usize > max_width {
                out.push_str(
                    &lerp_color(palette.base, palette.error, self.stall_intensity).fg_escape(),
                );
                out.push('…');
                out.push_str(RESET);
                break;
            }

            let char_color = if self.stall_intensity > 0.0 {
                lerp_color(palette.base, palette.error, self.stall_intensity)
            } else {
                let distance = (col - glimmer_center).unsigned_abs() as f32;
                let intensity = if distance > 3.0 {
                    0.0
                } else {
                    1.0 - distance / 3.0
                };
                lerp_color(palette.base, palette.shimmer, intensity)
            };

            out.push_str(&char_color.fg_escape());
            out.push_str(grapheme);
            col += gw;
        }
        out.push_str(RESET);
    }

    fn render_vibe_segment(
        &self,
        out: &mut String,
        vibe: &str,
        max_width: usize,
        palette: &Palette,
    ) {
        let visible_width = UnicodeWidthStr::width(vibe);
        let intro_progress = self.vibe_intro_ms as f32 / VIBE_FADE_IN_MS as f32;
        let shimmer_center = -2.0 + (visible_width as f32 + 4.0) * intro_progress;

        let mut col = 0f32;
        for grapheme in vibe.graphemes(true) {
            let gw = UnicodeWidthStr::width(grapheme);
            if (col as usize + gw) > max_width {
                break;
            }

            let color = if self.stall_intensity > 0.0 {
                lerp_color(palette.base, palette.error, self.stall_intensity)
            } else if self.vibe_intro_ms < VIBE_FADE_IN_MS {
                let distance = (col - shimmer_center).abs();
                let intensity = if distance > 3.0 {
                    0.0
                } else {
                    1.0 - distance / 3.0
                };
                lerp_color(palette.base, palette.shimmer, intensity)
            } else {
                palette.base
            };

            out.push_str(&color.fg_escape());
            out.push_str(grapheme);
            col += gw as f32;
        }
        out.push_str(RESET);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_boundaries() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(255, 255, 255);
        let c0 = lerp_color(a, b, 0.0);
        assert_eq!((c0.r, c0.g, c0.b), (0, 0, 0));
        let c1 = lerp_color(a, b, 1.0);
        assert_eq!((c1.r, c1.g, c1.b), (255, 255, 255));
        let mid = lerp_color(a, b, 0.5);
        assert_eq!((mid.r, mid.g, mid.b), (128, 128, 128));
    }

    #[test]
    fn lerp_clamps() {
        let a = Rgb::new(100, 100, 100);
        let b = Rgb::new(200, 200, 200);
        let under = lerp_color(a, b, -1.0);
        assert_eq!((under.r, under.g, under.b), (100, 100, 100));
        let over = lerp_color(a, b, 2.0);
        assert_eq!((over.r, over.g, over.b), (200, 200, 200));
    }

    #[test]
    fn bounce_sequence_is_correct() {
        let bounce = Season::Spring.bounce();
        assert_eq!(bounce.len(), 11); // 6 forward + 5 reverse (skip last dup)
        assert_eq!(bounce[0], "·");
        assert_eq!(bounce[5], "❀");
        assert_eq!(bounce[10], "·"); // back to start
    }

    #[test]
    fn render_contains_glyph_message_and_vibe_chars() {
        let state = SpinnerState::new(SpinnerMode::Thinking);
        let output = state.render("Loading...", 80);
        let plain = crate::utils::strip_ansi(&output);
        // Message is rendered per-grapheme with color codes, so check
        // against stripped text for content and raw output for color.
        assert!(plain.contains("Loading..."));
        assert!(plain.contains("·")); // first frame glyph
        assert!(plain.contains(&format_vibe_status(state.current_vibe)));
        // Should contain truecolor escapes
        assert!(output.contains("\x1b[38;2;"));
    }

    #[test]
    fn stall_intensity_ramps_after_threshold() {
        let mut state = SpinnerState::new(SpinnerMode::Thinking);
        // 3 seconds of no activity = 3000/80 ≈ 38 ticks
        for _ in 0..38 {
            state.tick();
        }
        assert!(state.stall_intensity < 0.01, "should not stall yet");

        // 50 more ticks (~4 more seconds)
        for _ in 0..50 {
            state.tick();
        }
        assert!(state.stall_intensity > 0.1, "should be stalling now");
    }

    #[test]
    fn activity_resets_stall() {
        let mut state = SpinnerState::new(SpinnerMode::Thinking);
        for _ in 0..100 {
            state.tick();
        }
        assert!(state.stall_intensity > 0.0);
        state.notify_activity();
        // Exponential smoothing needs many ticks to fully decay
        for _ in 0..80 {
            state.tick();
            state.notify_activity();
        }
        assert!(state.stall_intensity < 0.01);
    }

    #[test]
    fn mode_switch_changes_glyph_set() {
        let mut state = SpinnerState::new(SpinnerMode::Thinking);
        assert_eq!(state.bounce[3], "✿"); // spring flower
        state.set_mode(SpinnerMode::Requesting);
        assert_eq!(state.bounce[3], "❄"); // winter snowflake
    }

    #[test]
    fn vibe_word_rotates_after_interval() {
        let mut state = SpinnerState::new(SpinnerMode::Thinking);
        let first = state.current_vibe;
        for _ in 0..80 {
            state.tick();
        }
        assert_ne!(state.current_vibe, first);
    }

    #[test]
    fn empty_message_renders_vibe_with_ellipsis_without_working_label() {
        let state = SpinnerState::new(SpinnerMode::Thinking);
        let output = state.render("", 80);
        let plain = crate::utils::strip_ansi(&output);

        assert!(!plain.contains("Working..."));
        assert!(plain.contains(&format_vibe_status(state.current_vibe)));
    }
}
