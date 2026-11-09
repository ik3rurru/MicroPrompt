//! MicroPrompt's own interface strings (the Teleprompter), kept apart from
//! Hex's table so merges from upstream stay clean. Same columns as
//! [`super::TRANSLATIONS`]; French, Portuguese, and Russian live in their
//! own tables as for every other string.

#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("System audio", ["Dźwięk systemowy", "系统音频", "システムオーディオ", "Systemaudio", "Audio del sistema", "Системний звук"]),
    ("Me", ["Ja", "我", "自分", "Ich", "Yo", "Я"]),
    ("Others", ["Inni", "其他人", "ほかの人", "Die anderen", "Los demás", "Інші"]),
    ("Interview prep", ["Przygotowanie do rozmowy", "面试准备", "面接の準備", "Interview-Vorbereitung", "Preparar entrevista", "Підготовка до співбесіди"]),
    ("Teleprompter opens an interview script by sections, with your CV, the offer, and company notes", ["Teleprompter otwiera scenariusz rozmowy podzielony na sekcje, z Twoim CV, ofertą i notatkami o firmie", "提词器按章节打开面试讲稿，并附上你的简历、职位信息和公司笔记", "テレプロンプターが面接用の原稿をセクションごとに開き、履歴書・求人・会社メモも添えます", "Der Teleprompter öffnet ein Interview-Skript nach Abschnitten, mit Lebenslauf, Stellenanzeige und Firmennotizen", "El Teleprompter abre un guion de entrevista por secciones, con tu CV, la oferta y notas de la empresa", "Телесуфлер відкриває сценарій співбесіди за розділами, з вашим резюме, вакансією та нотатками про компанію"]),
    ("CV", ["CV", "简历", "履歴書", "Lebenslauf", "CV", "Резюме"]),
    ("Offer", ["Oferta", "职位", "求人", "Stellenanzeige", "Oferta", "Вакансія"]),
    ("Company notes", ["Notatki o firmie", "公司笔记", "会社メモ", "Firmennotizen", "Notas de la empresa", "Нотатки про компанію"]),
    ("Both", ["Oba", "两者", "両方", "Beides", "Ambos", "Обидва"]),
    ("Couldn't save it: {}", ["Nie udało się zapisać: {}", "无法保存：{}", "保存できませんでした：{}", "Speichern fehlgeschlagen: {}", "No se pudo guardar: {}", "Не вдалося зберегти: {}"]),
    ("Open script…", ["Otwórz scenariusz…", "打开讲稿…", "原稿を開く…", "Skript öffnen…", "Abrir guion…", "Відкрити сценарій…"]),
    ("Open script", ["Otwórz scenariusz", "打开讲稿", "原稿を開く", "Skript öffnen", "Abrir guion", "Відкрити сценарій"]),
    ("Close script", ["Zamknij scenariusz", "关闭讲稿", "原稿を閉じる", "Skript schließen", "Cerrar guion", "Закрити сценарій"]),
    ("Scripts", ["Scenariusze", "讲稿", "原稿", "Skripte", "Guiones", "Сценарії"]),
    ("Couldn't open it: {}", ["Nie udało się otworzyć: {}", "无法打开：{}", "開けませんでした：{}", "Öffnen fehlgeschlagen: {}", "No se pudo abrir: {}", "Не вдалося відкрити: {}"]),
    ("Saved to {}", ["Zapisano w {}", "已保存到 {}", "{} に保存しました", "Gesichert unter {}", "Guardado en {}", "Збережено в {}"]),
    ("Save as .srt", ["Zapisz jako .srt", "存为 .srt", ".srt で保存", "Als .srt sichern", "Guardar como .srt", "Зберегти як .srt"]),
    ("Save as .txt", ["Zapisz jako .txt", "存为 .txt", ".txt で保存", "Als .txt sichern", "Guardar como .txt", "Зберегти як .txt"]),
    ("Export", ["Eksport", "导出", "書き出し", "Exportieren", "Exportar", "Експорт"]),
    ("Teleprompter", ["Teleprompter", "提词器", "テレプロンプター", "Teleprompter", "Teleprompter", "Телесуфлер"]),
    ("Start a session and speak; your words show here", ["Rozpocznij sesję i mów; Twoje słowa pojawią się tutaj", "开始会话并说话；你的话会显示在这里", "セッションを開始して話すと、ここに言葉が表示されます", "Starte eine Sitzung und sprich; deine Worte erscheinen hier", "Empieza una sesión y habla; tus palabras aparecen aquí", "Почніть сеанс і говоріть; ваші слова з’являться тут"]),
    ("Listening… your words show here", ["Słucham… Twoje słowa pojawią się tutaj", "正在聆听…你的话会显示在这里", "聞いています…話した言葉がここに表示されます", "Hört zu… deine Worte erscheinen hier", "Escuchando… tus palabras aparecen aquí", "Слухаю… ваші слова з’являться тут"]),
    ("Stopping…", ["Zatrzymywanie…", "正在停止…", "停止しています…", "Wird beendet…", "Parando…", "Зупинення…"]),
    ("Stop session", ["Zakończ sesję", "停止会话", "セッションを停止", "Sitzung beenden", "Parar sesión", "Зупинити сеанс"]),
    ("Start session", ["Rozpocznij sesję", "开始会话", "セッションを開始", "Sitzung starten", "Empezar sesión", "Почати сеанс"]),
    ("Live subtitles of your voice: hex teleprompter start, stop, and status", ["Napisy na żywo z Twojego głosu: hex teleprompter start, stop i status", "实时显示你说话的字幕：hex teleprompter start、stop 和 status", "声をリアルタイムで字幕に：hex teleprompter start、stop、status", "Live-Untertitel deiner Stimme: hex teleprompter start, stop und status", "Subtítulos en directo de tu voz: hex teleprompter start, stop y status", "Субтитри вашого голосу наживо: hex teleprompter start, stop і status"]),
    ("Duration", ["Czas trwania", "时长", "長さ", "Dauer", "Duración", "Тривалість"]),
    ("Start and stop the session", ["Rozpocznij i zakończ sesję", "开始和停止会话", "セッションの開始と停止", "Sitzung starten und beenden", "Empezar y parar la sesión", "Почати й зупинити сеанс"]),
    ("Works with the window closed", ["Działa przy zamkniętym oknie", "窗口关闭时也可用", "ウィンドウを閉じていても使えます", "Funktioniert bei geschlossenem Fenster", "Funciona con la ventana cerrada", "Працює із закритим вікном"]),
];
