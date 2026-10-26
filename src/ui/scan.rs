//! Scan instrument. A real traversal is shown while it happens: files found,
//! bytes observed, errors skipped, throughput samples and the directory that is
//! currently being read. Nothing here is simulated; an empty chart means no
//! progress has been reported yet.

use iced::font::Weight;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::widget::{column, container, row};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Point, Rectangle, Renderer,
    Size, Theme as IcedTheme,
};

use crate::ui::app::{App, Message, SUMMARY_HEIGHT, Screen};
use crate::ui::components::*;
use crate::ui::icon::{Icon, icon};
use crate::ui::theme::{self, Theme};
use crate::ui::{fmt, view as shell};

pub fn view<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    if let Some(error) = &app.scan_error {
        return error_screen(app, &t, error);
    }
    let Some(scan) = &app.scan else {
        return idle_screen(app, &t);
    };

    let target = scan.target.to_string_lossy();
    let progress = &scan.progress;
    let elapsed = scan.started.elapsed().as_millis() as u64;
    let rate = progress
        .files
        .saturating_mul(1000)
        .checked_div(elapsed)
        .unwrap_or(0);

    let hero = column![
        label("Scanning", theme::TEXT_SM, Weight::Medium, t.ink_faint),
        label(
            scan.target
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| target.to_string()),
            28.0,
            Weight::Semibold,
            t.ink
        ),
        label(
            "Read-only traversal. space never writes, moves or removes files.",
            12.5,
            Weight::Normal,
            t.ink_muted
        ),
    ]
    .spacing(6);

    let metrics = container(
        row![
            cell("Files", fmt::count(progress.files), "found", &t),
            vline(t.hairline),
            cell("Folders", fmt::count(progress.dirs), "read", &t),
            vline(t.hairline),
            cell("Bytes", fmt::bytes_short(progress.bytes), "so far", &t),
            vline(t.hairline),
            cell(
                "Skipped",
                fmt::count(progress.errors),
                if progress.errors == 0 {
                    "none"
                } else {
                    "permission or I/O"
                },
                &t
            ),
        ]
        .height(Length::Fixed(SUMMARY_HEIGHT)),
    )
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 7.0.into(),
        },
        ..Default::default()
    });

    let chart = container(
        iced::widget::canvas(Trend {
            samples: scan.samples.clone(),
            line: t.accent,
            soft: Color {
                a: 0.16,
                ..t.accent
            },
            track: t.surface_sunken,
            hairline: t.hairline,
        })
        .width(Length::Fill)
        .height(Length::Fixed(132.0)),
    )
    .width(Length::Fill)
    .padding(10)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 7.0.into(),
        },
        ..Default::default()
    });

    let current = progress.current.clone();
    let current = fmt::short_path(&current, 92);
    let footer = row![
        column![
            label("CURRENT FOLDER", 9.5, Weight::Medium, t.ink_faint),
            mono(current, 11.0, Weight::Normal, t.ink_muted),
        ]
        .spacing(3)
        .width(Length::Fill),
        row![
            label("THROUGHPUT", 9.5, Weight::Medium, t.ink_faint),
            mono(
                format!(
                    "{} files/s · {} elapsed",
                    fmt::count(rate),
                    fmt::elapsed(elapsed)
                ),
                11.0,
                Weight::Normal,
                t.ink_muted
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        {
            let cancel: Element<'a, Message> = if scan.cancelling {
                container(label("Cancelling…", 11.5, Weight::Medium, t.ink_muted))
                    .padding(Padding::from([7.0, 14.0]))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(t.surface_sunken)),
                        border: Border {
                            color: t.hairline,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    })
                    .into()
            } else {
                outline_button(
                    row![
                        icon(Icon::Close, 12.0, t.ink_muted),
                        label("Cancel scan", 11.5, Weight::Medium, t.ink_muted)
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                    Message::CancelScan,
                    &t,
                )
                .into()
            };
            cancel
        },
    ]
    .spacing(18)
    .align_y(Alignment::Center);

    let quiet = container(
        row![
            icon(Icon::Shield, 11.0, t.ink_faint),
            label(
                "Recursive traversal of real directories. Results open automatically when the walk completes.",
                10.5,
                Weight::Normal,
                t.ink_faint
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );

    let content = column![hero, metrics, chart, footer, quiet]
        .spacing(16)
        .max_width(900.0);

    column![
        shell::app_bar(app, &t),
        container(
            container(content)
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .padding(Padding {
                    top: 36.0,
                    right: 28.0,
                    bottom: 24.0,
                    left: 28.0
                })
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::Start),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn cell<'a>(caption: &'a str, value: String, note: &'a str, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    container(
        column![
            label(caption, theme::TEXT_SM, Weight::Normal, t.ink_muted),
            row![
                mono(value, 21.0, Weight::Semibold, t.ink),
                label(note, theme::TEXT_XS + 0.5, Weight::Normal, t.ink_faint),
            ]
            .spacing(8)
            .align_y(Alignment::End),
        ]
        .spacing(7),
    )
    .width(Length::FillPortion(1))
    .height(Length::Fill)
    .padding(Padding::from([14.0, 18.0]))
    .align_y(Alignment::Center)
    .into()
}

fn idle_screen<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let body = column![
        label("Scan", theme::TEXT_SM, Weight::Medium, t.ink_faint),
        label("No scan is running", 28.0, Weight::Semibold, t.ink),
        label(
            "Choose a volume or drop a folder to start a read-only traversal.",
            12.5,
            Weight::Normal,
            t.ink_muted
        ),
        row![
            outline_button(
                row![
                    icon(Icon::ArrowLeft, 13.0, t.ink),
                    label("Volumes", 11.5, Weight::Medium, t.ink)
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                Message::Go(Screen::Drives),
                &t
            ),
            primary_button(
                row![
                    icon(Icon::FolderOpen, 13.0, t.accent_ink),
                    label("Scan a folder", 11.5, Weight::Medium, t.accent_ink)
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                Message::PickFolder,
                &t
            ),
        ]
        .spacing(8),
    ]
    .spacing(12);

    column![
        shell::app_bar(app, &t),
        container(
            container(body)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
        )
        .width(Length::Fill)
        .height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn error_screen<'a>(app: &'a App, t: &Theme, message: &str) -> Element<'a, Message> {
    let t = *t;
    let panel = container(
        column![
            row![
                icon(Icon::Alert, 18.0, t.danger),
                label("Scan could not start", 18.0, Weight::Semibold, t.ink),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            label(message.to_string(), 12.0, Weight::Normal, t.ink_muted),
            row![
                outline_button("Back to volumes", Message::DismissScanError, &t),
                primary_button("Choose another folder", Message::PickFolder, &t),
            ]
            .spacing(8),
        ]
        .spacing(14),
    )
    .padding(24)
    .width(Length::Fixed(540.0))
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    column![
        shell::app_bar(app, &t),
        container(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
        )
        .width(Length::Fill)
        .height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Real sample bars: each bar is bytes added between two progress reports.
struct Trend {
    samples: Vec<u64>,
    line: Color,
    soft: Color,
    track: Color,
    hairline: Color,
}

impl canvas::Program<Message> for Trend {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &IcedTheme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.track);

        let height = bounds.height - 12.0;
        let baseline = bounds.height - 6.0;
        // A quiet baseline scale; no labels compete with the data.
        for i in 1..4 {
            let y = baseline - height * i as f32 / 4.0;
            frame.fill_rectangle(
                Point::new(6.0, y),
                Size::new((bounds.width - 12.0).max(0.0), 1.0),
                Color {
                    a: 0.5,
                    ..self.hairline
                },
            );
        }

        if self.samples.is_empty() {
            frame.fill_text(canvas::Text {
                content: "Waiting for the first progress report…".into(),
                position: Point::new(bounds.width / 2.0, bounds.height / 2.0),
                color: Color {
                    a: 0.9,
                    ..self.line
                },
                size: 11.5.into(),
                font: theme::ui_font(Weight::Normal),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
            return vec![frame.into_geometry()];
        }

        let max = self.samples.iter().copied().max().unwrap_or(1).max(1) as f32;
        let n = self.samples.len();
        let gap = 2.0;
        let usable = (bounds.width - 12.0).max(1.0);
        let bar_w = ((usable - gap * (n.saturating_sub(1)) as f32) / n as f32).max(1.0);
        let mut x = 6.0;
        for sample in &self.samples {
            let h = ((*sample as f32 / max) * height).max(1.5);
            let rect = Path::rounded_rectangle(
                Point::new(x, baseline - h),
                Size::new(bar_w, h),
                1.5.into(),
            );
            frame.fill(&rect, self.line);
            x += bar_w + gap;
        }

        // The newest sample is emphasised; the rest reads as history.
        if let Some(last) = self.samples.last() {
            let h = ((*last as f32 / max) * height).max(1.5);
            let x = (bounds.width - 6.0 - bar_w).max(6.0);
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(x, baseline - h),
                    Size::new(bar_w, h),
                    1.5.into(),
                ),
                self.soft,
            );
        }

        vec![frame.into_geometry()]
    }
}
