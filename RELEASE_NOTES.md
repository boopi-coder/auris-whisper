## Что нового в 2.3.0

**Голосовой ввод в любую программу.** Нажмите горячую клавишу — по умолчанию **⌥Space** на Mac и **Ctrl+Alt+Space** на Windows и Linux — и говорите. Текст появится там, где стоит курсор: в мессенджере, почте, документе, браузере. Нажмите клавишу ещё раз, чтобы закончить.

- Текст вставляется по ходу речи: закончили фразу, сделали паузу — она уже в программе.
- В правом нижнем углу экрана — индикатор: запись идёт, сколько прошло времени, что распознаётся. На быстром компьютере он показывает фразу ещё до паузы.
- Распознаёт модель, выбранная в окне. Быстрее всего — T-One, точнее для русского — GigaAM v3; Whisper без видеокарты вставляет фразы с задержкой.
- Приложение живёт в трее: закрытие окна его не выключает. В настройках голосового ввода можно сменить клавишу и включить запуск вместе с системой.
- macOS попросит разрешение «Универсальный доступ» — без него Auris Whisper не может нажать «Вставить» за вас (текст тогда остаётся в буфере обмена). После обновлений приложения разрешение, возможно, придётся выдать заново.
- Linux: вставка работает в сеансах X11; в Wayland текст остаётся в буфере обмена.

Кнопка голосового ввода — на панели вверху окна, рядом с выбором модели.

---

## What's new in 2.3.0

**Voice input into any app.** Press the hotkey — **⌥Space** on a Mac and **Ctrl+Alt+Space** on Windows and Linux by default — and speak. The text appears where the cursor is: a messenger, email, a document, a browser. Press the key again to finish.

- Text is pasted as you speak: finish a phrase, pause — it is already there.
- An indicator in the bottom-right corner of the screen shows that recording is on, how long it has been and what is being recognised. On a fast computer it shows the phrase before you even pause.
- Recognition uses the model chosen in the window. T-One is the fastest, GigaAM v3 the most accurate for Russian; Whisper without a GPU pastes phrases with a delay.
- The app lives in the tray: closing the window does not quit it. The voice input settings let you change the key and start the app with the system.
- macOS asks for the Accessibility permission — without it Auris Whisper cannot press Paste for you (the text then stays on the clipboard). After app updates you may need to grant it again.
- Linux: pasting works in X11 sessions; under Wayland the text stays on the clipboard.

The voice input button is in the toolbar at the top of the window, next to the model picker.
