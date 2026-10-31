//! Command palette. Every row is a real action or a real node from the scan;
//! there are no placeholder commands.

use iced::font::Weight;
use iced::widget::{MouseArea, Stack, column, container, row, scrollable, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme as IcedTheme};

use crate::ui::app::{App, Command, Message};
use crate::ui::components::*;
use crate::ui::icon::{Icon, category_icon, file_icon, icon};
use crate::ui::theme::{self, Theme};
use crate::ui::{PALETTE_INPUT, fmt};

pub fn view<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let scrim = MouseArea::new(
        container(
            iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.scrim)),
            ..Default::default()
        }),
    )
    .on_press(Message::PaletteClose);

    let panel = container(palette_panel(app, &t))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: 86.0,
            right: 20.0,
            bottom: 20.0,
            left: 20.0,
        })
        .align_x(Alignment::Center)
        .align_y(Alignment::Start);

    Stack::new()
        .width(Length::Fill)
        .height(Length::Fill)
        .push(Element::from(scrim))
        .push(Element::from(panel))
        .into()
}

fn palette_panel<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let input = text_input("Search commands, files and folders…", &app.palette.query)
        .id(PALETTE_INPUT)
        .on_input(Message::PaletteQuery)
        .on_submit(Message::PaletteAccept)
        .padding(Padding::from([9.0, 10.0]))
        .size(14.0)
        .font(theme::ui_font(Weight::Normal))
        .style(move |_iced: &IcedTheme, _status| text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            icon: t.ink_faint,
            placeholder: t.ink_faint,
            value: t.ink,
            selection: t.selection,
        })
        .width(Length::Fill);

    let search = container(
        row![
            icon(Icon::Search, 15.0, t.ink_faint),
            input,
            keycap("Esc", &t),
        ]
        .spacing(9)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([6.0, 12.0]))
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface_sunken)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    });

    let mut results = iced::widget::Column::new().spacing(0).width(Length::Fill);
    let mut index = 0usize;

    if !app.palette.commands.is_empty() {
        results = results.push(section_row("Commands", &t));
        for command in &app.palette.commands {
            let selected = app.palette.index == index;
            results = results.push(command_row(*command, selected, index, &t));
            index += 1;
        }
    }

    if !app.palette.hits.is_empty() {
        results = results.push(section_row("In this scan", &t));
        for hit in &app.palette.hits {
            let selected = app.palette.index == index;
            results = results.push(hit_row(hit, selected, index, &t));
            index += 1;
        }
    }

    if app.palette.commands.is_empty() && app.palette.hits.is_empty() {
        results = results.push(
            container(
                column![
                    label(
                        if app.palette.query.trim().is_empty() {
                            "Type to find a command or a file."
                        } else {
                            "Nothing matches that search."
                        },
                        12.5,
                        Weight::Normal,
                        t.ink_muted
                    ),
                    label(
                        "Search covers the scanned tree; it never leaves this volume.",
                        10.5,
                        Weight::Normal,
                        t.ink_faint
                    ),
                ]
                .spacing(4),
            )
            .padding(Padding::from([22.0, 16.0])),
        );
    }

    // The panel grows with real content but never past the viewport budget.
    // A bare search stays compact instead of leaving a large empty slab.
    let mut body_height = 16.0;
    if !app.palette.commands.is_empty() {
        body_height += 24.0 + 36.0 * app.palette.commands.len() as f32;
    }
    if !app.palette.hits.is_empty() {
        body_height += 24.0 + 52.0 * app.palette.hits.len() as f32;
    }
    let body_height = body_height.clamp(150.0, 420.0);

    let body = container(
        scrollable(results)
            .height(Length::Fixed(body_height))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new()
                    .width(8.0)
                    .scroller_width(4.0)
                    .margin(2.0)
                    .spacing(4.0),
            ))
            .style(move |_, _| scrollable::Style {
                container: container::Style::default(),
                vertical_rail: scrollable::Rail {
                    background: None,
                    border: Border::default(),
                    scroller: scrollable::Scroller {
                        background: Background::Color(t.hairline_strong),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 3.0.into(),
                        },
                    },
                },
                horizontal_rail: scrollable::Rail {
                    background: None,
                    border: Border::default(),
                    scroller: scrollable::Scroller {
                        background: Background::Color(Color::TRANSPARENT),
                        border: Border::default(),
                    },
                },
                gap: None,
                auto_scroll: scrollable::AutoScroll {
                    background: Background::Color(t.surface_raised),
                    border: Border {
                        color: t.hairline,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    shadow: iced::Shadow::default(),
                    icon: t.ink_muted,
                },
            }),
    )
    .width(Length::Fill)
    .padding(Padding::from([6.0, 6.0]));

    let footer = container(
        row![
            keycap("↑", &t),
            keycap("↓", &t),
            label("navigate", 10.0, Weight::Normal, t.ink_faint),
            keycap("Enter", &t),
            label("run", 10.0, Weight::Normal, t.ink_faint),
            iced::widget::Space::new().width(Length::Fill),
            label(
                "Actions operate on the current selection or volume.",
                10.0,
                Weight::Normal,
                t.ink_faint
            ),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([8.0, 12.0]))
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface_raised)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    });

    container(column![search, body, hline(t.hairline), footer])
        .width(Length::Fixed(620.0))
        .max_height(560.0)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border {
                color: t.hairline_strong,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .clip(true)
        .into()
}

fn section_row<'a>(title: &'a str, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    container(label(
        title.to_ascii_uppercase(),
        9.5,
        Weight::Semibold,
        t.ink_faint,
    ))
    .width(Length::Fill)
    .padding(Padding {
        top: 10.0,
        right: 10.0,
        bottom: 5.0,
        left: 10.0,
    })
    .into()
}

fn row_style(
    selected: bool,
    t: Theme,
) -> impl Fn(&IcedTheme, iced::widget::button::Status) -> iced::widget::button::Style + 'static {
    let hover = t.surface_sunken;
    let selection = t.accent_soft;
    let ink = t.ink;
    move |_iced: &IcedTheme, status| iced::widget::button::Style {
        background: Some(Background::Color(if selected {
            selection
        } else if matches!(status, iced::widget::button::Status::Hovered) {
            hover
        } else {
            Color::TRANSPARENT
        })),
        text_color: ink,
        border: Border {
            color: if selected {
                t.hairline
            } else {
                Color::TRANSPARENT
            },
            width: if selected { 1.0 } else { 0.0 },
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
        snap: true,
    }
}

fn command_row<'a>(
    command: Command,
    selected: bool,
    index: usize,
    t: &Theme,
) -> Element<'a, Message> {
    let t = *t;
    let glyph = command_icon(command);
    iced::widget::button(
        row![
            icon(glyph, 14.0, if selected { t.accent } else { t.ink_faint }),
            label(command.label(), 12.5, Weight::Medium, t.ink),
            iced::widget::Space::new().width(Length::Fill),
            if command.hint().is_empty() {
                label("", 10.0, Weight::Normal, t.ink_faint)
            } else {
                label(command.hint(), 10.0, Weight::Normal, t.ink_faint)
            },
        ]
        .spacing(9)
        .align_y(Alignment::Center)
        .padding(Padding::from([0.0, 6.0])),
    )
    .width(Length::Fill)
    .padding(Padding::from([7.0, 8.0]))
    .on_press(Message::PaletteChoose(index))
    .style(row_style(selected, t))
    .into()
}

fn hit_row<'a>(
    hit: &'a crate::ui::app::PaletteHit,
    selected: bool,
    index: usize,
    t: &Theme,
) -> Element<'a, Message> {
    let t = *t;
    let glyph = if hit.is_dir {
        Icon::Folder
    } else {
        file_icon(&hit.path)
    };
    iced::widget::button(
        row![
            icon(
                if hit.is_dir {
                    glyph
                } else {
                    category_icon(hit.cat)
                },
                14.0,
                if selected {
                    t.category_ink(hit.cat)
                } else {
                    t.ink_faint
                }
            ),
            column![
                label(
                    crate::ui::theme::truncated(file_name(&hit.path), 52),
                    11.5,
                    Weight::Medium,
                    t.ink
                ),
                label(path_tail(&hit.path), 9.5, Weight::Normal, t.ink_faint)
                    .font(theme::mono_font(Weight::Normal)),
            ]
            .spacing(2)
            .width(Length::Fill),
            mono(
                fmt::bytes_short(hit.size),
                10.5,
                Weight::Normal,
                t.ink_muted
            ),
            keycap("↵", &t),
        ]
        .spacing(9)
        .align_y(Alignment::Center)
        .padding(Padding::from([0.0, 6.0])),
    )
    .width(Length::Fill)
    .padding(Padding::from([6.0, 8.0]))
    .on_press(Message::PaletteChoose(index))
    .style(row_style(selected, t))
    .into()
}

fn file_name(path: &str) -> &str {
    path.rsplit_once(['\\', '/'])
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
}

fn path_tail(path: &str) -> String {
    let parent = path
        .rsplit_once(['\\', '/'])
        .map(|(parent, _)| parent)
        .unwrap_or(path);
    format!("in {}", fmt::short_path(parent, 56))
}

fn command_icon(command: Command) -> Icon {
    match command {
        Command::Drives => Icon::Drive,
        Command::Rescan => Icon::Refresh,
        Command::ToggleTheme => Icon::Sun,
        Command::Refit => Icon::Maximize,
        Command::CopyPath => Icon::Copy,
        Command::Open => Icon::External,
        Command::Reveal => Icon::FolderOpen,
        Command::Up => Icon::UpLeft,
        Command::Back => Icon::ArrowLeft,
        Command::Forward => Icon::ArrowRight,
        Command::PickFolder => Icon::FolderOpen,
        Command::RichPreviews => Icon::Settings,
    }
}
