// Индикатор голосового ввода в углу экрана: состояние, уровень, текст.
"use strict";
const $ = (id) => document.getElementById(id);
const L = {
  ru: { listening: "Слушаю", loading: "Загружаю модель…", finishing: "Дописываю…", message: "Auris Whisper" },
  en: { listening: "Listening", loading: "Loading model…", finishing: "Finishing…", message: "Auris Whisper" },
};
// Конец фразы — последние сказанные слова; начало прячем за «…».
const tail = (s, n = 52) => (s.length > n ? "…" + s.slice(-n).replace(/^\S*\s/, "") : s);
const clock = (s) => { s = Math.floor(s || 0); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; };
let lastText = "", lastSeconds = 0;

window.__TAURI__.event.listen("dictation", ({ payload: p }) => {
  const pill = $("pill");
  pill.className = "pill on " + p.state;
  const l = p.en ? L.en : L.ru;
  $("title").textContent = l[p.state] || l.listening;
  if (p.state === "loading") { lastText = ""; lastSeconds = 0; }
  if (p.state === "listening") {
    // Время пошло заново — новая диктовка.
    if (p.seconds && p.seconds < lastSeconds) lastText = "";
    if (p.seconds) lastSeconds = p.seconds;
    $("time").textContent = clock(lastSeconds);
    pill.style.setProperty("--lvl", Math.min(1, p.level || 0).toFixed(2));
    if (p.text) lastText = p.text;
    $("text").textContent = tail(lastText);
  } else if (p.state === "message") {
    $("time").textContent = "";
    $("text").textContent = p.message;
  } else {
    $("time").textContent = p.state === "finishing" ? clock(lastSeconds) : "";
    if (p.text) lastText = p.text;
    $("text").textContent = tail(lastText);
  }
});
