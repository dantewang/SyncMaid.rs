//! One destination's own lines out of the log file.
//!
//! **A real top-level window, not a dialog inside the main one.** The point of it is to be read
//! *beside* the task list — while a run goes, while a folder is being checked in Explorer — and
//! a modal that greys out the thing it is explaining does the opposite. `Root`'s dialog layer
//! only does modal, so this is its own frame: non-modal, resizable, and with a taskbar entry.
//!
//! **There is only ever one.** Clicking another destination's status text re-points the window
//! already open rather than stacking a second; the caller keeps the handle and hands it back in.
//! Two log windows would be two things to close and no way to tell which said what.
//!
//! Nothing lives in it that is not on disk. It reads the file on open and on Refresh, so what it
//! shows is a snapshot — the button says so, and "Open log file" is the way out to the whole
//! thing in the user's own editor.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Entity, FontWeight, ScrollHandle,
    SharedString, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use gpui_component::{
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, Root, Sizable as _,
};
use uuid::Uuid;

use crate::components::Glyph;
use crate::platform::shell_open;
use crate::services::logging::{self, DestinationTag, LogLine};
use crate::strings;

/// How many of a destination's lines the window shows. Enough to see a failure and the runs
/// around it; past that the file itself is the better tool, which is what the button is for.
const TAIL_LINES: usize = 20;

/// The window's opening size, and the smallest shape its content still reads at.
const WINDOW_SIZE: (f32, f32) = (720., 420.);
const WINDOW_MIN_SIZE: (f32, f32) = (420., 260.);

/// Which destination's lines to show, and where to read them from.
#[derive(Debug, Clone)]
pub struct LogRequest {
    pub task_id: Uuid,
    pub task_name: String,
    pub destination_name: String,
    pub log_path: PathBuf,
}

impl LogRequest {
    /// The frame's own title. It names the destination, because in the taskbar and in Alt-Tab
    /// this window has nothing else to say which of them it is about.
    fn window_title(&self) -> String {
        strings::log_viewer_window_title_format(&self.destination_name)
    }

    fn tag(&self) -> DestinationTag {
        DestinationTag::new(self.task_id, &self.task_name, &self.destination_name)
    }
}

/// The open window, kept by whoever opened it so that the next click re-points this one.
pub struct OpenLogViewer {
    window: WindowHandle<Root>,
    view: Entity<LogViewer>,
}

impl OpenLogViewer {
    /// Re-points the window at another destination and brings it forward.
    ///
    /// `false` when the user has since closed it: the handle is stale, and the caller opens a
    /// fresh window rather than letting the click do nothing.
    pub fn show(&self, request: LogRequest, cx: &mut App) -> bool {
        self.window
            .update(cx, |_, window, cx| {
                window.set_window_title(&request.window_title());
                self.view.update(cx, |view, cx| view.point_at(request, cx));
                // Open but behind the main window is indistinguishable from nothing having
                // happened, which is what the click would look like.
                window.activate_window();
            })
            .is_ok()
    }
}

/// Opens the window. `None` when it could not be opened, which leaves the status text inert
/// rather than taking anything else down with it.
pub fn open(request: LogRequest, cx: &mut App) -> Option<OpenLogViewer> {
    let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);
    let title = request.window_title();
    let captured: Rc<RefCell<Option<Entity<LogViewer>>>> = Rc::default();

    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(title.into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                // Resizable, unlike the mirror-delete window: this one holds lines of arbitrary
                // length written by the OS, and widening it is the only way to read one whole.
                is_resizable: true,
                window_min_size: Some(size(px(WINDOW_MIN_SIZE.0), px(WINDOW_MIN_SIZE.1))),
                kind: WindowKind::Normal,
                ..Default::default()
            },
            {
                let captured = Rc::clone(&captured);
                move |window, cx| {
                    let view = cx.new(|_| LogViewer::new(request));
                    *captured.borrow_mut() = Some(view.clone());
                    // Wrapped in `Root` like every other window of ours: the library's
                    // components expect its layers to exist.
                    let any: gpui::AnyView = view.into();
                    cx.new(|cx| Root::new(any, window, cx))
                }
            },
        )
        .ok()?;

    let view = captured.borrow_mut().take()?;
    Some(OpenLogViewer { window, view })
}

/// See the module docs.
pub struct LogViewer {
    request: LogRequest,
    /// The lines as they were last read, oldest first.
    lines: Vec<LogLine>,
    /// Whether there is a file to hand to the editor at all. Read alongside the lines rather
    /// than at render time, which happens every frame.
    log_exists: bool,
    scroll: ScrollHandle,
}

impl LogViewer {
    fn new(request: LogRequest) -> Self {
        let mut view = Self {
            request,
            lines: Vec::new(),
            log_exists: false,
            scroll: ScrollHandle::new(),
        };
        view.reread();
        view
    }

    /// Shows another destination's lines in the window that is already open.
    fn point_at(&mut self, request: LogRequest, cx: &mut Context<Self>) {
        self.request = request;
        self.reread();
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.reread();
        cx.notify();
    }

    /// The one place the file is read. Everything on screen comes from here.
    fn reread(&mut self) {
        self.lines =
            logging::tail_matching(&self.request.log_path, &self.request.tag(), TAIL_LINES);
        self.log_exists = self.request.log_path.exists();
        // The newest line is the one the status text was summarising, so it is the one that has
        // to be in view. Oldest first is the right order to read a history in and the wrong end
        // to open it at. Resolved in the next prepaint, against sizes measured there, so this
        // works on the very first frame as well as on a Refresh.
        self.scroll.scroll_to_bottom();
    }

    /// Hands the whole file to whatever the user opens `.log` files with.
    fn open_log_file(&self) {
        if !shell_open::open(&self.request.log_path) {
            // Said out loud rather than swallowed: the button visibly did nothing, and this
            // line is the only record of why.
            tracing::warn!(
                path = %self.request.log_path.display(),
                "the shell would not open the log file"
            );
        }
    }

    /// `Photos → NAS backup`. Which destination these lines are about — on screen rather than
    /// only in the title bar, because it changes under the user when another status is clicked.
    fn pair(&self) -> String {
        format!(
            "{} → {}",
            self.request.task_name, self.request.destination_name
        )
    }
}

impl Render for LogViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap(px(12.))
            .p(px(20.))
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_heading(cx))
            .child(self.render_lines(cx))
            .child(self.render_footer(cx))
    }
}

impl LogViewer {
    fn render_heading(&self, cx: &App) -> impl IntoElement {
        v_flex()
            .gap(px(2.))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Icon::new(Glyph::Log)
                            .size(px(18.))
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(strings::log_viewer_title()),
                    ),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.pair()),
            )
            // The task's id. Two tasks may share a name, so this is the only thing on screen
            // that says which one these lines came from — and it is what the lines themselves
            // are matched on. Monospace and unlabelled: it reads as the identifier it is.
            .child(
                div()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().muted_foreground.opacity(0.7))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.request.task_id.to_string()),
            )
    }

    /// The lines, as a timestamp column and the message beside it.
    ///
    /// The tag is gone from every row: the heading above names the task and the destination
    /// once, and repeating them twenty times would leave no width for what the rows say.
    fn render_lines(&self, cx: &App) -> impl IntoElement {
        let rows = self
            .lines
            .iter()
            .map(|line| self.render_line(line, cx))
            .collect::<Vec<_>>();

        // The scrolling box sits inside a relative wrapper because `Scrollbar` lays itself out
        // absolutely over its parent — as a child of the scroll area it would scroll away with
        // the content it is measuring.
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .child(
                div()
                    .id("log-lines")
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .size_full()
                    // Vertical only, and messages wrap. A failure line ends in the engine's own
                    // sentence, which is the half being looked up — clipping it off the right
                    // edge hides exactly what the window was opened for.
                    .overflow_y_scroll()
                    .p(px(10.))
                    .text_sm()
                    .font_family(cx.theme().mono_font_family.clone())
                    .when(self.lines.is_empty(), |element| {
                        element.child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(strings::log_viewer_empty()),
                        )
                    })
                    .children(rows),
            )
            // `Always` rather than the theme's default, which fades the bar out two seconds
            // after the last scroll. It still hides itself when everything fits, so what is
            // left is exactly the signal wanted: a bar means there is more above or below.
            .child(Scrollbar::vertical(&self.scroll).scrollbar_show(ScrollbarShow::Always))
    }

    fn render_line(&self, line: &LogLine, cx: &App) -> impl IntoElement {
        // A failure is what these twenty lines are usually being read for, so it carries the
        // same colour it has in the row that sent the user here.
        let color = match line.level.as_str() {
            "ERR" => cx.theme().danger,
            "WRN" => cx.theme().warning,
            _ => cx.theme().foreground,
        };

        h_flex()
            // Top, not centre: a wrapped message must not drag its timestamp down the rows.
            .items_start()
            .gap(px(10.))
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(line.time.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(color)
                    .child(SharedString::from(line.message.clone())),
            )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap(px(10.))
            .child(
                // The path itself, so opening the file by hand stays possible even when nothing
                // is registered to open it.
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.request.log_path.display().to_string()),
            )
            .child(
                Button::new("log-refresh")
                    .icon(Glyph::Sync)
                    .label(strings::log_viewer_refresh())
                    .outline()
                    .small()
                    .tooltip(strings::log_viewer_refresh_tip())
                    .on_click(cx.listener(|view, _, _, cx| view.refresh(cx))),
            )
            .child(
                Button::new("log-open-file")
                    .icon(Glyph::OpenExternal)
                    .label(strings::log_viewer_open_log_file())
                    .primary()
                    .small()
                    .tooltip(strings::log_viewer_open_log_file_tip())
                    // Nothing to open is a disabled button, not a click that quietly fails.
                    .disabled(!self.log_exists)
                    .on_click(cx.listener(|view, _, _, _| view.open_log_file())),
            )
    }
}
