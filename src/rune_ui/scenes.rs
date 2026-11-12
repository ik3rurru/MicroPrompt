//! Fixtures for snapshots and tests: the named starting states `--scene`
//! opens.

use super::*;

/// A named starting state for `--scene`.
pub(super) type Scene = (&'static str, fn(&mut Ui));

pub(super) const SCENES: &[Scene] = &[
    // The running service's real state, for headless snapshots and scripts
    // (`--snap`): waits for its first snapshot instead of drawing
    // "Connecting…" at virtual time zero.
    ("live", |ui| {
        // Reads the saved settings and the running Hex; writes neither.
        let settings = AppSettings::read().unwrap_or_default();
        crate::desktop::i18n::apply(settings.ui_language.as_deref());
        *ui = Ui::with(settings, Service::Fixture(Vec::new()));
        let mut backend = Backend::connect(Default::default(), Arc::new(|| {}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while backend.state().is_none() && backend.error().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
            backend.refresh();
        }
        ui.service = Service::Live(Box::new(backend));
        poll(ui, 0.0, &mut Tx::default());
    }),
    ("portal", |ui| *ui = portal_fixture()),
    ("portal-mouse", |ui| {
        *ui = portal_fixture();
        ui.settings.dictation_hotkey = HotkeyBinding {
            modifiers: Default::default(),
            key: crate::app_settings::HotkeyKey::mouse(3),
        };
        if let Some(view) = &mut ui.view {
            view.listener.shortcuts =
                DesktopShortcuts::Portal(vec![crate::desktop::host::DesktopGrantedShortcut {
                    name: "Dictation".into(),
                    trigger: None,
                    unconfirmed: false,
                }]);
        }
    }),
    ("commands", |ui| *ui = fixture(DesktopShortcuts::Commands)),
    ("disabled", |ui| {
        *ui = portal_fixture();
        if let Some(view) = &mut ui.view {
            view.listener.running = false;
            view.listener.status = "Disabled".into();
        }
    }),
    ("capturing", |ui| {
        *ui = portal_fixture();
        ui.capture = Capture::Listening {
            target: ShortcutTarget::Dictation,
            message: None,
            held: Default::default(),
        };
    }),
    ("chinese", |ui| {
        *ui = portal_fixture();
        ui.settings.ui_language = Some("zh".into());
        crate::desktop::i18n::apply(Some("zh"));
    }),
    ("menus", |ui| {
        *ui = portal_fixture();
        ui.open = Some(Menu::Microphone);
        ui.microphones = Ok(crate::audio::InputDevices {
            names: vec!["Built-in Audio Analog Stereo".into(), "Shure MV7".into()],
            system_default: Some("Shure MV7".into()),
            default_is_hex_microphone: false,
            ..Default::default()
        });
    }),
    ("history", |ui| {
        *ui = portal_fixture();
        ui.history = history::HistoryPane::fixture();
        ui.pane = Pane::History;
    }),
    ("history-menu", |ui| {
        *ui = portal_fixture();
        ui.history = history::HistoryPane::fixture();
        ui.pane = Pane::History;
        ui.open = Some(Menu::HistoryRetention);
    }),
    ("history-clear", |ui| {
        *ui = portal_fixture();
        ui.history = history::HistoryPane::fixture();
        ui.history.clear_armed = true;
        ui.pane = Pane::History;
    }),
    // MicroPrompt: a kept Teleprompter session, selected, just saved as .srt.
    ("history-teleprompter", |ui| {
        *ui = portal_fixture();
        ui.history = history::HistoryPane::fixture();
        let mut session = ui.history.entries[0].clone();
        session.id = 4;
        session.kind = crate::history::HistoryKind::Teleprompter;
        session.application = None;
        session.audio_ms = 25_980;
        session.inference_ms = 0;
        session.total_ms = 0;
        session.model = Some("Nemotron 3.5 ASR".into());
        session.final_text = "Hola, esto es una prueba de subtítulos en directo. MicroPrompt escribe lo que digo mientras hablo.".into();
        session.raw_text = session.final_text.clone();
        session.notes = Some(
            "## Notes\n\n**Temas:**\n- Fecha de lanzamiento\n\n**Decisiones:**\n- Lanzar el 15 de diciembre\n\n**Tareas:**\n- Presupuesto de marketing: Ana (viernes)\n\n## Insights\n\n**Puntos fuertes:**\n- Propusiste una fecha concreta"
                .into(),
        );
        ui.history.entries.insert(0, session);
        ui.history.selected = Some(4);
        ui.history.exported = Some((
            4,
            Ok("C:\\Users\\you\\Documents\\MicroPrompt\\Teleprompter 2026-11-07 21-30.srt".into()),
        ));
        ui.pane = Pane::History;
    }),
    ("history-empty", |ui| {
        *ui = portal_fixture();
        ui.history = history::HistoryPane::fixture_empty();
        ui.pane = Pane::History;
    }),
    ("output", |ui| {
        *ui = portal_fixture();
        output::fixture(ui);
        ui.pane = Pane::Output;
    }),
    ("output-open", |ui| {
        *ui = portal_fixture();
        output::fixture(ui);
        ui.output.expanded = Some(0);
        ui.output.prompt_copied = true;
        ui.pane = Pane::Output;
    }),
    ("output-trust", |ui| {
        *ui = portal_fixture();
        output::trust_fixture(ui);
    }),
    ("output-empty", |ui| {
        *ui = portal_fixture();
        ui.pane = Pane::Output;
    }),
    ("modes", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
        ui.pane = Pane::Modes;
    }),
    ("modes-opencode-not-running", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
        ui.models = models::Models::fixture_not_running();
        ui.settings.dictation_processing.modes[0]
            .post_processing
            .enabled = false;
        ui.pane = Pane::Modes;
    }),
    ("modes-global", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
        ui.modes.select(modes::Selection::Default);
        ui.pane = Pane::Modes;
    }),
    ("modes-picker", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
        ui.modes.application_picker_open = true;
        ui.pane = Pane::Modes;
    }),
    ("modes-transformation-menu", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
        ui.open = Some(Menu::Transformations);
        ui.pane = Pane::Modes;
    }),
    ("picker", |ui| {
        *ui = portal_fixture();
        picker::fixture(ui, None);
    }),
    ("picker-downloading", |ui| {
        *ui = portal_fixture();
        picker::fixture(ui, Some(TranscriptionModelId::CohereTranscribe));
    }),
    // Labs › Parakeet Ultra: its card's details, credit, and published
    // benchmarks (lists Ultra only where the saved settings turn it on).
    ("picker-parakeet-ultra", |ui| {
        *ui = portal_fixture();
        picker::parakeet_ultra_fixture(ui);
    }),
    // Every model, with the full list's Add model file….
    ("picker-custom-models", |ui| {
        *ui = portal_fixture();
        picker::all_fixture(ui);
    }),
    // Found on this computer: Parakeet v3's exact file and an untested Whisper.
    ("picker-found", |ui| {
        *ui = portal_fixture();
        ui.found.models = found::fixture();
        picker::all_fixture(ui);
    }),
    // Japanese saved: Japanese models compared, SenseVoice downloading.
    ("picker-japanese", |ui| {
        *ui = portal_fixture();
        picker::japanese_fixture(ui, Some(TranscriptionModelId::SenseVoiceSmall));
    }),
    // Every model: the English-only ones fold away and switch the language.
    ("picker-japanese-all", |ui| {
        *ui = portal_fixture();
        picker::japanese_fixture(ui, None);
        (ui.picker.view, ui.picker.others_open) = (picker::View::All, true);
    }),
    // Parakeet chosen over Japanese: Settings names the language change.
    ("language-fallback", |ui| {
        *ui = portal_fixture();
        ui.picker.fallback = Some(picker::LanguageFallback {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            from: "ja".into(),
        });
    }),
    // Whisper's languages open below the model.
    ("language-menu", |ui| {
        *ui = portal_fixture();
        ui.settings.transcription.model = TranscriptionModelId::WhisperLargeV3Turbo;
        ui.picker.installed = Some(vec![TranscriptionModelId::WhisperLargeV3Turbo]);
        ui.open = Some(Menu::Pick("transcription-language"));
    }),
    ("shortcut-mode", shortcut_mode::scene_modifier),
    ("shortcut-mode-key", shortcut_mode::scene_key),
    ("shortcut-mode-chord", shortcut_mode::scene_chord),
    ("shortcut-mode-motion", shortcut_mode::scene_motion),
    // The Shortcut row with a modifier-only shortcut, Left Option, beside
    // its side picker (macOS), in each face of the recorder.
    ("shortcut-row", |ui| shortcut_row(ui, Capture::Idle)),
    ("shortcut-row-right", |ui| {
        shortcut_row(ui, Capture::Idle);
        ui.settings.dictation_hotkey.modifiers.option = Some(ModifierSide::Right);
    }),
    // Two sided modifiers have no side picker, so their keycaps name the side.
    ("shortcut-row-sided-chord", |ui| {
        shortcut_row(ui, Capture::Idle);
        ui.settings.dictation_hotkey.modifiers.command = Some(ModifierSide::Right);
    }),
    ("shortcut-row-recording", |ui| {
        shortcut_row(ui, listening(None, HotkeyModifiers::default()));
    }),
    ("shortcut-row-held", |ui| {
        shortcut_row(ui, listening(None, HotkeyModifiers::option_command()));
    }),
    ("shortcut-row-rejected", |ui| {
        let message = crate::desktop::i18n::tr("Add a modifier");
        shortcut_row(ui, listening(Some(message), HotkeyModifiers::default()));
    }),
    ("shortcut-row-saved", |ui| {
        shortcut_row(
            ui,
            Capture::Saved {
                target: ShortcutTarget::Dictation,
                until: None,
            },
        );
    }),
    // The first run on Linux (Windows is the same window): setting up Hex
    // with nothing found, the model step with models found here, the demo,
    // waiting for the model, the practice game, and Hex is ready.
    ("onboarding", |ui| {
        first_run(ui, FirstRun::Fresh, Page::Download)
    }),
    ("onboarding-download-failed", |ui| {
        first_run(ui, FirstRun::Failed, Page::Download)
    }),
    // Continue without a model after it, then Settings with no model.
    ("onboarding-no-model", |ui| {
        first_run(ui, FirstRun::Failed, Page::Download);
        onboarding_kit::live::continue_without_model(ui);
    }),
    ("onboarding-model-found", found::listed),
    ("onboarding-model-found-downloading", found::loading),
    ("onboarding-model-found-recommended", found::present),
    ("onboarding-model-found-back", found::chosen),
    ("onboarding-demo", |ui| {
        first_run(ui, FirstRun::Downloading, Page::Demo)
    }),
    ("onboarding-waiting", |ui| {
        first_run(ui, FirstRun::Downloading, Page::Waiting)
    }),
    // GNOME and KDE on Wayland: why the desktop asks before the first paste.
    ("onboarding-paste", |ui| {
        first_run(ui, FirstRun::PasteAccess, Page::Permissions)
    }),
    ("onboarding-try", |ui| {
        first_run(ui, FirstRun::Ready, Page::Try)
    }),
    ("onboarding-ready", |ui| {
        first_run(ui, FirstRun::Ready, Page::Ready)
    }),
    // Windows with its microphone privacy switch off.
    ("windows-onboarding-microphone", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.platform.permissions.microphone =
            crate::desktop::onboarding::PermissionState::NeedsSettings;
        onboarding::fixture(ui, crate::desktop::onboarding::Step::Setup, true);
        onboarding_kit::live::fixture(ui, Page::Permissions);
    }),
    // macOS and Windows surfaces, from preview data (see `platform`).
    ("mac-settings", |ui| *ui = mac_fixture()),
    // The microphone row following a recorded voice (`--video`), and with
    // the microphone released while idle, when no audio arrives.
    ("mac-settings-mic-level", |ui| {
        *ui = mac_fixture();
        ui.mic_level = general::MicLevel::playing(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/vocabulary/audio/term-opencode-2.wav"
        )));
    }),
    ("mac-settings-mic-released", |ui| {
        *ui = mac_fixture();
        ui.settings.release_microphone_while_idle = true;
        ui.mic_level = general::MicLevel::closed();
    }),
    ("mac-settings-ducking", |ui| {
        *ui = mac_fixture();
        ui.settings.recording_audio_behavior = crate::app_settings::RecordingAudioBehavior::Duck;
    }),
    ("mac-input-source-models", |ui| {
        use crate::app_settings::KeyboardModelChoice;
        *ui = mac_fixture();
        ui.advanced = vec![Pane::General];
        ui.settings.keyboard_model_switching.enabled = true;
        ui.settings.keyboard_model_switching.mappings = [
            (
                "com.apple.keylayout.ABC",
                TranscriptionModelId::ParakeetUnifiedEnglish,
                "en",
            ),
            (
                "com.apple.keylayout.RussianWin",
                TranscriptionModelId::ParakeetV3,
                "ru",
            ),
        ]
        .into_iter()
        .map(|(id, model, language)| {
            (
                id.into(),
                KeyboardModelChoice {
                    model,
                    language: language.into(),
                },
            )
        })
        .collect();
        ui.platform.input_sources = [
            ("com.apple.keylayout.ABC", "ABC", "en"),
            ("com.apple.keylayout.RussianWin", "Russian - PC", "ru"),
            ("com.apple.keylayout.Spanish", "Spanish", "es"),
        ]
        .into_iter()
        .map(
            |(id, name, language)| crate::transcription::keyboard_model_switching::InputSource {
                id: id.into(),
                name: name.into(),
                languages: vec![language.into()],
            },
        )
        .collect();
        ui.picker.installed = Some(vec![
            TranscriptionModelId::ParakeetUnifiedEnglish,
            TranscriptionModelId::ParakeetV3,
        ]);
    }),
    // The sound sets sheet at rest, then Wood's dictation recording.
    ("sound-sets", |ui| {
        *ui = mac_fixture();
        sound_sets::fixture(ui, None);
    }),
    ("sound-sets-playing", |ui| {
        *ui = mac_fixture();
        sound_sets::fixture(
            ui,
            Some((
                crate::feedback::SoundSet::Wood,
                crate::feedback::Audition::Dictation,
                900.0,
            )),
        );
    }),
    ("mac-microphone-missing", |ui| {
        *ui = mac_fixture();
        ui.settings.microphone = Some("Shure MV7".into());
        ui.open = Some(Menu::Microphone);
    }),
    ("mac-microphone-lid-closed", |ui| {
        *ui = mac_fixture();
        if let Ok(devices) = &mut ui.microphones {
            devices.system_default = Some("Studio Display Microphone".into());
            devices.off_with_lid_closed = Some("MacBook Pro Microphone".into());
            devices.default_is_off_with_lid_closed = true;
        }
        ui.open = Some(Menu::Microphone);
    }),
    // System default passes over the Bluetooth default for the built-in mic.
    ("mac-microphone-bluetooth", |ui| {
        *ui = mac_fixture();
        if let Ok(devices) = &mut ui.microphones {
            devices.system_default = Some("MacBook Pro Microphone".into());
            devices.bluetooth_default_passed_over = Some("Kit Pods".into());
        }
        ui.advanced = vec![Pane::General];
    }),
    ("mac-settings-vocabulary", |ui| {
        *ui = mac_fixture();
        vocabulary::fixture(ui, true);
    }),
    ("mac-settings-vocabulary-empty", |ui| {
        *ui = mac_fixture();
        vocabulary::fixture(ui, false);
    }),
    // Cohere ignores the list: the row and the sheet say so.
    ("vocabulary-unused", |ui| {
        *ui = portal_fixture();
        ui.settings.transcription.model = TranscriptionModelId::CohereTranscribe;
        ui.picker.installed = Some(vec![TranscriptionModelId::CohereTranscribe]);
        vocabulary::fixture(ui, true);
    }),
    // Whisper reads only the terms that fit its prompt: the sheet says how many.
    ("vocabulary-budget", |ui| {
        *ui = portal_fixture();
        ui.settings.transcription.model = TranscriptionModelId::WhisperLargeV3Turbo;
        ui.picker.installed = Some(vec![TranscriptionModelId::WhisperLargeV3Turbo]);
        vocabulary::fixture(ui, true);
        let entries: Vec<_> = (1..=52)
            .map(|n| crate::dictation::vocabulary::VocabularyEntry {
                term: format!("Project codename {n}"),
                sounds_like: Vec::new(),
            })
            .collect();
        ui.settings.vocabulary = entries;
        ui.vocabulary.reload(&ui.settings.vocabulary);
    }),
    ("mac-settings-recording", |ui| {
        *ui = mac_fixture();
        let held = crate::app_settings::HotkeyModifiers {
            option: Some(crate::app_settings::ModifierSide::Either),
            command: Some(crate::app_settings::ModifierSide::Either),
            ..Default::default()
        };
        ui.capture = Capture::Listening {
            target: ShortcutTarget::Dictation,
            message: None,
            held,
        };
    }),
    ("mac-settings-permissions", |ui| {
        platform_settings::mac_permissions_fixture(ui, false)
    }),
    ("mac-settings-permissions-stale", |ui| {
        platform_settings::mac_permissions_fixture(ui, true)
    }),
    ("mac-settings-pause-browser", |ui| {
        *ui = mac_fixture();
        ui.settings.recording_audio_behavior =
            crate::app_settings::RecordingAudioBehavior::PauseMedia;
        ui.settings.pause_browser_media = true;
        ui.platform.system_audio = false;
    }),
    ("mac-settings-advanced", |ui| {
        *ui = mac_fixture();
        ui.advanced = vec![Pane::General];
    }),
    ("mac-settings-dev-channel", |ui| {
        *ui = mac_fixture();
        ui.settings.updates.channel = Some(crate::app_settings::UpdateChannel::Dev);
    }),
    // A local build: the badge, the quiet status line, no channel row.
    ("mac-local-build", |ui| {
        *ui = mac_fixture();
        ui.build_preview = Some(BuildKind::Local);
        if let Some(view) = &mut ui.view {
            view.update_status = DesktopUpdateStatus::SourceBuild;
        }
    }),
    ("mac-test-build", |ui| {
        *ui = mac_fixture();
        ui.build_preview = Some(BuildKind::Test);
    }),
    ("mac-update-ready", |ui| {
        *ui = mac_fixture();
        if let Some(view) = &mut ui.view {
            view.update_status = DesktopUpdateStatus::ReadyToRestart;
        }
    }),
    ("mac-update-checking", |ui| {
        *ui = mac_fixture();
        if let Some(view) = &mut ui.view {
            view.update_status = DesktopUpdateStatus::Checking;
        }
    }),
    ("mac-update-downloading", |ui| {
        *ui = mac_fixture();
        if let Some(view) = &mut ui.view {
            view.update_status = DesktopUpdateStatus::Downloading;
        }
    }),
    ("windows-settings", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.paste_with_shift = false;
    }),
    ("meetings", |ui| {
        *ui = mac_fixture();
        ui.pane = Pane::Meetings;
    }),
    ("meetings-settings", |ui| {
        *ui = mac_fixture();
        ui.meetings.view = meetings::View::Settings;
        ui.pane = Pane::Meetings;
    }),
    // Summaries' picker open: Off, On this Mac, OpenCode, each saying what
    // it does; then with Apple Intelligence off, On this Mac kept, disabled,
    // with the fix.
    ("meetings-summaries", |ui| {
        *ui = mac_fixture();
        ui.meetings.view = meetings::View::Settings;
        ui.pane = Pane::Meetings;
        ui.open = Some(Menu::Pick("meeting-summaries"));
    }),
    ("meetings-summaries-unavailable", |ui| {
        *ui = mac_fixture();
        ui.platform.notes = platform::NotesAvailability::NotEnabled;
        ui.meetings.view = meetings::View::Settings;
        ui.pane = Pane::Meetings;
        ui.open = Some(Menu::Pick("meeting-summaries"));
    }),
    // Summaries set to OpenCode: its model nested under it, then with
    // OpenCode not running, the saved model kept.
    ("meetings-summary-model", |ui| {
        summary_model_scene(ui, models::Models::fixture())
    }),
    ("meetings-summary-model-not-running", |ui| {
        summary_model_scene(ui, models::Models::fixture_not_running())
    }),
    // System Audio not yet allowed: tap Allow to wait, again to grant.
    ("meetings-allow", |ui| {
        *ui = mac_fixture();
        ui.platform.system_audio = false;
        ui.meetings.view = meetings::View::Settings;
        ui.pane = Pane::Meetings;
    }),
    ("meetings-calendar", |ui| {
        *ui = mac_fixture();
        ui.settings.meetings.calendar.connected = true;
        ui.settings.meetings.calendar.excluded_calendars = vec!["birthdays".into()];
        ui.platform.calendar = platform::CalendarView::preview();
        ui.meetings.view = meetings::View::Settings;
        ui.pane = Pane::Meetings;
    }),
    ("meetings-calendar-denied", |ui| {
        *ui = mac_fixture();
        ui.platform.calendar.access = crate::calendar::CalendarAccess::Denied;
        ui.meetings.view = meetings::View::Settings;
        ui.pane = Pane::Meetings;
    }),
    ("meetings-off", |ui| {
        *ui = mac_fixture();
        ui.settings.meetings.enabled = false;
        ui.pane = Pane::Meetings;
    }),
    ("meetings-recording", |ui| {
        *ui = mac_fixture();
        ui.meetings.select("preview-live".into());
        ui.pane = Pane::Meetings;
    }),
    ("meetings-actions", |ui| {
        *ui = mac_fixture();
        ui.open = Some(Menu::MeetingActions);
        ui.pane = Pane::Meetings;
    }),
    ("meetings-delete", |ui| {
        *ui = mac_fixture();
        ui.meetings.delete_armed = Some("preview-notes".into());
        ui.pane = Pane::Meetings;
    }),
    ("meetings-rename", |ui| {
        *ui = mac_fixture();
        ui.meetings.renaming = Some("preview-notes".into());
        ui.meetings.rename = TextEdit::new("Arcade launch plan");
        ui.pane = Pane::Meetings;
    }),
    ("meetings-failed", |ui| {
        *ui = mac_fixture();
        ui.meetings.select("preview-failed".into());
        ui.pane = Pane::Meetings;
    }),
    ("meetings-chapters", |ui| {
        *ui = mac_fixture();
        ui.meetings = meetings::MeetingsPane::chapters_fixture();
        ui.pane = Pane::Meetings;
    }),
    ("meetings-empty", |ui| {
        *ui = mac_fixture();
        ui.meetings = meetings::MeetingsPane::default();
        ui.pane = Pane::Meetings;
    }),
    #[cfg(hex_capture)]
    ("mac-scrawl-off", |ui| scrawl_fixture(ui, false, |_| {})),
    #[cfg(hex_capture)]
    ("mac-scrawl", |ui| scrawl_fixture(ui, true, |_| {})),
    #[cfg(hex_capture)]
    ("mac-scrawl-running", |ui| {
        scrawl_fixture(ui, true, |view| view.scrawl_running = true)
    }),
    #[cfg(hex_capture)]
    ("mac-scrawl-permission", |ui| {
        scrawl_fixture(ui, true, |view| view.screen_recording = false)
    }),
    #[cfg(hex_capture)]
    ("mac-scrawl-restart", |ui| {
        scrawl_fixture(ui, true, |view| {
            view.screen_recording = false;
            view.screen_needs_restart = true;
        })
    }),
    ("mac-voice-filter", |ui| {
        *ui = mac_fixture();
        ui.pane = Pane::VoiceFilter;
    }),
    ("mac-voice-filter-muted", |ui| {
        *ui = mac_fixture();
        ui.platform.tuning.mode = crate::audio::voice_filter::VoiceFilterMode::Muted;
        ui.platform.demo = Some(fun::preview_demo(
            false,
            Some(crate::audio::hex_microphone::DemoPlaybackTrack::CallHears),
        ));
        ui.pane = Pane::VoiceFilter;
    }),
    ("mac-voice-filter-driver-missing", |ui| {
        *ui = mac_fixture();
        if let Some(driver) = &mut ui.platform.hex_microphone {
            driver.driver_bundle_installed = false;
            driver.driver_loaded = false;
            driver.installed_version = None;
            driver.active_apps.clear();
        }
        ui.platform.demo = Some(fun::preview_demo(true, None));
        ui.pane = Pane::VoiceFilter;
    }),
    ("voice-filter", |ui| {
        *ui = fixture(DesktopShortcuts::Commands);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::LINUX);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.experimental.hex_microphone = true;
        ui.pane = Pane::VoiceFilter;
    }),
    ("voice-filter-unavailable", |ui| {
        *ui = fixture(DesktopShortcuts::Commands);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::LINUX);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        if let Some(source) = &mut ui.platform.hex_microphone {
            source.driver_loaded = false;
            source.action_state = crate::audio::hex_microphone::DriverActionState::Failed(
                "pw-cat was not found; Hex Microphone needs PipeWire".into(),
            );
        }
        ui.settings.experimental.hex_microphone = true;
        ui.pane = Pane::VoiceFilter;
    }),
    ("mac-hud-lab", |ui| {
        *ui = mac_fixture();
        ui.platform.developer = true;
        ui.pane = Pane::HudLab;
    }),
    ("mac-labs", labs_with_some_on),
    // Where hex://labs/vocabulary-biasing lands: its group's chip.
    ("mac-labs-link", |ui| {
        *ui = mac_fixture();
        ui.labs_open = true;
        ui.lab_store.group = Some(crate::labs::Feature::VocabularyBiasing.group());
    }),
    ("mac-labs-on", |ui| {
        labs_with_some_on(ui);
        ui.lab_store.tab = labs::Tab::On;
    }),
    ("mac-labs-category", |ui| {
        labs_with_some_on(ui);
        ui.lab_store.group = Some(crate::labs::Group::Shortcuts);
    }),
    ("mac-labs-search", |ui| {
        labs_with_some_on(ui);
        ui.lab_store.search = rune::TextEdit::new("paste".to_string());
    }),
    // A lab's own settings on its card while it's on.
    ("mac-labs-settings", |ui| {
        labs_with_some_on(ui);
        ui.settings.labs.unload_idle = true;
        ui.lab_store.group = Some(crate::labs::Feature::UnloadIdle.group());
    }),
    // Labs › Screenshots' card while on (macOS only: elsewhere the lab has
    // no row).
    ("mac-labs-screenshots", |ui| {
        labs_with_some_on(ui);
        ui.settings.labs.screenshots = true;
        ui.lab_store.group = Some(crate::labs::Feature::Screenshots.group());
    }),
    // Labs on Windows and Linux: only their OS's labs, groups holding a
    // lab that's on open.
    ("windows-labs", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.paste_with_shift = true;
        ui.labs_open = true;
    }),
    ("linux-labs", |ui| {
        *ui = portal_fixture();
        ui.platform.caps = platform::Capabilities::LINUX;
        ui.settings.labs.skip_pauses = true;
        ui.labs_open = true;
    }),
    ("mac-production", |ui| {
        *ui = mac_fixture();
        ui.settings.meetings.enabled = false;
    }),
    // MicroPrompt: a session with closed lines and words still settling.
    ("teleprompter", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.labs.teleprompter = true;
        ui.pane = Pane::Teleprompter;
        ui.platform.teleprompter = crate::teleprompter::View {
            running: true,
            segments: vec![
                "Hola, esto es una prueba de subtítulos en directo.".into(),
                "MicroPrompt escribe lo que digo mientras hablo.".into(),
                "La latencia tiene que ser baja para leer cómodo, también cuando la frase es larga y ocupa más de una línea.".into(),
            ],
            stable: "Después vendrá el guion".into(),
            tentative: " que sigue mi".into(),
            ..Default::default()
        };
    }),
    // MicroPrompt: a call heard from both sources, each turn named.
    ("teleprompter-speakers", |ui| {
        use crate::teleprompter::source::Speaker::{Me, Others};
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.labs.teleprompter = true;
        ui.pane = Pane::Teleprompter;
        ui.platform.teleprompter = crate::teleprompter::View {
            running: true,
            segments: vec![
                "Gracias por venir. ¿Me cuentas un poco sobre ti?".into(),
                "Claro. Llevo siete años en backend, sobre todo con Python y Go.".into(),
                "Ahora lidero un equipo de cuatro personas.".into(),
            ],
            speakers: vec![Some(Others), Some(Me), Some(Me)],
            speaker: Some(Others),
            stable: "¿Y por qué quieres".into(),
            tentative: " cambiar de".into(),
            ..Default::default()
        };
    }),
    ("teleprompter-idle", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.labs.teleprompter = true;
        ui.settings.labs.teleprompter_hotkey =
            serde_json::from_value(serde_json::json!({ "keys": [
                { "modifier": "control", "side": "any" },
                { "modifier": "option", "side": "any" },
                { "code": 17, "label": "T" },
            ]}))
            .ok();
        ui.pane = Pane::Teleprompter;
    }),
    // MicroPrompt: Labs › Assistant with Ollama running and a test answer
    // streaming in, beside a session.
    ("teleprompter-assistant", |ui| {
        use crate::teleprompter::assistant::{Model, Status, View};
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.labs.teleprompter = true;
        ui.settings.labs.assistant = true;
        ui.pane = Pane::Teleprompter;
        let model = |name: &str| Model {
            name: name.into(),
            thinks: true,
        };
        ui.platform.teleprompter_assistant = View {
            status: Status::Running,
            models: vec![
                model("qwen3.5:9b"),
                model("gemma4:12b"),
                model("qwen3.5:4b"),
            ],
            ..View::default()
        };
        use crate::teleprompter::prompts::Action;
        use crate::teleprompter::source::Speaker::{Me, Others};
        let answer = |action, after, text: &str, done| crate::teleprompter::Answer {
            action,
            after,
            text: text.into(),
            done,
            error: None,
            used: true,
            question: None,
        };
        ui.platform.teleprompter = crate::teleprompter::View {
            running: true,
            segments: vec![
                "¿Me cuentas un proyecto del que estés orgulloso?".into(),
                "Pues el año pasado migramos los pagos a colas.".into(),
            ],
            speakers: vec![Some(Others), Some(Me)],
            times: vec![(0, 3_000), (9_000, 12_000)],
            answers: vec![
                answer(
                    Action::Suggest,
                    1,
                    concat!(
                        "**Say this:** El año pasado lideré la migración de los pagos a colas: ",
                        "bajamos los errores un 80 % sin parar el servicio.

",
                        "**Key points:**
- Liderazgo de 4 personas
- Cero caídas
",
                        "- Métricas antes y después",
                    ),
                    true,
                ),
                answer(
                    Action::Answer,
                    2,
                    "**Understood:** qué aprendiste de esa migración",
                    true,
                ),
                crate::teleprompter::Answer {
                    question: Some("dame un ejemplo más corto".into()),
                    ..answer(
                        Action::Ask,
                        2,
                        "Aprendí a medir antes de cambiar: los errores bajaron un 80 %.",
                        false,
                    )
                },
            ],
            ..Default::default()
        };
    }),
    // MicroPrompt: a call as a thread, one message left out of the AI.
    ("teleprompter-thread", |ui| {
        use crate::teleprompter::source::Speaker::{Me, Others};
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.labs.teleprompter = true;
        ui.settings.labs.assistant = true;
        ui.pane = Pane::Teleprompter;
        ui.platform.teleprompter_assistant = crate::teleprompter::assistant::View {
            status: crate::teleprompter::assistant::Status::Running,
            models: vec![crate::teleprompter::assistant::Model {
                name: "gemma4:12b".into(),
                thinks: true,
            }],
            ..Default::default()
        };
        ui.platform.teleprompter = crate::teleprompter::View {
            running: true,
            segments: vec![
                "Gracias por venir.".into(),
                "¿Me cuentas un poco sobre ti?".into(),
                "Claro. Llevo siete años en backend, sobre todo con Python y Go.".into(),
                "Perdona, se me ha caído el café.".into(),
                "Ahora lidero un equipo de cuatro personas.".into(),
            ],
            speakers: vec![Some(Others), Some(Others), Some(Me), Some(Me), Some(Me)],
            times: vec![
                (0, 1_200),
                (1_400, 3_000),
                (3_600, 8_000),
                (11_000, 12_500),
                (16_000, 19_000),
            ],
            unused: [3].into(),
            answers: vec![crate::teleprompter::Answer {
                action: crate::teleprompter::prompts::Action::Hint,
                after: 2,
                text: concat!(
                    "*From your script: Háblame de ti*

> ¿Me cuentas un poco sobre ti?

",
                    "- 7 años backend, Python y Go
- Logística y pagos
- Lidero 4 personas
",
                )
                .into(),
                done: true,
                error: None,
                used: true,
                question: None,
            }],
            speaker: Some(Others),
            stable: "¿Y por qué quieres".into(),
            tentative: " cambiar de".into(),
            ..Default::default()
        };
    }),
    // MicroPrompt: Labs › Assistant while Ollama is closed.
    ("teleprompter-assistant-off", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.labs.teleprompter = true;
        ui.settings.labs.assistant = true;
        ui.pane = Pane::Teleprompter;
        ui.platform.teleprompter_assistant.status =
            crate::teleprompter::assistant::Status::NotRunning;
    }),
    // MicroPrompt: before a session with Labs › Assistant on: the
    // Teleprompter's shortcut and the assistant's two.
    ("teleprompter-assistant-idle", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.labs.teleprompter = true;
        ui.settings.labs.assistant = true;
        ui.settings.labs.assistant_suggest_hotkey = serde_json::from_value(serde_json::json!({
            "keys": [
                { "modifier": "control", "side": "any" },
                { "modifier": "option", "side": "any" },
                { "code": 1, "label": "S" },
            ]
        }))
        .ok();
        ui.pane = Pane::Teleprompter;
        ui.platform.teleprompter_assistant = crate::teleprompter::assistant::View {
            status: crate::teleprompter::assistant::Status::Running,
            models: vec![crate::teleprompter::assistant::Model {
                name: "gemma4:12b".into(),
                thinks: true,
            }],
            ..Default::default()
        };
    }),
    // MicroPrompt: a multi-page Markdown script, opened to read.
    ("teleprompter-script", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.labs.teleprompter = true;
        ui.pane = Pane::Teleprompter;
        ui.platform.teleprompter_script.script = Some(crate::teleprompter::script::Script {
            name: "Presentación.md".into(),
            text: SCRIPT_FIXTURE.into(),
            markdown: true,
        });
    }),
    // MicroPrompt: an interview script by sections, a question ticked off.
    ("teleprompter-interview", |ui| {
        use crate::teleprompter::{interview::Interview, script::Script};
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.labs.teleprompter = true;
        ui.settings.labs.interview = true;
        ui.pane = Pane::Teleprompter;
        let script = Script {
            name: "guion.md".into(),
            text: INTERVIEW_FIXTURE.into(),
            markdown: true,
        };
        let mut interview = Interview::read(&script).expect("the fixture has sections");
        interview.documents[0] = Some(Script {
            name: "cv.md".into(),
            ..Script::default()
        });
        let questions = interview.sections.len() - 1;
        let first = interview.sections[questions].body.find('[').unwrap_or(0);
        interview.tick(questions, first, true);
        let pane = &mut ui.platform.teleprompter_script;
        pane.section = questions;
        pane.interview = Some(interview);
        pane.script = Some(script);
    }),
    ("teleprompter-interview-stories", |ui| {
        use crate::teleprompter::{interview::Interview, script::Script};
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.labs.teleprompter = true;
        ui.settings.labs.interview = true;
        ui.pane = Pane::Teleprompter;
        let script = Script {
            name: "guion.md".into(),
            text: INTERVIEW_FIXTURE.into(),
            markdown: true,
        };
        let pane = &mut ui.platform.teleprompter_script;
        pane.section = 1;
        pane.interview = Interview::read(&script);
        pane.script = Some(script);
    }),
    ("mac-activity", |ui| {
        *ui = mac_fixture();
        ui.platform.developer = true;
        ui.activity = activity::ActivityPane::fixture();
        ui.pane = Pane::Activity;
    }),
    // Toggle's cancel shortcut under the shortcut mode, on its default
    // (Escape), and on Wayland, where the desktop binds it.
    ("toggle-cancel", |ui| {
        *ui = mac_fixture();
        ui.settings.shortcut_mode = crate::dictation::ShortcutMode::Toggle;
    }),
    ("portal-toggle-cancel", |ui| {
        *ui = portal_fixture();
        ui.settings.shortcut_mode = crate::dictation::ShortcutMode::Toggle;
    }),
    ("mac-settings-sides", |ui| {
        *ui = mac_fixture();
        ui.advanced = vec![Pane::General];
        ui.settings.dictation_hotkey = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(crate::app_settings::ModifierSide::Left),
                ..HotkeyModifiers::default()
            },
            key: None,
        };
    }),
    // The first run on macOS: setting up Hex, each permission state, the
    // practice game, and Hex is ready.
    ("mac-onboarding", |ui| {
        mac_first_run(ui, onboarding::MacStep::Setup, Page::Download)
    }),
    ("mac-onboarding-permissions", |ui| {
        mac_first_run(ui, onboarding::MacStep::Setup, Page::Permissions)
    }),
    ("mac-onboarding-denied", |ui| {
        mac_first_run(ui, onboarding::MacStep::SetupDenied, Page::Permissions)
    }),
    ("mac-onboarding-stale", |ui| {
        mac_first_run(ui, onboarding::MacStep::SetupStale, Page::Permissions)
    }),
    ("mac-onboarding-restart", |ui| {
        mac_first_run(ui, onboarding::MacStep::SetupRestart, Page::Permissions)
    }),
    ("mac-onboarding-try", |ui| {
        mac_first_run(ui, onboarding::MacStep::TryIt, Page::Try)
    }),
    ("mac-onboarding-reopen", |ui| {
        mac_first_run(ui, onboarding::MacStep::TryItReopen, Page::Try)
    }),
    ("mac-onboarding-ready", |ui| {
        mac_first_run(ui, onboarding::MacStep::Done, Page::Ready)
    }),
];

/// The tour state a Linux first-run scene starts from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstRun {
    /// Nothing installed; the recommended model just started downloading.
    Fresh,
    /// The download is a third of the way.
    Downloading,
    /// The download stopped.
    Failed,
    /// The model is installed.
    Ready,
    /// The model is installed, and the desktop asks before Hex's first
    /// paste (GNOME and KDE on Wayland).
    PasteAccess,
}

fn first_run(ui: &mut Ui, state: FirstRun, page: Page) {
    *ui = portal_fixture();
    onboarding::fixture(ui, crate::desktop::onboarding::Step::Setup, true);
    match state {
        FirstRun::Fresh => onboarding::set_downloaded(ui, 8),
        FirstRun::Downloading => onboarding::set_downloaded(ui, 31),
        FirstRun::Failed => {
            if let Some(view) = &mut ui.view {
                view.transcription.preparing = None;
                view.transcription.error = Some("The connection was reset.".into());
            }
        }
        FirstRun::Ready | FirstRun::PasteAccess => {
            ui.picker.installed = Some(vec![ui.settings.transcription.model]);
            if let Some(view) = &mut ui.view {
                view.transcription.preparing = None;
            }
        }
    }
    if state == FirstRun::PasteAccess {
        ui.platform.paste = Some(paste_access::PasteAccess::Ask);
    }
    onboarding::poll(ui);
    onboarding_kit::live::fixture(ui, page);
}

fn mac_first_run(ui: &mut Ui, step: onboarding::MacStep, page: Page) {
    *ui = mac_fixture();
    onboarding::mac_fixture(ui, step);
    onboarding_kit::live::fixture(ui, page);
}

/// The Annotate pane with captures `enabled`, a 3-day video retention, and
/// `adjust` applied to what the host shows.
#[cfg(hex_capture)]
fn scrawl_fixture(ui: &mut Ui, enabled: bool, adjust: fn(&mut platform::ScrawlView)) {
    *ui = mac_fixture();
    ui.settings.capture.enabled = enabled;
    ui.settings.capture.media_days = Some(3);
    let mut view = platform::ScrawlView::preview();
    adjust(&mut view);
    ui.platform.scrawl = Some(view);
    ui.pane = Pane::Annotate;
}

pub(super) fn mac_fixture() -> Ui {
    let mut ui = fixture(DesktopShortcuts::Registered);
    ui.platform = platform::PlatformState::preview(platform::Capabilities::MACOS);
    ui.host = platform::PlatformHost::Fixture(Vec::new());
    ui.settings.dictation_hotkey = AppSettings::default().dictation_hotkey;
    ui.settings.paste_with_shift = false;
    ui.settings.meetings.enabled = true;
    ui.settings.meetings.summaries = crate::app_settings::MeetingSummaries::Mac;
    ui.settings.meetings.providers = vec![
        crate::app_settings::MeetingProviderChoice {
            key: "com.tinyspeck.slackmacgap".into(),
            label: "Slack huddles".into(),
            policy: crate::app_settings::MeetingPolicy::AlwaysRecord,
        },
        crate::app_settings::MeetingProviderChoice {
            key: "problems.kitlangton.com".into(),
            label: "problems.kitlangton.com".into(),
            policy: crate::app_settings::MeetingPolicy::NeverOffer,
        },
    ];
    ui.settings.experimental.hex_microphone = true;
    ui.microphones = Ok(crate::audio::InputDevices {
        names: vec![
            "Kit Pods".into(),
            "MacBook Pro Microphone".into(),
            "Studio Display Microphone".into(),
        ],
        system_default: Some("Kit Pods".into()),
        default_is_hex_microphone: false,
        ..Default::default()
    });
    ui.meetings = meetings::MeetingsPane::fixture();
    ui
}

/// Meetings › Settings with Summaries set to OpenCode, a model saved, and
/// `models` as the OpenCode catalog.
fn summary_model_scene(ui: &mut Ui, models: models::Models) {
    *ui = mac_fixture();
    ui.settings.meetings.summaries = crate::app_settings::MeetingSummaries::OpenCode;
    ui.settings.meetings.opencode_model = Some("anthropic/claude-sonnet-4-5".into());
    ui.models = models;
    ui.meetings.view = meetings::View::Settings;
    ui.pane = Pane::Meetings;
}

fn shortcut_row(ui: &mut Ui, capture: Capture) {
    *ui = mac_fixture();
    ui.settings.dictation_hotkey = HotkeyBinding {
        modifiers: HotkeyModifiers {
            option: Some(ModifierSide::Left),
            ..HotkeyModifiers::default()
        },
        key: None,
    };
    ui.capture = capture;
}

fn listening(message: Option<&'static str>, held: HotkeyModifiers) -> Capture {
    Capture::Listening {
        target: ShortcutTarget::Dictation,
        message,
        held,
    }
}

pub(super) fn portal_fixture() -> Ui {
    fixture(DesktopShortcuts::Portal(vec![
        DesktopGrantedShortcut {
            name: "Dictation".into(),
            trigger: Some("F12".into()),
            unconfirmed: false,
        },
        DesktopGrantedShortcut {
            name: "Paste last dictation".into(),
            trigger: Some("Ctrl+Alt+V".into()),
            unconfirmed: false,
        },
    ]))
}

/// Deterministic settings and service state matching a typical Wayland
/// desktop with the shortcuts portal.
pub(super) fn fixture(shortcuts: DesktopShortcuts) -> Ui {
    crate::desktop::i18n::apply(Some("en"));
    let settings = AppSettings {
        dictation_hotkey: HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: Some(HotkeyKey::named("f12")),
        },
        transcription: crate::transcription::models::TranscriptionSelection {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            language: "en".into(),
        },
        paste_with_shift: true,
        ..AppSettings::default()
    };
    let mut ui = Ui::with(settings, Service::Fixture(Vec::new()));
    ui.models = models::Models::fixture();
    ui.picker.installed = Some(vec![TranscriptionModelId::ParakeetUnifiedEnglish]);
    ui.view = Some(ServiceView {
        listener: DesktopListenerSnapshot {
            running: true,
            status: "Enabled".into(),
            warning: None,
            shortcuts,
            shortcuts_configurable: false,
        },
        operation_error: None,
        transcription: DesktopTranscriptionSnapshot {
            downloaded_bytes: 0,
            error: None,
            preparation_stage: None,
            selection: ui.settings.transcription.clone(),
            preparing: None,
        },
        update_status: DesktopUpdateStatus::Current,
        update_notes: None,
        update_blocked: None,
        editing: false,
    });
    ui
}

/// Labs with a few labs on: every store scene's fixture, so a lab reads
/// the same in each.
fn labs_with_some_on(ui: &mut Ui) {
    *ui = mac_fixture();
    for feature in [
        crate::labs::Feature::LiveText,
        crate::labs::Feature::SmartSpacing,
        crate::labs::Feature::TapOrHold,
    ] {
        feature.set(&mut ui.settings, true);
    }
    #[cfg(hex_capture)]
    {
        ui.platform.scrawl = Some(platform::ScrawlView::preview());
    }
    ui.labs_open = true;
}

/// The `teleprompter-interview` scenes' script: the interview prototype's example
/// (`tools/microprompt-interview`), all made up.
const INTERVIEW_FIXTURE: &str = "# Guion de entrevista (EJEMPLO FICTICIO: Alex García, backend en Rutalia)

## Sobre mí

- Backend 7 años: Python y Go, logística y pagos
- Ahora en Pagaflux: lidero 4 personas, plataforma de cobros
- Me gusta medir: SLO, latencia, coste por pedido
- Busco producto con impacto físico: rutas, entregas reales

## Historias y proyectos

### Migración de cobros a eventos (Pagaflux)

- Situación: cobros síncronos, caídas en picos de Black Friday
- Qué hice: diseñé cola con Kafka y reintentos idempotentes
- Resultado: -40 % incidencias, cero cobros duplicados en 18 meses

### Optimizador de rutas (TransLeón, 2019–2021)

- Situación: rutas a mano, 12 % de entregas tarde
- Qué hice: servicio en Python con OR-Tools, ventanas horarias
- Resultado: entregas tarde del 12 % al 5 %

### Conflicto con producto por un plazo (Pagaflux)

- Situación: fecha imposible para una nueva pasarela
- Qué hice: propuse versión mínima y fases con datos de riesgo
- Resultado: lanzamos a tiempo la fase 1, sin horas extra

## Preguntas previstas

### Háblame de ti
También: preséntate / resume tu trayectoria / cuéntame tu experiencia / tu perfil

- 7 años backend, Python y Go
- Logística (TransLeón) y pagos (Pagaflux)
- Lidero 4 personas; me obsesiona medir

### ¿Por qué quieres trabajar en Rutalia?
También: qué te atrae de la empresa / por qué nosotros / qué te ha llamado la atención de la compañía

- Vuelvo a logística: mi optimizador de rutas
- Vuestra migración a eventos = lo que hice en pagos
- Producto con impacto visible: entregas reales

### ¿Por qué quieres dejar tu trabajo actual?
También: por qué cambias de empresa / qué buscas en un cambio / por qué te vas

- Plataforma de cobros ya madura y estable
- Quiero problemas de optimización otra vez
- Nada negativo: buen equipo, crecí mucho

### Cuéntame un proyecto del que estés orgulloso
También: tu mayor logro / un proyecto que lideraste / algo que salió muy bien

- Cobros a eventos con Kafka
- -40 % incidencias, cero duplicados
- Lideré diseño y formé al equipo

### ¿Cuál es tu mayor debilidad?
También: punto débil / qué mejorarías de ti / área de mejora

- Me cuesta delegar lo crítico
- Lo trabajo: guardias rotativas, revisiones por pares
- Ya delego despliegues completos

### ¿Cuáles son tus expectativas salariales?
También: cuánto quieres cobrar / qué sueldo buscas / rango salarial / cuánto ganas

- Rango 58–64 k brutos
- Flexible si hay remoto y formación
- Primero entender el puesto completo

## Preguntas que haré

- ¿Cómo medís el éxito del equipo en 6 meses?
- ¿Qué parte de la migración a eventos queda?
- ¿Cómo son las guardias y el remoto?
";

/// The `teleprompter-script` scene's script: several pages of Markdown.
const SCRIPT_FIXTURE: &str = "# Bienvenida

Buenos días a **todos** y gracias por venir. Hoy os quiero contar
cómo hemos llegado hasta aquí y qué viene *después*.

## Por qué empezamos

Hace un año leíamos los discursos en papel. Perdíamos el hilo cada vez
que levantábamos la vista, y el público lo notaba.

> Un buen guion se lee sin que se note que lo estás leyendo.

## Lo que hemos aprendido

- La letra tiene que ser grande.
- Las frases cortas se leen mejor.
- Hay que poder volver atrás sin perder el sitio.
  - Con la rueda del ratón.
  - Con `Re Pág` y `Av Pág`.

1. Primero, preparar el texto.
2. Después, ensayarlo en voz alta.
3. Por último, leerlo con calma.

| Ensayo | Minutos | Errores |
|--------|--------:|--------:|
| Lunes  | 12      | 5       |
| Jueves | 9       | 1       |

```
hex teleprompter start
```

---

## Lo que viene

El siguiente paso es que el guion siga tu voz: mientras hablas, la línea
que estás leyendo se queda en el centro de la pantalla y el resto sube a
su ritmo, sin tocar nada.

Queremos que funcione en local, sin enviar el audio a ningún sitio, y
que tarde lo menos posible en reaccionar.

## Cierre

Gracias por escucharme. Si tenéis preguntas, este es un buen momento
para hacerlas; y si no, nos vemos en la próxima reunión.
";
