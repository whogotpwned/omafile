//! Spectrum for the Now Playing view. The real thing when possible: the
//! monitor of the default output is recorded with `parec` (or `pw-record`),
//! run through a small FFT and folded into log-spaced bands. Nothing leaves
//! the process and nothing is stored. Without a capture tool, or while the
//! monitor is silent, a procedural animation stands in.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const BANDS: usize = 44;
const RATE: f32 = 44_100.0;
const FFT_SIZE: usize = 2048;
const HOP: usize = 1024;

/// In-place radix-2 FFT; `re.len()` must be a power of two.
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    // Bit reversal.
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (angle.cos(), angle.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (i + k, i + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// Fold `samples` (mono, -1..1, at least FFT_SIZE of them) into BANDS values in 0..1.
pub fn bands_from(samples: &[f32]) -> [f32; BANDS] {
    let mut out = [0.0f32; BANDS];
    if samples.len() < FFT_SIZE {
        return out;
    }
    let start = samples.len() - FFT_SIZE;
    let mut re: Vec<f32> = (0..FFT_SIZE)
        .map(|i| {
            // Hann window.
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos();
            samples[start + i] * w
        })
        .collect();
    let mut im = vec![0.0f32; FFT_SIZE];
    fft(&mut re, &mut im);
    let (lo, hi) = (40.0f32, 16_000.0f32);
    let bin_hz = RATE / FFT_SIZE as f32;
    for (b, slot) in out.iter_mut().enumerate() {
        let f0 = lo * (hi / lo).powf(b as f32 / BANDS as f32);
        let f1 = lo * (hi / lo).powf((b + 1) as f32 / BANDS as f32);
        let i0 = ((f0 / bin_hz) as usize).max(1);
        let i1 = ((f1 / bin_hz) as usize).max(i0 + 1).min(FFT_SIZE / 2);
        let mut peak = 0.0f32;
        for i in i0..i1 {
            peak = peak.max((re[i] * re[i] + im[i] * im[i]).sqrt());
        }
        // Magnitude → dB → 0..1 over a 60 dB window, with a gentle treble lift.
        let mag = peak / (FFT_SIZE as f32 / 4.0);
        let db = 20.0 * (mag + 1e-9).log10() + 6.0 * (b as f32 / BANDS as f32);
        *slot = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    }
    out
}

/// Bars that look alive without any audio: layered sines, calm when paused.
pub fn procedural(t: f64, playing: bool) -> [f32; BANDS] {
    let mut out = [0.0f32; BANDS];
    for (i, slot) in out.iter_mut().enumerate() {
        let x = i as f64;
        if playing {
            let a = (t * 2.1 + x * 0.45).sin() * 0.5 + 0.5;
            let b = (t * 3.7 - x * 0.23).sin() * 0.5 + 0.5;
            let c = (t * 1.3 + x * 1.1).sin() * 0.5 + 0.5;
            let beat = ((t * 2.0 * std::f64::consts::PI * 1.9).sin().max(0.0)).powf(6.0);
            let tilt = 1.0 - (x / BANDS as f64) * 0.55;
            *slot = ((0.18 + 0.45 * a * b + 0.25 * c + 0.3 * beat * (1.0 - x / BANDS as f64)) * tilt).clamp(0.04, 1.0) as f32;
        } else {
            *slot = (0.05 + 0.03 * ((t * 0.8 + x * 0.3).sin() * 0.5 + 0.5)) as f32;
        }
    }
    out
}

/// The command that writes raw s16le mono 44.1 kHz of what the speakers play to stdout.
pub fn capture_command() -> Option<Vec<String>> {
    let has = |tool: &str| gtk::glib::find_program_in_path(tool).is_some();
    if has("parec") {
        Some(["parec", "--device=@DEFAULT_MONITOR@", "--format=s16le", "--rate=44100", "--channels=1", "--latency-msec=30", "--raw", "--client-name=omafile", "--stream-name=Now Playing spectrum"].iter().map(|s| s.to_string()).collect())
    } else if has("pw-record") {
        Some(["pw-record", "-P", "{ stream.capture.sink = true }", "--format=s16", "--rate=44100", "--channels=1", "--latency=30ms", "--raw", "-"].iter().map(|s| s.to_string()).collect())
    } else {
        None
    }
}

struct Shared {
    bands: [f32; BANDS],
    at: Instant,
    /// Last time the monitor carried any signal.
    loud_at: Option<Instant>,
}

/// A running capture. Dropping it stops the recorder.
pub struct Spectrum {
    shared: Arc<Mutex<Shared>>,
    child: Mutex<Option<Child>>,
    stop: Arc<AtomicBool>,
}

impl Spectrum {
    pub fn start() -> Option<Spectrum> {
        let argv = capture_command()?;
        let mut child = Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let mut stdout = child.stdout.take()?;
        let shared = Arc::new(Mutex::new(Shared { bands: [0.0; BANDS], at: Instant::now(), loud_at: None }));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared2, stop2) = (shared.clone(), stop.clone());
        std::thread::Builder::new().name("omafile-spectrum".into()).spawn(move || {
            let mut ring: Vec<f32> = Vec::with_capacity(FFT_SIZE * 2);
            let mut buf = vec![0u8; HOP * 2];
            let mut first = true;
            while !stop2.load(Ordering::Relaxed) {
                if stdout.read_exact(&mut buf).is_err() {
                    break;
                }
                // pw-record may prepend a WAV header: skip one hop if so.
                if std::mem::take(&mut first) && buf.starts_with(b"RIFF") {
                    continue;
                }
                let mut energy = 0.0f32;
                for pair in buf.as_chunks::<2>().0 {
                    let s = i16::from_le_bytes(*pair) as f32 / 32768.0;
                    energy += s * s;
                    ring.push(s);
                }
                if ring.len() > FFT_SIZE {
                    let extra = ring.len() - FFT_SIZE;
                    ring.drain(..extra);
                }
                let bands = bands_from(&ring);
                if let Ok(mut s) = shared2.lock() {
                    s.bands = bands;
                    s.at = Instant::now();
                    if energy / HOP as f32 > 1e-7 {
                        s.loud_at = Some(Instant::now());
                    }
                }
            }
        }).ok()?;
        Some(Spectrum { shared, child: Mutex::new(Some(child)), stop })
    }

    /// Fresh bands from the capture, or None when it is stale or has been silent for a while.
    pub fn latest(&self) -> Option<[f32; BANDS]> {
        let s = self.shared.lock().ok()?;
        let fresh = s.at.elapsed().as_millis() < 400;
        let audible = s.loud_at.map(|t| t.elapsed().as_millis() < 1500).unwrap_or(false);
        (fresh && audible).then_some(s.bands)
    }
}

impl Drop for Spectrum {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut c) = self.child.lock() {
            if let Some(mut child) = c.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_finds_a_sine() {
        let n = 1024;
        let mut re: Vec<f32> = (0..n).map(|i| (2.0 * std::f32::consts::PI * 64.0 * i as f32 / n as f32).sin()).collect();
        let mut im = vec![0.0; n];
        fft(&mut re, &mut im);
        let mags: Vec<f32> = (0..n / 2).map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt()).collect();
        let peak = mags.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).unwrap().0;
        assert_eq!(peak, 64);
        assert!((mags[64] - n as f32 / 2.0).abs() < 1.0);
    }

    #[test]
    fn bands_follow_the_pitch() {
        let tone = |hz: f32| -> Vec<f32> { (0..FFT_SIZE).map(|i| 0.8 * (2.0 * std::f32::consts::PI * hz * i as f32 / RATE).sin()).collect() };
        let peak = |b: [f32; BANDS]| b.iter().enumerate().max_by(|a, c| a.1.partial_cmp(c.1).unwrap()).unwrap().0;
        let low = peak(bands_from(&tone(110.0)));
        let high = peak(bands_from(&tone(4000.0)));
        assert!(low < high, "{low} < {high}");
        assert!(bands_from(&tone(440.0)).iter().cloned().fold(0.0, f32::max) > 0.6);
        assert_eq!(bands_from(&vec![0.0; FFT_SIZE]), [0.0; BANDS]);
        assert_eq!(bands_from(&[0.0; 10]), [0.0; BANDS]);
    }

    #[test]
    fn procedural_bars_stay_in_range() {
        for t in [0.0, 0.7, 12.3] {
            assert!(procedural(t, true).iter().all(|v| (0.0..=1.0).contains(v)));
            assert!(procedural(t, false).iter().all(|v| *v < 0.2));
        }
    }
}
