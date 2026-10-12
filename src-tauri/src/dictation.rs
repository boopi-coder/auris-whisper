//! Голосовой ввод в любую программу: горячая клавиша — запись, ещё раз — конец.
//!
//! Речь режется на фразы по паузам (детектор Silero). Как только фраза
//! закончилась, её расшифровывает выбранная модель, и текст вставляется туда,
//! где стоит курсор, — не дожидаясь конца диктовки. На быстром компьютере
//! индикатор в углу экрана ещё и показывает недоговорённую фразу на лету.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Size, WebviewUrl, WebviewWindowBuilder};

use crate::audio::SAMPLE_RATE;
use crate::engine::{plain_text, Engine, EngineError};
use crate::i18n::tr;
use crate::models::{self, Backend};
use crate::recorder::{self, Recording};
use crate::settings::Settings;
use crate::{insert, ru_asr, vad, AppState};

pub const OVERLAY: &str = "overlay";

/// Пауза такой длины — фраза закончилась, пора её расшифровать и вставить.
const PHRASE_PAUSE: f64 = 0.7;
/// Фраза длиннее — режем на ближайшей паузе, чтобы текст не ждал слишком долго.
const LONG_PHRASE: f64 = 15.0;
/// А без пауз — режем принудительно.
const MAX_PHRASE: f64 = 28.0;
/// Короче этого речь не расшифровываем: кашель, щелчок.
const MIN_SPEECH: f64 = 0.35;
/// Предпросмотр на лету — если модель считает хотя бы в столько раз быстрее речи.
const PREVIEW_SPEED: f64 = 4.0;
const PREVIEW_EVERY: Duration = Duration::from_millis(700);
const TICK: Duration = Duration::from_millis(150);

/// Решение по недорасшифрованному звуку.
#[derive(Debug, PartialEq)]
pub enum Decision {
    /// Фраза закончилась: первые столько отсчётов — расшифровать и вставить
    /// (`speech` — есть ли в них речь; если нет, просто пропустить).
    Commit { samples: usize, speech: bool },
    /// Долгая тишина — столько отсчётов можно выбросить.
    Drop(usize),
    /// Фраза ещё идёт; столько секунд речи в ней уже есть.
    Wait(f64),
}

/// Нарезка живой речи на фразы по паузам. Не знает ни про микрофон, ни про
/// модель — поэтому её можно прогнать по записи (см. examples/dictation.rs).
pub struct Streamer {
    detector: Option<vad::Detector>,
}

impl Streamer {
    pub fn new(vad_model: Option<&std::path::Path>) -> Self {
        Self { detector: vad_model.and_then(vad::Detector::new) }
    }

    /// `pending` — звук от конца последней вставленной фразы до текущего момента.
    pub fn decide(&mut self, pending: &[f32]) -> Decision {
        let rate = SAMPLE_RATE as f64;
        let len = pending.len() as f64 / rate;
        let Some(det) = self.detector.as_mut() else {
            // Без детектора пауз — просто куски по 10 секунд.
            return if len >= 10.0 { Decision::Commit { samples: pending.len(), speech: true } } else { Decision::Wait(len) };
        };
        let spans = det.spans(pending);
        if let Some(cut) = phrase_end(&spans, len) {
            let samples = ((cut * rate) as usize).min(pending.len());
            return Decision::Commit { samples, speech: speech(&spans, cut) >= MIN_SPEECH };
        }
        if spans.is_empty() && len > 3.0 {
            // Долгая тишина: держим только последнюю секунду.
            return Decision::Drop(pending.len() - SAMPLE_RATE as usize);
        }
        Decision::Wait(speech(&spans, len))
    }

    /// Есть ли речь в хвосте после того, как диктовку остановили.
    pub fn has_speech(&mut self, tail: &[f32]) -> bool {
        match self.detector.as_mut() {
            Some(det) => speech(&det.spans(tail), f64::MAX) >= MIN_SPEECH,
            None => tail.len() as f64 / SAMPLE_RATE as f64 >= MIN_SPEECH,
        }
    }
}

/// Предпросмотр недоговорённой фразы — если модель считает хотя бы во столько раз
/// быстрее речи (по последней фразе).
pub fn preview_allowed(speed: Option<f64>) -> bool {
    speed.is_some_and(|s| s >= PREVIEW_SPEED)
}

/// Что показывает индикатор.
#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    /// listening | loading | finishing | message
    pub state: &'static str,
    pub level: f32,
    pub seconds: f64,
    /// Недоговорённая фраза (предпросмотр) или последняя вставленная.
    pub text: String,
    pub message: String,
    pub en: bool,
}

fn show(app: &AppHandle, mut st: OverlayState) {
    st.en = crate::i18n::is_english();
    let _ = app.emit_to(OVERLAY, "dictation", st);
}

/// Одна диктовка: запись идёт, пока не нажмут клавишу ещё раз.
pub struct Session {
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

/// Горячая клавиша или пункт в трее: начать или закончить диктовку.
pub fn toggle(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut slot = state.dictation.lock().unwrap();
    if let Some(s) = slot.take() {
        if !s.finished.load(Ordering::Relaxed) {
            s.stop.store(true, Ordering::Relaxed);
            return;
        }
    }
    match start(app, &state) {
        Ok(s) => *slot = Some(s),
        Err(message) => flash(app, message),
    }
}

/// Короткое сообщение на индикаторе (например, «сначала выберите модель»).
fn flash(app: &AppHandle, message: String) {
    show_overlay(app);
    show(app, OverlayState { state: "message", message, ..Default::default() });
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(4));
        let busy = app.state::<AppState>().dictation.lock().unwrap().as_ref().is_some_and(|s| !s.finished.load(Ordering::Relaxed));
        if !busy {
            hide_overlay(&app);
        }
    });
}

fn start(app: &AppHandle, state: &AppState) -> Result<Session, String> {
    let settings = state.settings.lock().unwrap().clone();
    let model_id = settings.model.clone().ok_or_else(no_model)?;
    let model_path = models::model_path(&state.models_dir, &model_id).ok_or_else(no_model)?;
    let backend = models::backend_of(&model_id);
    if state.busy.load(Ordering::Relaxed) || state.recording.lock().unwrap().is_some() {
        return Err(tr(
            "Идёт расшифровка в окне приложения — голосовой ввод заработает, когда она закончится.",
            "A transcription is running in the app window — voice input will work once it finishes.",
        ));
    }
    if recorder::access_denied() {
        return Err(tr(
            "Windows запрещает доступ к микрофону: «Параметры → Конфиденциальность → Микрофон».",
            "Windows blocks the microphone: Settings → Privacy → Microphone.",
        ));
    }
    // macOS: заранее попросить разрешение вставлять текст (покажется один раз).
    insert::request_access();

    let level_app = app.clone();
    let rec = Recording::start(settings.mic.clone(), move |l| {
        show(&level_app, OverlayState { state: "listening", level: l.level, seconds: l.seconds, ..Default::default() });
    })
    .map_err(|e| e.trim_start_matches(recorder::MIC_ERROR).to_string())?;

    state.busy.store(true, Ordering::SeqCst);
    show_overlay(app);
    show(app, OverlayState { state: "listening", ..Default::default() });

    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let job = Job {
        app: app.clone(),
        engine: state.engine.clone(),
        busy: state.busy.clone(),
        settings,
        model_path,
        backend,
        stop: stop.clone(),
        finished: finished.clone(),
    };
    std::thread::Builder::new()
        .name("dictation".into())
        .spawn(move || job.run(rec))
        .map_err(|e| e.to_string())?;
    Ok(Session { stop, finished })
}

fn no_model() -> String {
    tr("Сначала выберите и скачайте модель в окне Auris Whisper.", "Choose and download a model in the Auris Whisper window first.")
}

struct Job {
    app: AppHandle,
    engine: Arc<Mutex<Engine>>,
    busy: Arc<AtomicBool>,
    settings: Settings,
    model_path: PathBuf,
    backend: Backend,
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

/// Что накопилось за диктовку: сколько вставлено и как быстро считает модель.
#[derive(Default)]
struct Progress {
    phrases: usize,
    last_text: String,
    /// Во сколько раз быстрее речи считает модель (по последней фразе).
    speed: Option<f64>,
    warned: bool,
}

impl Job {
    fn run(self, rec: Recording) {
        let result = self.dictate(&rec);
        // Запись могла ещё идти, если всё сорвалось раньше.
        let _ = rec.stop();
        self.busy.store(false, Ordering::SeqCst);
        self.finished.store(true, Ordering::Relaxed);
        match result {
            Ok(()) => {
                std::thread::sleep(Duration::from_millis(600));
                hide_overlay(&self.app);
            }
            Err(message) => {
                show(&self.app, OverlayState { state: "message", message, ..Default::default() });
                std::thread::sleep(Duration::from_secs(5));
                hide_overlay(&self.app);
            }
        }
    }

    fn dictate(&self, rec: &Recording) -> Result<(), String> {
        // Модель грузится один раз и остаётся в памяти — следующие диктовки начнутся сразу.
        let vad_path = {
            let mut engine = self.engine.lock().unwrap();
            if !engine.is_loaded_with(&self.model_path, self.backend, self.settings.use_gpu) {
                show(&self.app, OverlayState { state: "loading", ..Default::default() });
                engine.load(&self.model_path, self.backend, self.settings.use_gpu).map_err(|e| e.message())?;
            }
            engine.vad_model().map(PathBuf::from)
        };
        let mut streamer = Streamer::new(vad_path.as_deref());
        let mut progress = Progress::default();
        let mut committed = 0usize;
        let mut last_preview = Instant::now();

        while !self.stop.load(Ordering::Relaxed) {
            std::thread::sleep(TICK);
            let pending = rec.slice(committed, rec.len());
            match streamer.decide(&pending) {
                Decision::Commit { samples, speech } => {
                    if speech {
                        self.commit(&pending[..samples], &mut progress)?;
                    }
                    committed += samples;
                }
                Decision::Drop(samples) => committed += samples,
                Decision::Wait(speech) => {
                    // Предпросмотр недоговорённой фразы — только если модель успевает.
                    if preview_allowed(progress.speed) && speech >= MIN_SPEECH && last_preview.elapsed() >= PREVIEW_EVERY {
                        if let Some(text) = self.recognize(&pending, &mut progress) {
                            show(&self.app, OverlayState { state: "listening", text, ..Default::default() });
                        }
                        last_preview = Instant::now();
                    }
                }
            }
        }

        // Клавишу нажали ещё раз — дорасшифровать хвост.
        show(&self.app, OverlayState { state: "finishing", text: progress.last_text.clone(), ..Default::default() });
        let tail = rec.slice(committed, usize::MAX);
        if streamer.has_speech(&tail) {
            self.commit(&tail, &mut progress)?;
        }
        Ok(())
    }

    /// Расшифровать фразу и вставить её туда, где курсор.
    fn commit(&self, audio: &[f32], progress: &mut Progress) -> Result<(), String> {
        let Some(text) = self.recognize(audio, progress) else { return Ok(()) };
        let text = if progress.phrases > 0 { format!(" {text}") } else { text };
        match insert::insert(&text)? {
            insert::Inserted::Pasted => {}
            insert::Inserted::Clipboard(message) => {
                if !progress.warned {
                    progress.warned = true;
                    show(&self.app, OverlayState { state: "message", message, ..Default::default() });
                    std::thread::sleep(Duration::from_secs(3));
                }
            }
        }
        progress.phrases += 1;
        progress.last_text = text.trim().to_string();
        show(&self.app, OverlayState { state: "listening", text: progress.last_text.clone(), ..Default::default() });
        Ok(())
    }

    fn recognize(&self, audio: &[f32], progress: &mut Progress) -> Option<String> {
        let seconds = audio.len() as f64 / SAMPLE_RATE as f64;
        let language = (self.settings.language != "auto").then_some(self.settings.language.as_str());
        let started = Instant::now();
        let segments = {
            let mut engine = self.engine.lock().unwrap();
            match engine.transcribe(audio, language, false, &mut |_| {}) {
                Ok(s) => s,
                Err(EngineError::Cancelled) | Err(EngineError::Failed(_)) => return None,
            }
        };
        progress.speed = Some(seconds / started.elapsed().as_secs_f64().max(0.001));
        let mut text = plain_text(&segments).trim().to_string();
        if self.backend == Backend::Tone {
            text = ru_asr::tidy_phrase(&text);
        }
        (!text.is_empty()).then_some(text)
    }
}

/// Где закончилась фраза (секунды от начала недорасшифрованного звука), если закончилась.
fn phrase_end(spans: &[(f64, f64)], len: f64) -> Option<f64> {
    let last = spans.last()?;
    // Договорили и молчат — режем в паузе.
    if len - last.1 >= PHRASE_PAUSE {
        return Some((last.1 + 0.25).min(len));
    }
    // Говорят долго — режем на последней паузе внутри.
    if len >= LONG_PHRASE {
        if let Some(w) = spans.windows(2).rev().find(|w| w[1].0 - w[0].1 >= 0.2 && w[0].1 >= 3.0) {
            return Some((w[0].1 + w[1].0) / 2.0);
        }
    }
    (len >= MAX_PHRASE).then_some(len - 0.3)
}

/// Сколько секунд речи до момента `until`.
fn speech(spans: &[(f64, f64)], until: f64) -> f64 {
    spans.iter().map(|&(a, b)| (b.min(until) - a).max(0.0)).sum()
}

// MARK: - Индикатор в углу экрана

/// Окно индикатора: без рамки, прозрачное, поверх всех окон, не забирает фокус
/// и пропускает клики — курсор остаётся в программе, куда идёт текст.
pub fn create_overlay(app: &AppHandle) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("overlay.html".into()))
        .title("Auris Whisper")
        .inner_size(OVERLAY_W, OVERLAY_H)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()?;
    let _ = w.set_ignore_cursor_events(true);
    Ok(())
}

const OVERLAY_W: f64 = 380.0;
const OVERLAY_H: f64 = 96.0;

fn show_overlay(app: &AppHandle) {
    let Some(w) = app.get_webview_window(OVERLAY) else { return };
    // Правый нижний угол того экрана, где сейчас курсор, над панелью задач / Dock.
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let scale = m.scale_factor();
        let area = m.work_area();
        let (ww, wh) = ((OVERLAY_W * scale) as i32, (OVERLAY_H * scale) as i32);
        let margin = (16.0 * scale) as i32;
        let x = area.position.x + area.size.width as i32 - ww - margin;
        let y = area.position.y + area.size.height as i32 - wh - margin;
        let _ = w.set_size(Size::Logical(LogicalSize::new(OVERLAY_W, OVERLAY_H)));
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.show();
    let _ = w.set_ignore_cursor_events(true);
}

fn hide_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(OVERLAY) {
        let _ = w.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_ends_after_pause() {
        assert_eq!(phrase_end(&[(0.2, 1.5)], 1.9), None);
        assert_eq!(phrase_end(&[(0.2, 1.5)], 2.3), Some(1.75));
    }

    #[test]
    fn long_phrase_cut_at_inner_pause() {
        let spans = [(0.0, 5.0), (5.4, 12.0), (12.3, 16.0)];
        let cut = phrase_end(&spans, 16.1).unwrap();
        assert!((cut - 12.15).abs() < 1e-9);
    }

    #[test]
    fn no_speech_no_phrase() {
        assert_eq!(phrase_end(&[], 5.0), None);
        assert!((speech(&[(0.0, 1.0), (2.0, 3.0)], 2.5) - 1.5).abs() < 1e-9);
    }
}
