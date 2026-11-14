//! Captura de micrófono con cpal + detector de voz (VAD) por energía.
//!
//! Produce "frases" (audio mono a 16 kHz) separadas por silencios. Solo se
//! transcribe cuando hay voz, así whisper no trabaja en vacío (batería).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub const TARGET_RATE: u32 = 16_000;
const FRAME_MS: usize = 30;
const FRAME: usize = TARGET_RATE as usize * FRAME_MS / 1000; // 480 muestras
const END_SILENCE_FRAMES: usize = 800 / FRAME_MS; // 0,8 s de silencio cierra la frase
const MIN_SPEECH_FRAMES: usize = 300 / FRAME_MS;
const MAX_UTTERANCE_FRAMES: usize = 15_000 / FRAME_MS;
const PREROLL_FRAMES: usize = 300 / FRAME_MS;

/// VAD por energía con suelo de ruido adaptativo.
pub struct Vad {
    noise: f32,
    in_speech: bool,
    silence: usize,
    speech_frames: usize,
    current: Vec<f32>,
    preroll: std::collections::VecDeque<Vec<f32>>,
    pending: Vec<f32>,
}

impl Default for Vad {
    fn default() -> Self {
        Self {
            noise: 0.003,
            in_speech: false,
            silence: 0,
            speech_frames: 0,
            current: Vec::new(),
            preroll: std::collections::VecDeque::new(),
            pending: Vec::new(),
        }
    }
}

impl Vad {
    pub fn reset(&mut self) {
        *self = Self {
            noise: self.noise,
            ..Self::default()
        };
    }

    /// Añade audio a 16 kHz. Devuelve las frases completas detectadas.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.pending.extend_from_slice(samples);
        let mut out = Vec::new();
        while self.pending.len() >= FRAME {
            let frame: Vec<f32> = self.pending.drain(..FRAME).collect();
            if let Some(u) = self.frame(frame) {
                out.push(u);
            }
        }
        out
    }

    fn frame(&mut self, frame: Vec<f32>) -> Option<Vec<f32>> {
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        let threshold = (self.noise * 3.5).max(0.012);
        let voiced = rms > threshold;

        if !self.in_speech {
            // El suelo de ruido solo se adapta fuera de la voz.
            self.noise = self.noise * 0.95 + rms * 0.05;
            if voiced {
                self.in_speech = true;
                self.silence = 0;
                self.speech_frames = 1;
                self.current.clear();
                for f in self.preroll.drain(..) {
                    self.current.extend(f);
                }
                self.current.extend(frame);
            } else {
                self.preroll.push_back(frame);
                if self.preroll.len() > PREROLL_FRAMES {
                    self.preroll.pop_front();
                }
            }
            return None;
        }

        self.current.extend(frame);
        if voiced {
            self.silence = 0;
            self.speech_frames += 1;
        } else {
            self.silence += 1;
        }
        let total = self.current.len() / FRAME;
        if self.silence >= END_SILENCE_FRAMES || total >= MAX_UTTERANCE_FRAMES {
            self.in_speech = false;
            let utt = std::mem::take(&mut self.current);
            if self.speech_frames >= MIN_SPEECH_FRAMES {
                return Some(utt);
            }
        }
        None
    }
}

/// Remuestreo lineal sencillo a 16 kHz (suficiente para voz).
pub struct Resampler {
    ratio: f64,
    pos: f64,
}

impl Resampler {
    pub fn new(from: u32) -> Self {
        Self {
            ratio: from as f64 / TARGET_RATE as f64,
            pos: 0.0,
        }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if (self.ratio - 1.0).abs() < f64::EPSILON {
            return input.to_vec();
        }
        let mut out = Vec::with_capacity((input.len() as f64 / self.ratio) as usize + 1);
        while self.pos < input.len() as f64 {
            let i = self.pos.floor() as usize;
            let frac = (self.pos - i as f64) as f32;
            let a = input[i];
            let b = input.get(i + 1).copied().unwrap_or(a);
            out.push(a + (b - a) * frac);
            self.pos += self.ratio;
        }
        self.pos -= input.len() as f64;
        out
    }
}

/// Hilo de captura: mantiene el stream de cpal (que no es Send) y entrega frases.
/// Termina cuando `running` pasa a false.
pub fn spawn_capture(
    running: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    utterances: Sender<Vec<f32>>,
) -> Receiver<Result<(), String>> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("nova-mic".into())
        .spawn(move || {
            let (raw_tx, raw_rx) = std::sync::mpsc::channel::<Vec<f32>>();
            let (stream, rate) = match open_stream(raw_tx) {
                Ok(v) => v,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("{e:#}")));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            let mut resampler = Resampler::new(rate);
            let mut vad = Vad::default();
            while running.load(Ordering::Relaxed) {
                let Ok(chunk) = raw_rx.recv_timeout(std::time::Duration::from_millis(200)) else {
                    continue;
                };
                if muted.load(Ordering::Relaxed) {
                    vad.reset();
                    continue;
                }
                let resampled = resampler.process(&chunk);
                for u in vad.push(&resampled) {
                    let _ = utterances.send(u);
                }
            }
            drop(stream); // libera el micrófono
        })
        .expect("hilo de micrófono");
    ready_rx
}

fn open_stream(tx: Sender<Vec<f32>>) -> Result<(cpal::Stream, u32)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no se encontró ningún micrófono"))?;
    let config = device.default_input_config()?;
    let rate = config.sample_rate();
    let channels = config.channels() as usize;
    let err_fn = |e| log::error!("micrófono: {e}");

    macro_rules! build {
        ($t:ty, $conv:expr) => {{
            let tx = tx.clone();
            device.build_input_stream(
                config.clone().into(),
                move |data: &[$t], _: &cpal::InputCallbackInfo| {
                    let mono: Vec<f32> = data
                        .chunks(channels)
                        .map(|c| c.iter().map(|s| $conv(*s)).sum::<f32>() / channels as f32)
                        .collect();
                    let _ = tx.send(mono);
                },
                err_fn,
                None,
            )?
        }};
    }

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build!(f32, |s: f32| s),
        cpal::SampleFormat::I16 => build!(i16, |s: i16| s as f32 / i16::MAX as f32),
        cpal::SampleFormat::U16 => build!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0),
        cpal::SampleFormat::I32 => build!(i32, |s: i32| s as f32 / i32::MAX as f32),
        other => return Err(anyhow!("formato de micrófono no soportado: {other:?}")),
    };
    stream.play()?;
    Ok((stream, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vad_detects_one_utterance() {
        let mut vad = Vad::default();
        let silence = vec![0.0f32; TARGET_RATE as usize];
        let tone: Vec<f32> = (0..TARGET_RATE as usize)
            .map(|i| (i as f32 * 0.05).sin() * 0.2)
            .collect();
        let mut out = vad.push(&silence);
        out.extend(vad.push(&tone));
        out.extend(vad.push(&silence));
        assert_eq!(out.len(), 1);
        assert!(out[0].len() >= TARGET_RATE as usize);
    }

    #[test]
    fn resampler_ratio() {
        let mut r = Resampler::new(48_000);
        let out = r.process(&vec![0.1; 48_000]);
        assert!((out.len() as i64 - 16_000).abs() <= 2);
    }
}
