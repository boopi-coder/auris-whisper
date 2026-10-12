//! Проверка голосового ввода без микрофона: запись «звучит» в реальном времени,
//! нарезка на фразы и расшифровка — те же, что в приложении. Печатает, когда
//! какая фраза была бы вставлена и сколько она ждала после конца речи.
//!
//!   cargo run --release --example dictation -- <модель.bin | папка модели> <аудио> [язык]

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use auris_whisper_lib::dictation::{preview_allowed, Decision, Streamer};
use auris_whisper_lib::{audio, engine, models::Backend};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    whisper_rs::install_logging_hooks();
    let model = Path::new(&args[1]);
    let backend = if model.join("encoder.int8.onnx").is_file() {
        Backend::Gigaam
    } else if model.join("model.onnx").is_file() {
        Backend::Tone
    } else {
        Backend::Whisper
    };
    let lang = args.get(3).cloned();
    let samples = audio::decode_file(Path::new(&args[2])).expect("decode");
    let vad = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ggml-silero-v5.1.2.bin");
    let mut eng = engine::Engine::new(Some(vad.clone()), Arc::new(AtomicBool::new(false)));
    eng.load(model, backend, true).map_err(|e| e.message()).expect("load");
    let mut streamer = Streamer::new(Some(&vad));

    let rate = audio::SAMPLE_RATE as usize;
    let tick = Duration::from_millis(150);
    let start = Instant::now();
    let (mut committed, mut speed, mut previews) = (0usize, None::<f64>, 0usize);
    let mut last_preview = Instant::now();
    let recognize = |eng: &mut engine::Engine, a: &[f32], speed: &mut Option<f64>| {
        let t = Instant::now();
        let segs = eng.transcribe(a, lang.as_deref(), false, &mut |_| {}).ok()?;
        *speed = Some(a.len() as f64 / rate as f64 / t.elapsed().as_secs_f64().max(0.001));
        Some(engine::plain_text(&segs).trim().to_string())
    };
    loop {
        std::thread::sleep(tick);
        // «Записано» столько, сколько прошло времени.
        let now = ((start.elapsed().as_secs_f64() * rate as f64) as usize).min(samples.len());
        let pending = &samples[committed..now];
        match streamer.decide(pending) {
            Decision::Commit { samples: n, speech } => {
                if speech {
                    let text = recognize(&mut eng, &pending[..n], &mut speed).unwrap_or_default();
                    let spoken_end = (committed + n) as f64 / rate as f64;
                    println!(
                        "[{:6.2} c] вставка (фраза до {:6.2} c, ждала {:.2} c, ×{:.0}): {text}",
                        start.elapsed().as_secs_f64(),
                        spoken_end,
                        start.elapsed().as_secs_f64() - spoken_end,
                        speed.unwrap_or(0.0)
                    );
                }
                committed += n;
            }
            Decision::Drop(n) => committed += n,
            Decision::Wait(sp) => {
                if preview_allowed(speed) && sp >= 0.35 && last_preview.elapsed() >= Duration::from_millis(700) {
                    let _ = recognize(&mut eng, pending, &mut speed);
                    previews += 1;
                    last_preview = Instant::now();
                }
            }
        }
        if now >= samples.len() {
            // «Нажали клавишу ещё раз».
            let tail = &samples[committed..];
            if streamer.has_speech(tail) {
                let text = recognize(&mut eng, tail, &mut speed).unwrap_or_default();
                println!("[{:6.2} c] хвост: {text}", start.elapsed().as_secs_f64());
            }
            break;
        }
    }
    println!("предпросмотров: {previews}");
}
