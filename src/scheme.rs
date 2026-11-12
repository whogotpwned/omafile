//! A colour scheme from a picture.
//!
//! The picture is looked at once (`analyse`): how bright it is, which tone its
//! dark and its light areas have, and which vivid colours stand out. From that a
//! complete scheme in the form of Omarchy's `colors.toml` is built (`scheme`):
//! background and foreground carry the picture's tone, the accent is one of its
//! vivid colours, and the sixteen terminal colours keep their meaning (red stays
//! red) but lean towards the shades the picture has. Every colour is moved in
//! lightness until it can be read on the background.
//!
//! Pure arithmetic on pixels: no image library, no clustering crate.

use crate::theme::Rgb;

/// Hue in degrees, saturation and lightness in 0..1.
pub fn to_hsl(c: Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-5 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-5);
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h.rem_euclid(360.0), s.min(1.0), l)
}

pub fn from_hsl(h: f32, s: f32, l: f32) -> Rgb {
    let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let u = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb(u(r), u(g), u(b))
}

/// Contrast ratio of two colours as WCAG defines it: 1 (none) to 21 (black on white).
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let lum = |c: Rgb| 0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2);
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Move a colour's lightness away from the background until the contrast is reached
/// (or lightness runs out). Hue and saturation stay.
pub fn readable(color: Rgb, on: Rgb, wanted: f32) -> Rgb {
    let (h, s, mut l) = to_hsl(color);
    let lighter = to_hsl(on).2 < 0.5;
    let mut out = color;
    for _ in 0..60 {
        if contrast(out, on) >= wanted {
            break;
        }
        l += if lighter { 0.015 } else { -0.015 };
        if !(0.0..=1.0).contains(&l) {
            break;
        }
        out = from_hsl(h, s, l);
    }
    out
}

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// A vivid colour of the picture and how much of the picture it covers (0..1, weighted).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vivid {
    pub color: Rgb,
    pub hue: f32,
    pub saturation: f32,
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    /// Mean brightness, 0..1.
    pub brightness: f32,
    /// Tone of the dark and of the light areas: hue and saturation.
    pub dark_tone: (f32, f32),
    pub light_tone: (f32, f32),
    /// The vivid colours, strongest first; at most six, at least 30° of hue apart.
    pub vivid: Vec<Vivid>,
    /// How colourful the picture is overall, 0..1.
    pub colourfulness: f32,
}

impl Analysis {
    /// A dark picture suggests a dark scheme.
    pub fn suggests_light(&self) -> bool {
        self.brightness > 0.58
    }
}

/// Look at a picture's pixels (a small copy of it is plenty: a hundred pixels across).
pub fn analyse(pixels: &[[u8; 3]]) -> Analysis {
    const SECTORS: usize = 36;
    let mut sector = [[0f64; 5]; SECTORS]; // weight, r, g, b, saturation (all weighted)
    let (mut dark, mut light, mut all) = ([0f64; 4], [0f64; 4], [0f64; 4]);
    let (mut brightness, mut colour) = (0f64, 0f64);
    for p in pixels {
        let c = Rgb(p[0], p[1], p[2]);
        let (h, s, l) = to_hsl(c);
        brightness += c.luminance() as f64;
        let add = |acc: &mut [f64; 4], w: f64| {
            acc[0] += w;
            acc[1] += p[0] as f64 * w;
            acc[2] += p[1] as f64 * w;
            acc[3] += p[2] as f64 * w;
        };
        add(&mut all, 1.0);
        if l < 0.35 {
            add(&mut dark, 1.0);
        }
        if l > 0.65 {
            add(&mut light, 1.0);
        }
        // Vivid: saturated and neither nearly black nor nearly white.
        let mid = 1.0 - ((l - 0.5).abs() * 2.0).powi(2);
        // Chroma, not HSL saturation: a pale sand counts as "saturated" there but is not vivid.
        let chroma = (p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])) as f32 / 255.0;
        let w = (chroma * chroma * mid.max(0.0)) as f64;
        colour += (chroma * mid.max(0.0)) as f64;
        if w > 0.02 {
            let slot = &mut sector[((h / 360.0 * SECTORS as f32) as usize).min(SECTORS - 1)];
            slot[0] += w;
            slot[1] += p[0] as f64 * w;
            slot[2] += p[1] as f64 * w;
            slot[3] += p[2] as f64 * w;
            slot[4] += s as f64 * w;
        }
    }
    let n = pixels.len().max(1) as f64;
    let tone = |acc: &[f64; 4], fallback: &[f64; 4]| {
        let acc = if acc[0] >= n * 0.02 { acc } else { fallback };
        if acc[0] <= 0.0 {
            return (0.0, 0.0);
        }
        let (h, s, _) = to_hsl(Rgb((acc[1] / acc[0]) as u8, (acc[2] / acc[0]) as u8, (acc[3] / acc[0]) as u8));
        (h, s)
    };
    let mut vivid: Vec<Vivid> = sector
        .iter()
        .filter(|s| s[0] > 0.0)
        .map(|s| {
            let color = Rgb((s[1] / s[0]) as u8, (s[2] / s[0]) as u8, (s[3] / s[0]) as u8);
            let (hue, _, _) = to_hsl(color);
            Vivid { color, hue, saturation: (s[4] / s[0]) as f32, weight: (s[0] / n) as f32 }
        })
        .collect();
    vivid.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    // Neighbouring sectors are one colour: keep the strongest of each group.
    let mut chosen: Vec<Vivid> = Vec::new();
    for v in vivid {
        if v.weight < 0.0015 {
            break;
        }
        if let Some(c) = chosen.iter_mut().find(|c| hue_distance(c.hue, v.hue) < 30.0) {
            c.weight += v.weight;
        } else if chosen.len() < 6 {
            chosen.push(v);
        }
    }
    chosen.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    Analysis { brightness: (brightness / n) as f32, dark_tone: tone(&dark, &all), light_tone: tone(&light, &all), vivid: chosen, colourfulness: (colour / n) as f32 }
}

/// The colours of a theme, with the keys of Omarchy's `colors.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct Scheme {
    pub light: bool,
    pub accent: Rgb,
    pub cursor: Rgb,
    pub foreground: Rgb,
    pub background: Rgb,
    pub selection_foreground: Rgb,
    pub selection_background: Rgb,
    /// Between red and yellow; Omarchy's palette names it.
    pub orange: Rgb,
    /// color0..color15: black, red, green, yellow, blue, magenta, cyan, white, then the bright ones.
    pub colors: [Rgb; 16],
}

/// How much an accent must stand out from the background. It colours borders, selections
/// and marks rather than body text, and on a light background a stricter rule turns every
/// orange into brown.
fn accent_contrast(light: bool) -> f32 {
    if light { 3.0 } else { 4.5 }
}

fn as_accent(color: Rgb, background: Rgb, light: bool) -> Rgb {
    let (h, s, l) = to_hsl(color);
    // Not washed out, not neon: the hue with a saturation that carries.
    readable(from_hsl(h, s.clamp(0.45, 0.9), if light { l.clamp(0.36, 0.52) } else { l.clamp(0.55, 0.75) }), background, accent_contrast(light))
}

/// The accents a picture offers: its vivid colours, made readable on the background the
/// scheme would get, then colours that go with the strongest one (its neighbours and its
/// opposite) so that there is always a choice. A grey picture offers quiet tones.
pub fn accents(analysis: &Analysis, light: bool) -> Vec<Rgb> {
    let background = scheme(analysis, light, None).background;
    let mut out: Vec<Rgb> = Vec::new();
    let mut hues: Vec<f32> = Vec::new();
    let mut offer = |color: Rgb, out: &mut Vec<Rgb>| {
        let accent = as_accent(color, background, light);
        let hue = to_hsl(accent).0;
        if hues.iter().all(|h| hue_distance(*h, hue) >= 22.0) && out.len() < 6 {
            hues.push(hue);
            out.push(accent);
        }
    };
    for v in &analysis.vivid {
        offer(v.color, &mut out);
    }
    let (base_hue, base_sat) = match analysis.vivid.first() {
        Some(v) => (v.hue, v.saturation.clamp(0.45, 0.8)),
        None => (if analysis.colourfulness < 0.01 { 215.0 } else if light { analysis.light_tone.0 } else { analysis.dark_tone.0 }, 0.35),
    };
    if out.is_empty() {
        offer(from_hsl(base_hue, base_sat, 0.6), &mut out);
    }
    for turn in [180.0, 35.0, -35.0, 120.0] {
        if out.len() >= 5 {
            break;
        }
        offer(from_hsl(base_hue + turn, base_sat, 0.6), &mut out);
    }
    out
}

/// Build the scheme. `accent` is one of `accents` (or any colour); None takes the strongest.
pub fn scheme(analysis: &Analysis, light: bool, accent: Option<Rgb>) -> Scheme {
    let (tone_hue, tone_sat) = if light { analysis.light_tone } else { analysis.dark_tone };
    // A grey picture gets a neutral background, not a tinted one.
    let tint = if analysis.colourfulness < 0.01 { 0.0 } else { tone_sat };
    let background = if light { from_hsl(tone_hue, tint.min(0.45), 0.955) } else { from_hsl(tone_hue, tint.min(0.32), 0.105) };
    let foreground = if light { from_hsl(tone_hue, tint.min(0.25), 0.24) } else { from_hsl(tone_hue, tint.min(0.22), 0.84) };
    let grey = |l: f32| from_hsl(tone_hue, tint.min(0.18), l);
    // How saturated the terminal colours are follows the picture: a muted photo, muted colours.
    let strength = if analysis.vivid.is_empty() { 0.42 } else { (analysis.vivid.iter().map(|v| v.saturation * v.weight).sum::<f32>() / analysis.vivid.iter().map(|v| v.weight).sum::<f32>().max(1e-6)).clamp(0.38, 0.78) };
    let base_l = if light { 0.40 } else { 0.68 };
    let named = |target: f32| {
        // The picture's own shade of this colour, when it has one near enough.
        let own = analysis.vivid.iter().filter(|v| hue_distance(v.hue, target) <= 32.0).max_by(|a, b| a.weight.total_cmp(&b.weight));
        let normal = match own {
            Some(v) => from_hsl(v.hue, ((v.saturation + strength) / 2.0).clamp(0.35, 0.85), base_l),
            // A colour the picture does not have: quieter, and leaning a little towards the
            // picture's main colour so that it belongs to the others.
            None => {
                let plain = from_hsl(target, (strength * 0.74).clamp(0.3, 0.7), base_l);
                match analysis.vivid.first() {
                    Some(main) => plain.mix(from_hsl(main.hue, main.saturation.clamp(0.3, 0.8), base_l), 0.14),
                    None => plain,
                }
            }
        };
        let normal = readable(normal, background, 4.0);
        let (h, s, l) = to_hsl(normal);
        let bright = from_hsl(h, (s + 0.08).min(0.95), if light { l - 0.07 } else { l + 0.08 });
        (normal, bright)
    };
    let (red, green, yellow, blue, magenta, cyan) = (named(2.0), named(125.0), named(45.0), named(218.0), named(300.0), named(182.0));
    let orange = named(24.0).0;
    let colors = [
        if light { grey(0.84) } else { grey(0.20) },
        red.0,
        green.0,
        yellow.0,
        blue.0,
        magenta.0,
        cyan.0,
        if light { grey(0.36) } else { grey(0.74) },
        if light { grey(0.66) } else { grey(0.40) },
        red.1,
        green.1,
        yellow.1,
        blue.1,
        magenta.1,
        cyan.1,
        if light { grey(0.22) } else { grey(0.92) },
    ];
    let accent = match accent {
        Some(a) => readable(a, background, 3.0),
        None => match analysis.vivid.first() {
            Some(v) => as_accent(v.color, background, light),
            None => as_accent(from_hsl(if analysis.colourfulness < 0.01 { 215.0 } else { tone_hue }, 0.35, 0.6), background, light),
        },
    };
    // A selection is a quiet tint of the accent, as in Omarchy's own themes, with the
    // brightest text colour on it.
    let selection_background = background.mix(accent, 0.24);
    let selection_foreground = readable(colors[15], selection_background, 7.0);
    Scheme { light, accent, cursor: colors[15], foreground, background, selection_foreground, selection_background, orange, colors }
}

impl Scheme {
    /// The theme's `colors.toml`. First the palette Omarchy 4 names (`mode`, `accent`,
    /// `background`, `red`, …), then the keys older releases read (`cursor`, the selection
    /// pair and `color0` … `color15`), so that one file serves both: Omarchy 4 takes the
    /// named colours, an older one finds every key its templates ask for.
    pub fn colors_toml(&self) -> String {
        let c = &self.colors;
        let (black, white) = (Rgb(0, 0, 0), Rgb(255, 255, 255));
        let shade = |amount: f32| self.background.mix(black, amount);
        let mut out = format!("mode = \"{}\"\n\n", if self.light { "light" } else { "dark" });
        let groups: [&[(&str, Rgb)]; 5] = [
            &[("accent", self.accent), ("selection", self.selection_background), ("muted", c[8])],
            &[("background", self.background), ("dark_background", shade(if self.light { 0.05 } else { 0.25 })), ("darker_background", shade(if self.light { 0.1 } else { 0.5 })), ("lighter_background", c[0])],
            &[("foreground", self.foreground), ("dark_foreground", self.foreground.mix(self.background, 0.45)), ("light_foreground", self.foreground.mix(if self.light { black } else { white }, 0.12)), ("bright_foreground", c[15])],
            &[("red", c[1]), ("yellow", c[3]), ("orange", self.orange), ("green", c[2]), ("cyan", c[6]), ("blue", c[4]), ("magenta", c[5]), ("brown", self.orange.mix(black, 0.5))],
            &[("bright_red", c[9]), ("bright_yellow", c[11]), ("bright_green", c[10]), ("bright_cyan", c[14]), ("bright_blue", c[12]), ("bright_magenta", c[13])],
        ];
        for group in groups {
            for (key, value) in group {
                out.push_str(&format!("{key} = \"{}\"\n", value.hex()));
            }
            out.push('\n');
        }
        out.push_str("# For Omarchy releases before 4: the same colours under the names they read.\n");
        for (key, value) in [("cursor", self.cursor), ("selection_foreground", self.selection_foreground), ("selection_background", self.selection_background)] {
            out.push_str(&format!("{key} = \"{}\"\n", value.hex()));
        }
        for (i, color) in c.iter().enumerate() {
            out.push_str(&format!("color{i} = \"{}\"\n", color.hex()));
        }
        out
    }

    /// The scheme a `colors.toml` holds, in either form: the named palette of Omarchy 4 or
    /// the `color0` … `color15` of older releases. None when the base colours are missing.
    pub fn from_colors_toml(text: &str) -> Option<Scheme> {
        let (colors, strings) = crate::theme::parse_colors(text);
        let get = |key: &str| colors.get(key).copied();
        let either = |a: &str, b: &str| get(a).or_else(|| get(b));
        let (background, foreground) = (get("background")?, get("foreground")?);
        let accent = get("accent").or_else(|| either("blue", "color4"))?;
        let light = match strings.get("mode").or_else(|| strings.get("theme_type")).map(String::as_str) {
            Some("light") => true,
            Some("dark") => false,
            _ => background.luminance() > 0.6,
        };
        let names = ["lighter_background", "red", "green", "yellow", "blue", "magenta", "cyan", "light_foreground", "muted", "bright_red", "bright_green", "bright_yellow", "bright_blue", "bright_magenta", "bright_cyan", "bright_foreground"];
        let mut sixteen = [foreground; 16];
        for (i, slot) in sixteen.iter_mut().enumerate() {
            let own = either(&format!("color{i}"), names[i]);
            *slot = own.unwrap_or(match i {
                0 | 8 => background.mix(foreground, if i == 0 { 0.18 } else { 0.4 }),
                // A bright colour that is not given is its normal one.
                9..=14 => either(&format!("color{}", i - 8), names[i - 8]).unwrap_or(foreground),
                _ => foreground,
            });
        }
        let orange = get("orange").unwrap_or_else(|| sixteen[1].mix(sixteen[3], 0.5));
        let selection_background = either("selection_background", "selection").unwrap_or_else(|| background.mix(accent, 0.24));
        Some(Scheme {
            light,
            accent,
            cursor: either("cursor", "bright_foreground").unwrap_or(sixteen[15]),
            foreground,
            background,
            selection_foreground: either("selection_foreground", "bright_foreground").unwrap_or(sixteen[15]),
            selection_background,
            orange,
            colors: sixteen,
        })
    }
}

/// The icon set whose folders are nearest to an accent, out of the ones Omarchy's manual
/// names for a theme's `icons.theme`. Plain "Yaru" is the orange one.
pub fn icon_theme_for(accent: Rgb) -> &'static str {
    let (h, s, _) = to_hsl(accent);
    if s < 0.12 {
        return "Yaru-dark";
    }
    match h as u32 {
        0..=14 | 346..=360 => "Yaru-red",
        15..=27 => "Yaru",
        28..=40 => "Yaru-wartybrown",
        41..=68 => "Yaru-yellow",
        69..=95 => "Yaru-olive",
        96..=160 => "Yaru-sage",
        161..=200 => "Yaru-prussiangreen",
        201..=252 => "Yaru-blue",
        253..=292 => "Yaru-purple",
        _ => "Yaru-magenta",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(parts: &[([u8; 3], usize)]) -> Vec<[u8; 3]> {
        parts.iter().flat_map(|(c, n)| std::iter::repeat_n(*c, *n)).collect()
    }

    #[test]
    fn colours_convert_both_ways() {
        for c in [Rgb(122, 162, 247), Rgb(26, 27, 38), Rgb(255, 0, 0), Rgb(0, 128, 64), Rgb(250, 244, 237), Rgb(128, 128, 128)] {
            let (h, s, l) = to_hsl(c);
            let back = from_hsl(h, s, l);
            assert!((back.0 as i32 - c.0 as i32).abs() <= 1 && (back.1 as i32 - c.1 as i32).abs() <= 1 && (back.2 as i32 - c.2 as i32).abs() <= 1, "{c:?} -> {back:?}");
        }
        assert!((contrast(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 0.01);
        assert!((contrast(Rgb(90, 90, 90), Rgb(90, 90, 90)) - 1.0).abs() < 0.01);
        let lifted = readable(Rgb(40, 40, 90), Rgb(20, 20, 30), 4.5);
        assert!(contrast(lifted, Rgb(20, 20, 30)) >= 4.5);
        let (h0, _, _) = to_hsl(Rgb(40, 40, 90));
        assert!(hue_distance(to_hsl(lifted).0, h0) < 3.0, "the hue stays");
    }

    #[test]
    fn a_night_picture_gives_a_dark_scheme_in_its_colours() {
        // Deep blue night, an orange lantern, a little teal.
        let pixels = picture(&[([14, 20, 44], 700), ([28, 40, 80], 200), ([240, 140, 40], 70), ([40, 170, 160], 30)]);
        let a = analyse(&pixels);
        assert!(!a.suggests_light());
        assert!(hue_distance(a.dark_tone.0, 225.0) < 20.0, "the dark areas are blue: {:?}", a.dark_tone);
        assert!(hue_distance(a.vivid[0].hue, 30.0) < 15.0 || hue_distance(a.vivid[0].hue, 225.0) < 20.0, "{:?}", a.vivid);
        let s = scheme(&a, false, None);
        assert!(!s.light && s.background.luminance() < 0.15);
        assert!(hue_distance(to_hsl(s.background).0, 225.0) < 20.0, "the background carries the picture's tone");
        assert!(contrast(s.foreground, s.background) >= 7.0);
        assert!(contrast(s.accent, s.background) >= 4.5);
        for (i, c) in s.colors.iter().enumerate().filter(|(i, _)| ![0, 8].contains(i)) {
            assert!(contrast(*c, s.background) >= 3.9, "color{i} {c:?} on {:?}", s.background);
        }
        // Red stays red, green stays green, whatever the picture.
        assert!(hue_distance(to_hsl(s.colors[1]).0, 0.0) < 35.0 && hue_distance(to_hsl(s.colors[2]).0, 125.0) < 35.0 && hue_distance(to_hsl(s.colors[4]).0, 218.0) < 35.0);
        // Yellow leans to the lantern's orange; cyan to the picture's teal.
        assert!(hue_distance(to_hsl(s.colors[3]).0, 30.0) < 12.0, "{:?}", to_hsl(s.colors[3]));
        assert!(hue_distance(to_hsl(s.colors[6]).0, 175.0) < 10.0);
        // The orange is offered as an accent and can be chosen.
        let offered = accents(&a, false);
        let orange = offered.iter().copied().find(|c| hue_distance(to_hsl(*c).0, 30.0) < 15.0).expect("orange is offered");
        assert_eq!(scheme(&a, false, Some(orange)).accent, orange);
        assert!(offered.iter().all(|c| contrast(*c, s.background) >= 4.5));
        assert!(offered.len() >= 3, "there is always a choice: {offered:?}");
        for (i, a) in offered.iter().enumerate() {
            assert!(offered.iter().skip(i + 1).all(|b| hue_distance(to_hsl(*a).0, to_hsl(*b).0) >= 20.0), "no two alike");
        }
        assert_eq!(offered[0], s.accent, "the first one offered is the one taken when nothing is chosen");
    }

    #[test]
    fn a_bright_picture_gives_a_light_scheme_and_a_grey_one_stays_neutral() {
        let beach = picture(&[([245, 235, 215], 600), ([230, 210, 180], 250), ([30, 120, 200], 150)]);
        let a = analyse(&beach);
        assert!(a.suggests_light());
        let s = scheme(&a, true, None);
        assert!(s.light && s.background.luminance() > 0.9);
        assert!(contrast(s.foreground, s.background) >= 7.0 && contrast(s.accent, s.background) >= 3.0);
        assert!(hue_distance(to_hsl(s.accent).0, 208.0) < 15.0, "the sea is the accent");
        // No green in the picture: the terminal's green is still green, but quieter than the sea.
        assert!(hue_distance(to_hsl(s.colors[2]).0, 125.0) < 35.0 && to_hsl(s.colors[2]).1 < to_hsl(s.colors[4]).1);
        for (i, c) in s.colors.iter().enumerate().filter(|(i, _)| ![0, 8].contains(i)) {
            assert!(contrast(*c, s.background) >= 3.9, "color{i}");
        }
        // The same picture as a dark scheme, when asked for.
        assert!(scheme(&a, false, None).background.luminance() < 0.15);

        let grey = picture(&[([30, 30, 30], 500), ([200, 200, 200], 500)]);
        let g = analyse(&grey);
        assert!(g.vivid.is_empty() && g.colourfulness < 0.01);
        let gs = scheme(&g, false, None);
        assert!(to_hsl(gs.background).1 < 0.02, "no tint from nowhere");
        assert!(accents(&g, false).len() >= 3 && accents(&g, false)[0] == gs.accent);
        assert!(contrast(gs.accent, gs.background) >= 4.5);
        assert_eq!(analyse(&[]).vivid.len(), 0, "an empty picture does not panic");
    }

    #[test]
    fn the_scheme_is_written_the_way_omarchy_reads_it() {
        let a = analyse(&picture(&[([14, 20, 44], 800), ([240, 140, 40], 200)]));
        let s = scheme(&a, false, None);
        let text = s.colors_toml();
        assert!(text.starts_with("mode = \"dark\"\n"));
        // Every name Omarchy 4's own themes define, and every key older templates ask for.
        let named = ["accent", "selection", "muted", "background", "dark_background", "darker_background", "lighter_background", "foreground", "dark_foreground", "light_foreground", "bright_foreground", "red", "yellow", "orange", "green", "cyan", "blue", "magenta", "brown", "bright_red", "bright_yellow", "bright_green", "bright_cyan", "bright_blue", "bright_magenta"];
        let legacy = ["cursor", "selection_foreground", "selection_background", "color0", "color7", "color15"];
        for key in named.iter().chain(legacy.iter()) {
            assert!(text.lines().any(|l| l.starts_with(&format!("{key} = \"#")) && l.len() == key.len() + 12), "{key}");
        }
        assert_eq!(text.lines().filter(|l| l.starts_with("color")).count(), 16);
        // Only characters Omarchy's parser accepts in a value, and no key twice.
        let keys: Vec<&str> = text.lines().filter_map(|l| l.split_once(" = ").map(|(k, _)| k)).collect();
        let mut unique = keys.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(keys.len(), unique.len());
        assert_eq!(Scheme::from_colors_toml(&text), Some(s.clone()), "what is written is read back the same");
        assert!(contrast(s.selection_foreground, s.selection_background) >= 7.0, "text on a selection can be read");
        let light = scheme(&a, true, None);
        assert!(light.colors_toml().starts_with("mode = \"light\"\n"));
        assert_eq!(Scheme::from_colors_toml(&light.colors_toml()), Some(light));

        // Tokyo Night as Omarchy 4 ships it: named colours, no colorN.
        let stock = "mode = \"dark\"\naccent = \"#7aa2f7\"\nselection = \"#292e42\"\nmuted = \"#414868\"\nbackground = \"#1a1b26\"\nlighter_background = \"#24283b\"\nforeground = \"#a9b1d6\"\nbright_foreground = \"#c0caf5\"\nred = \"#f7768e\"\ngreen = \"#9ece6a\"\norange = \"#eb927b\"\nbright_red = \"#ff7a93\"\n";
        let tokyo = Scheme::from_colors_toml(stock).unwrap();
        assert_eq!((tokyo.accent, tokyo.background, tokyo.light), (Rgb(0x7a, 0xa2, 0xf7), Rgb(0x1a, 0x1b, 0x26), false));
        assert_eq!((tokyo.colors[1], tokyo.colors[2], tokyo.colors[8], tokyo.colors[9], tokyo.colors[15]), (Rgb(0xf7, 0x76, 0x8e), Rgb(0x9e, 0xce, 0x6a), Rgb(0x41, 0x48, 0x68), Rgb(0xff, 0x7a, 0x93), Rgb(0xc0, 0xca, 0xf5)));
        assert_eq!((tokyo.selection_background, tokyo.orange, tokyo.colors[0]), (Rgb(0x29, 0x2e, 0x42), Rgb(0xeb, 0x92, 0x7b), Rgb(0x24, 0x28, 0x3b)));
        assert_eq!(tokyo.colors[10], tokyo.colors[2], "a bright colour that is not given is its normal one");
        // The same theme the way a release before 4 wrote it.
        let old = "accent = \"#7aa2f7\"\ncursor = \"#c0caf5\"\nforeground = \"#a9b1d6\"\nbackground = \"#1a1b26\"\nselection_foreground = \"#c0caf5\"\nselection_background = \"#7aa2f7\"\ncolor0 = \"#32344a\"\ncolor1 = \"#f7768e\"\n";
        let before = Scheme::from_colors_toml(old).unwrap();
        assert_eq!((before.accent, before.colors[0], before.colors[1], before.selection_background), (Rgb(0x7a, 0xa2, 0xf7), Rgb(0x32, 0x34, 0x4a), Rgb(0xf7, 0x76, 0x8e), Rgb(0x7a, 0xa2, 0xf7)));
        assert!(Scheme::from_colors_toml("mode = \"light\"\nbackground = \"#101010\"\nforeground = \"#eeeeee\"\naccent = \"#ff0000\"\n").unwrap().light, "the mode key decides, not the brightness");
        assert_eq!(Scheme::from_colors_toml("nothing = 1\n"), None);
        // Icon sets: only names from Omarchy's own list.
        let offered = ["Yaru", "Yaru-blue", "Yaru-dark", "Yaru-magenta", "Yaru-olive", "Yaru-prussiangreen", "Yaru-purple", "Yaru-red", "Yaru-sage", "Yaru-wartybrown", "Yaru-yellow"];
        for hue in (0..360).step_by(5) {
            assert!(offered.contains(&icon_theme_for(from_hsl(hue as f32, 0.6, 0.6))), "hue {hue}");
        }
        assert_eq!(icon_theme_for(Rgb(0x7a, 0xa2, 0xf7)), "Yaru-blue");
        assert_eq!(icon_theme_for(Rgb(230, 60, 50)), "Yaru-red");
        assert_eq!(icon_theme_for(Rgb(120, 120, 124)), "Yaru-dark");
    }
}
