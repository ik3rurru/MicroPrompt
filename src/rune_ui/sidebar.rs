//! The sidebar: every pane in navigation order, their items, Quit on Linux,
//! and the footer with the version and the update check.

use super::*;

static SETTINGS_ICON: Icon = Icon {
    name: "hex-settings",
    view: 24.0,
    body: include_str!("../../assets/icons/settings.svg"),
};
static MODES_ICON: Icon = Icon {
    name: "hex-modes",
    view: 24.0,
    body: include_str!("../../assets/icons/modes.svg"),
};
static OUTPUT_ICON: Icon = Icon {
    name: "hex-output",
    view: 24.0,
    body: include_str!("../../assets/icons/output.svg"),
};
static HISTORY_ICON: Icon = Icon {
    name: "hex-history",
    view: 24.0,
    body: include_str!("../../assets/icons/history.svg"),
};
static ACTIVITY_ICON: Icon = Icon {
    name: "hex-activity",
    view: 24.0,
    body: include_str!("../../assets/icons/activity.svg"),
};
static HUD_LAB_ICON: Icon = Icon {
    name: "hex-hud-lab",
    view: 24.0,
    body: include_str!("../../assets/icons/hud-lab.svg"),
};
pub(super) static VOICE_FILTER_ICON: Icon = Icon {
    name: "hex-voice-filter",
    view: 24.0,
    body: include_str!("../../assets/icons/voice-filter.svg"),
};
static MEETINGS_ICON: Icon = Icon {
    name: "hex-meetings",
    view: 24.0,
    body: include_str!("../../assets/icons/meetings.svg"),
};
static TELEPROMPTER_ICON: Icon = Icon {
    name: "hex-teleprompter",
    view: 24.0,
    body: include_str!("../../assets/icons/teleprompter.svg"),
};
#[cfg(hex_capture)]
pub(super) static SCRAWL_ICON: Icon = Icon {
    name: "hex-scrawl",
    view: 24.0,
    body: include_str!("../../assets/icons/scrawl.svg"),
};

/// Every pane, in navigation order. Which ones show depends on the OS's
/// [`platform::Capabilities`] and opt-ins ([`Pane::shown`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Pane {
    /// Dictation, output, and application basics.
    #[default]
    General,
    /// What Hex does to every dictation and in some apps, read-only: the
    /// person's agent edits it.
    Output,
    /// The old per-app editor, kept for its scenes until it is removed.
    Modes,
    /// Recorded calls and their settings, or the opt-in (macOS beta).
    Meetings,
    History,
    /// Annotate (Scrawl's captures inside Hex): the switch, shortcuts,
    /// permission, transcriber, folder, retention, and Scrawl's own app.
    /// Dev builds, macOS and Linux only.
    #[cfg(hex_capture)]
    Annotate,
    /// The Hex Microphone's driver, tuning, and try-it demo (macOS, while
    /// its Labs toggle is on).
    VoiceFilter,
    /// MicroPrompt: the live subtitles of a session (Labs › Teleprompter).
    Teleprompter,
    /// The dictation indicator's tuning scene (Playground only).
    HudLab,
    /// Legacy event-log inspector scene (Playground only).
    Activity,
}

impl Pane {
    #[cfg(all(test, target_os = "linux"))]
    pub(super) const ALL: &[Pane] = &[
        Pane::General,
        Pane::Output,
        Pane::Modes,
        Pane::Meetings,
        Pane::History,
        #[cfg(hex_capture)]
        Pane::Annotate,
        Pane::VoiceFilter,
        Pane::Teleprompter,
        Pane::HudLab,
        Pane::Activity,
    ];

    pub(super) fn shown(self, ui: &Ui) -> bool {
        let caps = ui.platform.caps;
        match self {
            Pane::Modes | Pane::HudLab | Pane::Activity => false,
            Pane::Meetings => caps.meetings && meetings::available(ui),
            // On from Labs (dev builds), or while its driver is installed.
            Pane::VoiceFilter => caps.fun && fun::running(ui),
            Pane::Teleprompter => labs::teleprompter::shown(ui),
            #[cfg(hex_capture)]
            Pane::Annotate => ui.platform.scrawl.is_some(),
            _ => true,
        }
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Pane::General => tr("General"),
            Pane::Output => tr("Output"),
            Pane::Modes => tr("Modes"),
            Pane::History => tr("History"),
            Pane::Meetings => tr("Meetings"),
            #[cfg(hex_capture)]
            Pane::Annotate => tr("Annotate"),
            Pane::VoiceFilter => tr("Voice Filter"),
            Pane::Teleprompter => "Teleprompter",
            Pane::HudLab => "HUD Lab",
            Pane::Activity => tr("Activity"),
        }
    }

    pub(super) fn key(self) -> &'static str {
        match self {
            Pane::General => "nav-general",
            Pane::Output => "nav-output",
            Pane::Modes => "nav-modes",
            Pane::History => "nav-history",
            Pane::Meetings => "nav-meetings",
            #[cfg(hex_capture)]
            Pane::Annotate => "nav-annotate",
            Pane::VoiceFilter => "nav-voice-filter",
            Pane::Teleprompter => "nav-teleprompter",
            Pane::HudLab => "nav-hud-lab",
            Pane::Activity => "nav-activity",
        }
    }

    pub(super) fn icon(self) -> &'static Icon {
        match self {
            Pane::General => &SETTINGS_ICON,
            Pane::Output => &OUTPUT_ICON,
            Pane::Modes => &MODES_ICON,
            Pane::History => &HISTORY_ICON,
            Pane::Meetings => &MEETINGS_ICON,
            #[cfg(hex_capture)]
            Pane::Annotate => &SCRAWL_ICON,
            Pane::VoiceFilter => &VOICE_FILTER_ICON,
            Pane::Teleprompter => &TELEPROMPTER_ICON,
            Pane::HudLab => &HUD_LAB_ICON,
            Pane::Activity => &ACTIVITY_ICON,
        }
    }
}

/// macOS draws the traffic lights over the sidebar (Rune's window has a
/// transparent, full-size title bar), so the navigation starts below them.
const SIDEBAR_TOP: f32 = if cfg!(target_os = "macos") {
    40.0
} else {
    12.0
};

/// Sidebar rows follow OpenCode Desktop's settings navigation: 28 pt rows
/// with a 4 pt gap, so each row's highlight stays its own.
pub(super) const NAV_ROW_HEIGHT: f32 = 28.0;
pub(super) const NAV_ROW_GAP: f32 = 4.0;

/// On macOS the sidebar and the pane header stand in for the title bar the
/// window draws none of: pressing their background moves the window.
pub(super) fn title_bar(element: El) -> El {
    if cfg!(target_os = "macos") {
        element.window_drag()
    } else {
        element
    }
}

/// The title bar of the content panel: the title, a view switch that stays
/// beside it, and the pane's actions on the right. The title is the pane's
/// level 1 heading. Actions that don't fit beside the title wrap onto more
/// lines under it, still at the right, so none is pushed out of a narrow
/// window; one line is 52 pt tall, as before.
pub(super) fn pane_header(p: Paint, title: &str, switch: Option<El>, actions: Vec<El>) -> El {
    title_bar(row())
        .key("pane-header")
        .min_h(52.0)
        .px(METRICS.gutter)
        .py(10.0)
        .gap(8.0)
        .flex_wrap()
        .justify_end()
        .center_y()
        .child(
            text(title)
                .key("pane-title")
                .role("heading")
                .level(1)
                .font(p.k.ty.body_medium)
                .color(p.text())
                .shrink()
                .truncate(),
        )
        .children(switch.map(|switch| switch.ml(8.0)))
        .child(el().key("header-spacer").grow(1.0))
        .children(actions)
}

/// Below this window width the sidebar shows its icons alone, each naming
/// itself on hover, so the pane keeps the room. macOS's window is a fixed
/// size and never narrows to it.
pub(super) const COMPACT_BELOW: f32 = 860.0;

/// The narrowest window: the compact sidebar leaves the panes room for their
/// rows down to here. macOS keeps the wide sidebar's minimum.
pub(super) const MIN_WINDOW_WIDTH: f32 = if cfg!(target_os = "macos") {
    COMPACT_BELOW
} else {
    640.0
};

/// The compact sidebar: one 28 pt icon column between 8 pt margins.
const COMPACT_WIDTH: f32 = NAV_ROW_HEIGHT + 16.0;

/// Whether the window is narrow enough for the compact sidebar. It reads
/// the window's own width, which the sidebar never changes, so it settles
/// in the first frame.
pub(super) fn compact(cx: &Cx<Ui>) -> bool {
    !cfg!(target_os = "macos")
        && cx
            .layout_with("window", |rect| rect[2] < COMPACT_BELOW)
            .unwrap_or(false)
}

pub(super) fn sidebar(ui: &Ui, p: Paint, compact: bool) -> El {
    let mut nav = lists::source_list(col().key("nav"));
    for pane in labs::main_panes(ui) {
        nav = nav.child(nav_slot(
            p,
            pane,
            ui.pane == pane && !ui.labs_open,
            compact,
            NAV_ROW_GAP / 2.0,
            NAV_ROW_GAP / 2.0,
        ));
    }
    nav = nav.children(labs::playground_nav(ui, p, compact));
    nav = nav.child(labs::nav(ui, p, compact));
    let sidebar = title_bar(col())
        .key("sidebar")
        .role("navigation")
        .h_full()
        .pt(SIDEBAR_TOP - NAV_ROW_GAP / 2.0)
        .pb(12.0)
        .child(nav)
        .child(el().key("spacer").grow(1.0))
        .children(sidebar_update::control(ui, p, compact));
    if compact {
        // The update's notes and why it can't install itself are left to
        // the wide sidebar; the version and the check are one icon.
        return sidebar
            .w(COMPACT_WIDTH)
            .px(8.0)
            .children(host::QUIT_IN_WINDOW.then(|| compact_quit(p)))
            .child(compact_footer(ui, p));
    }
    sidebar
        .w(220.0)
        .pl(10.0)
        .pr(6.0)
        .children(sidebar_update::notes(ui, p))
        .children(sidebar_update::blocked(ui, p))
        .children(host::QUIT_IN_WINDOW.then(|| quit_item(p)))
        .child(footer(ui, p))
}

/// The version and, under it, a manual check or what the updater is doing.
/// Both lines keep their place in every state, so a check never moves the
/// version, and the status line is one element whose words change in place
/// rather than two that cross-fade over each other. An available update is
/// the Update button above.
fn footer(ui: &Ui, p: Paint) -> El {
    let c = &p.k.c;
    let line = |key: &str, words: &str, color| {
        row()
            .key(key)
            .h(FOOTER_LINE)
            .px(6.0)
            .center_y()
            .self_start()
            .radius(p.k.radius.sm)
            .child(
                text(words)
                    .key("label")
                    .font(p.k.ty.small)
                    .color(color)
                    .truncate(),
            )
    };
    let status = update_status(ui).map(|status| match status {
        Status::Check(words) => checks(p, line("update-status", words, c.text_muted)),
        // Quieter while it checks; the hourly check keeps the line's words.
        Status::Checking(words) => {
            line("update-status", words, p.faint()).label(tr("Checking for updates…"))
        }
        Status::Quiet(words) => line("update-status", words, p.faint()),
        Status::Command(command) => text(command)
            .key("update-command")
            .font(p.k.ty.small)
            .color(p.faint())
            .wrap()
            .px(6.0)
            .py(2.0),
    });
    col()
        .key("footer")
        .mt(8.0)
        .child(version_line(
            ui,
            p,
            line("version", &version_label(), p.faint()),
        ))
        .children(status)
}

/// What the footer's status line says under the version.
enum Status {
    /// A manual check, saying how the last one the person asked for went.
    Check(&'static str),
    Checking(&'static str),
    /// What the updater is doing, or why it doesn't.
    Quiet(&'static str),
    /// The package manager's command that updates Hex.
    Command(String),
}

fn update_status(ui: &Ui) -> Option<Status> {
    let status = ui.view.as_ref().map(|view| &view.update_status)?;
    Some(match status {
        DesktopUpdateStatus::Unavailable
        | DesktopUpdateStatus::Available
        | DesktopUpdateStatus::ReadyToRestart
        | DesktopUpdateStatus::SwitchAvailable { .. } => return None,
        // After a check the person asked for, the line says how it went
        // (#1120); the hourly check alone never changes its words.
        DesktopUpdateStatus::Current if ui.update_asked => Status::Check(tr("Hex is up to date")),
        DesktopUpdateStatus::Failed if ui.update_asked => {
            Status::Check(tr("Couldn't check for updates"))
        }
        DesktopUpdateStatus::Current | DesktopUpdateStatus::Failed => {
            Status::Check(tr("Check for updates"))
        }
        DesktopUpdateStatus::Checking => Status::Checking(if ui.update_asked {
            tr("Checking for updates…")
        } else {
            tr("Check for updates")
        }),
        DesktopUpdateStatus::Managed => Status::Quiet(tr("Managed by your organization")),
        DesktopUpdateStatus::Downloading => Status::Quiet(tr("Downloading update")),
        DesktopUpdateStatus::SourceBuild => Status::Quiet(
            if ui.build_preview.unwrap_or_else(build_kind) == BuildKind::Local {
                tr("Local build · updates off")
            } else {
                tr("Updated from your source checkout")
            },
        ),
        DesktopUpdateStatus::PackageManager => Status::Quiet(tr("Updated by your package manager")),
        DesktopUpdateStatus::PackageUpdate { command } => Status::Command(command.clone()),
    })
}

/// A footer element that checks for updates when pressed.
fn checks(p: Paint, element: El) -> El {
    let c = &p.k.c;
    element
        .role("button")
        .label(tr("Check for updates"))
        .hover(c.hover)
        .press(c.pressed)
        .focusable(true)
        .focus_ring(2.0, -2.0, c.border_focus)
        .on_press(|ui: &mut Ui| {
            ui.update_asked = true;
            ui.send(Request::Desktop(DesktopAction::CheckForUpdates));
        })
}

/// The compact footer: one icon whose tooltip gives the version and the
/// status line, and which checks for updates where the line would.
fn compact_footer(ui: &Ui, p: Paint) -> El {
    let version = version_label();
    let (about, check) = match update_status(ui) {
        Some(Status::Check(words)) => (format!("{version}\n{words}"), true),
        Some(Status::Checking(words) | Status::Quiet(words)) => {
            (format!("{version}\n{words}"), false)
        }
        Some(Status::Command(command)) => (format!("{version}\n{command}"), false),
        None => (version, false),
    };
    let item = compact_icon(p, "version", &INFO_ICON, &about).mt(8.0);
    if check { checks(p, item) } else { item }
}

/// A compact sidebar icon, a `NAV_ROW_HEIGHT` square named `label`, which
/// shows its name on hover.
pub(super) fn compact_icon(p: Paint, key: &str, glyph: &'static Icon, label: &str) -> El {
    row()
        .key(key)
        .label(label)
        .help(label)
        .h(NAV_ROW_HEIGHT)
        .center()
        .radius(p.k.radius.sm)
        .child(
            icon(glyph, 16.0)
                .key("icon")
                .color(p.k.c.icon_muted)
                .a11y_hidden(),
        )
}

static INFO_ICON: Icon = Icon {
    name: "hex-sidebar-info",
    view: 24.0,
    body: concat!(
        r#"<path d="M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
        r#"<path d="M12 16v-4M12 8h.01" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
    ),
};

/// A local or test build marks itself beside the version, quietly: an outline
/// pill with a dot, in muted theme tokens.
fn version_line(ui: &Ui, p: Paint, version: El) -> El {
    let badge = match ui.build_preview.unwrap_or_else(build_kind) {
        BuildKind::Release => return version,
        BuildKind::Local => tr("Local"),
        BuildKind::Test => tr("Test"),
    };
    let c = &p.k.c;
    row()
        .key("version-row")
        .gap(2.0)
        .center_y()
        .child(version.key("version"))
        .child(
            row()
                .key("build-badge")
                .h(16.0)
                .px(6.0)
                .gap(4.0)
                .center_y()
                .radius(8.0)
                .border(p.hair.max(0.5), c.border_muted)
                .child(
                    controls::status_dot(c.text_faint)
                        .size(5.0, 5.0)
                        .radius(2.5),
                )
                .child(
                    text(badge)
                        .key("label")
                        .font(p.k.ty.small)
                        .color(c.text_muted),
                ),
        )
}

/// What kind of build this is, for the footer's badge. The scenes preview
/// the others on a release build.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum BuildKind {
    Release,
    Local,
    Test,
}

fn build_kind() -> BuildKind {
    if crate::app_paths::is_local_build() {
        BuildKind::Local
    } else if option_env!("HEX_TEST_BUILD") == Some("1") {
        BuildKind::Test
    } else {
        BuildKind::Release
    }
}

/// The footer's lines: as tall as each other, so a check moves nothing.
pub(super) const FOOTER_LINE: f32 = 20.0;

/// Test and local builds also name their commit: main moves past a release without
/// changing the version.
fn version_label() -> String {
    // Goldens render a fixed version: a local build's names its commit.
    let version = if cfg!(test) { "0.0.0" } else { crate::VERSION };
    match option_env!("HEX_COMMIT") {
        Some(commit) => {
            let kind = if crate::app_paths::is_local_build() {
                "local"
            } else {
                "test"
            };
            tr_fill("Version {}", &format!("{version} ({kind} {commit})"))
        }
        None => tr_fill("Version {}", version),
    }
}

/// A sidebar item inside its slot. Slots tile the item column, each
/// reaching halfway into the gaps around its item, so wherever the pointer
/// is in the column, the nearest item shows its hover, and a press there
/// selects it.
fn nav_slot(p: Paint, pane: Pane, on: bool, compact: bool, above: f32, below: f32) -> El {
    col()
        .key(format!("{}-slot", pane.key()))
        // Assistive technology sees the item alone.
        .role("none")
        .pt(above)
        .pb(below)
        .hover_group()
        .on_drag(move |ui: &mut Ui, d: &mut rune::Drag, tx: &mut Tx| {
            select_on_down(ui, d, tx, pane)
        })
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| ui.select_pane(pane, tx))
        .child(nav_item(p, pane, on, compact))
}

/// Switch on pointer down, like a native sidebar; the press keeps keyboard
/// activation.
fn select_on_down(ui: &mut Ui, d: &mut rune::Drag, tx: &mut Tx, pane: Pane) {
    if d.phase == rune::Phase::Down {
        ui.select_pane(pane, tx);
    }
}

fn nav_item(p: Paint, pane: Pane, on: bool, compact: bool) -> El {
    let c = &p.k.c;
    // A row of the navigation list, selected while its pane shows.
    let mut item = row()
        .key(pane.key())
        .role("listitem")
        .label(pane.title())
        .h(NAV_ROW_HEIGHT)
        .px(6.0)
        .gap(8.0)
        .center_y()
        .radius(p.k.radius.sm)
        .press(c.pressed)
        .on_drag(move |ui: &mut Ui, d: &mut rune::Drag, tx: &mut Tx| {
            select_on_down(ui, d, tx, pane)
        })
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| ui.select_pane(pane, tx))
        // The slot's hover, drawn on the item.
        .child(
            el().key("hover")
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .radius(p.k.radius.sm)
                .fill(c.hover)
                .group_opacity(0.0, 1.0)
                .a11y_hidden(),
        )
        .child(
            icon(pane.icon(), 16.0)
                .key("icon")
                .color(if on { c.icon_base } else { c.icon_muted })
                .a11y_hidden(),
        )
        .children((!compact).then(|| {
            text(pane.title())
                .key("label")
                .font(p.k.ty.ui)
                .color(if on { c.text_base } else { c.text_muted })
                .truncate()
                .a11y_hidden()
        }));
    if compact {
        item = item.help(pane.title());
    }
    if on {
        item = item.fill(c.layer_02);
    }
    lists::source_item(item, on)
}

/// [`quit_item`] in the compact sidebar.
fn compact_quit(p: Paint) -> El {
    let c = &p.k.c;
    compact_icon(p, "quit", &icons::POWER, tr("Quit Hex"))
        .role("button")
        .mt(NAV_ROW_GAP)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(|ui: &mut Ui| ui.quit())
        .focusable(true)
        .focus_ring(2.0, -2.0, c.border_focus)
}

/// Linux has no tray: the window's way to stop Hex. Closing the window
/// keeps Hex dictating.
fn quit_item(p: Paint) -> El {
    let c = &p.k.c;
    row()
        .key("quit")
        .role("button")
        .label(tr("Quit Hex"))
        .h(NAV_ROW_HEIGHT)
        .px(6.0)
        .mt(NAV_ROW_GAP)
        .center_y()
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(|ui: &mut Ui| ui.quit())
        .focusable(true)
        .focus_ring(2.0, -2.0, c.border_focus)
        .child(
            text(tr("Quit Hex"))
                .key("label")
                .font(p.k.ty.ui)
                .color(c.text_muted)
                .truncate()
                .a11y_hidden(),
        )
}
