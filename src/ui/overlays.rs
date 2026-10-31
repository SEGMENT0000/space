//! Floating layers: context menu, drop target and transient toasts.

use iced::font::Weight;
use iced::widget::{MouseArea, Stack, button, column, container, row};
use iced::{Alignment, Background, Border, Element, Length, Padding, Point, Theme as IcedTheme};

use crate::model::NodeId;
use crate::ui::app::{
    APP_BAR_HEIGHT, App, Command, ContextMenu, Message, SUMMARY_HEIGHT, TOOLBAR_HEIGHT, Toast,
    ToastKind,
};
use crate::ui::components::*;
use crate::ui::icon::{Icon, file_icon, icon};
use crate::ui::theme::{self, Theme};
use crate::ui::{fmt, view};

const MENU_WIDTH: f32 = 282.0;
const MENU_HEIGHT: f32 = 292.0;

pub fn context_menu<'a>(app: &'a App, t: &Theme, context: &ContextMenu) -> Element<'a, Message> {
    let t = *t;
    let Some(tree) = app.tree() else {
        return iced::widget::Space::new().into();
    };
    if context.node as usize >= tree.len() {
        return iced::widget::Space::new().into();
    }
    let node = tree.node(context.node);
    let name = tree.name(context.node);
    let path = tree.path_string(context.node);

    let max_x = (app.window_size.width - MENU_WIDTH - 10.0).max(10.0);
    let max_y = (app.window_size.height - MENU_HEIGHT - 10.0).max(10.0);
    let x = context.at.x.clamp(10.0, max_x);
    let y = context.at.y.clamp(10.0, max_y);

    let header = container(
        column![
            row![
                icon(
                    if node.is_dir {
                        Icon::Folder
                    } else {
                        file_icon(name)
                    },
                    14.0,
                    t.category_ink(node.cat)
                ),
                label(theme::truncated(name, 26), 12.0, Weight::Semibold, t.ink),
                iced::widget::Space::new().width(Length::Fill),
                category_swatch(node.cat, &t, 7.0),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            label(fmt::short_path(&path, 38), 9.6, Weight::Normal, t.ink_faint)
                .font(theme::mono_font(Weight::Normal)),
        ]
        .spacing(3),
    )
    .padding(Padding::from([9.0, 10.0]));

    let mut items = iced::widget::Column::new().spacing(0);
    items = items.push(menu_item(
        if node.is_dir {
            Icon::FolderOpen
        } else {
            Icon::External
        },
        if node.is_dir {
            "Open folder"
        } else {
            "Open file"
        },
        "Enter",
        Message::OpenSelected,
        &t,
    ));
    items = items.push(menu_item(
        Icon::Search,
        "Reveal in Explorer",
        "",
        Message::RevealSelected,
        &t,
    ));
    items = items.push(menu_item(
        Icon::Copy,
        "Copy path",
        "Ctrl C",
        Message::CopyPath,
        &t,
    ));
    items = items.push(hline(t.hairline));
    if node.is_dir && context.node != app.current {
        items = items.push(menu_item(
            Icon::ArrowLeft,
            "Show in parent folder",
            "",
            Message::RevealHit(context.node),
            &t,
        ));
    }
    items = items.push(menu_item(
        Icon::UpLeft,
        "Go up one level",
        "Backspace",
        Message::GoUp,
        &t,
    ));
    items = items.push(menu_item(
        Icon::Refresh,
        "Rescan this volume",
        "",
        Message::Command(Command::Rescan),
        &t,
    ));

    let menu = container(column![header, hline(t.hairline), items])
        .width(Length::Fixed(MENU_WIDTH))
        .style(move |_| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border {
                color: t.hairline_strong,
                width: 1.0,
                radius: 7.0.into(),
            },
            ..Default::default()
        })
        .clip(true);

    let backdrop = MouseArea::new(
        container(
            iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .on_press(Message::CloseContext);

    let positioned = container(
        container(menu)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Start)
            .align_y(Alignment::Start)
            .padding(Padding {
                top: y,
                right: 0.0,
                bottom: 0.0,
                left: x,
            }),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    Stack::new()
        .width(Length::Fill)
        .height(Length::Fill)
        .push(Element::from(backdrop))
        .push(Element::from(opaque(positioned)))
        .into()
}

fn menu_item<'a>(
    glyph: Icon,
    title: &'a str,
    hint: &'a str,
    message: Message,
    t: &Theme,
) -> Element<'a, Message> {
    let t = *t;
    let hover = t.surface_sunken;
    iced::widget::button(
        row![
            icon(glyph, 13.0, t.ink_faint),
            label(title, 11.5, Weight::Normal, t.ink),
            iced::widget::Space::new().width(Length::Fill),
            label(hint, 9.8, Weight::Normal, t.ink_faint),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding(Padding::from([0.0, 4.0])),
    )
    .width(Length::Fill)
    .padding(Padding::from([7.0, 8.0]))
    .on_press(message)
    .style(
        move |_iced: &IcedTheme, status| iced::widget::button::Style {
            background: match status {
                iced::widget::button::Status::Hovered => Some(Background::Color(hover)),
                _ => None,
            },
            text_color: t.ink,
            border: Border::default(),
            shadow: iced::Shadow::default(),
            snap: true,
        },
    )
    .into()
}

pub fn breadcrumb_menu<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let Some(tree) = app.tree() else {
        return iced::widget::Space::new().into();
    };
    let lineage = tree.lineage(app.current);
    if lineage.len() <= 4 {
        return iced::widget::Space::new().into();
    }
    // The visible breadcrumb keeps the root, an ellipsis, the last two
    // ancestors and the current folder. The menu lists everything the
    // ellipsis hides, root-first.
    let hidden = view::hidden_ancestors(&lineage);
    if hidden.is_empty() {
        return iced::widget::Space::new().into();
    }

    let width = 308.0f32;
    let x = 220.0f32
        .min((app.window_size.width - width - 10.0).max(10.0))
        .max(10.0);
    let y = (APP_BAR_HEIGHT + SUMMARY_HEIGHT + TOOLBAR_HEIGHT + 30.0)
        .min((app.window_size.height - 80.0).max(10.0));

    let mut items = column![].spacing(0);
    items = items.push(
        container(label(
            "Ancestor folders",
            9.8,
            Weight::Semibold,
            t.ink_faint,
        ))
        .padding(Padding::from([8.0, 10.0])),
    );
    for id in hidden {
        let name = tree.name(*id);
        items = items.push(
            button(
                row![
                    icon(Icon::Folder, 13.0, t.ink_muted),
                    label(theme::truncated(name, 40), 11.5, Weight::Normal, t.ink),
                    iced::widget::Space::new().width(Length::Fill),
                    icon(Icon::ChevronRight, 11.0, t.ink_faint),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .padding(Padding::from([0.0, 3.0])),
            )
            .width(Length::Fill)
            .padding(Padding::from([7.0, 9.0]))
            .on_press(Message::Drill(*id))
            .style({
                let hover = t.surface_sunken;
                move |_iced: &IcedTheme, status| button::Style {
                    background: match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(Background::Color(hover))
                        }
                        _ => None,
                    },
                    text_color: t.ink,
                    border: Border::default(),
                    shadow: iced::Shadow::default(),
                    snap: true,
                }
            }),
        );
        items = items.push(hline(t.hairline));
    }

    let menu = container(items)
        .width(Length::Fixed(width))
        .style(move |_| container::Style {
            background: Some(Background::Color(t.surface_raised)),
            border: Border {
                color: t.hairline_strong,
                width: 1.0,
                radius: 7.0.into(),
            },
            ..Default::default()
        })
        .clip(true);

    let backdrop = MouseArea::new(
        container(
            iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .on_press(Message::CloseCrumbMenu);

    let positioned = container(
        container(menu)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Start)
            .align_y(Alignment::Start)
            .padding(Padding {
                top: y,
                right: 0.0,
                bottom: 0.0,
                left: x,
            }),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    Stack::new()
        .width(Length::Fill)
        .height(Length::Fill)
        .push(Element::from(backdrop))
        .push(Element::from(opaque(positioned)))
        .into()
}

pub fn settings_panel<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let width = 392.0f32;
    let x = (app.window_size.width - width - 14.0).max(10.0);
    let y = (APP_BAR_HEIGHT + 8.0).min((app.window_size.height - 180.0).max(10.0));

    let header = row![
        icon(Icon::Settings, 14.0, t.ink_muted),
        label("Settings", 13.0, Weight::Semibold, t.ink),
        iced::widget::Space::new().width(Length::Fill),
        button(icon(Icon::Close, 12.0, t.ink_faint))
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(24.0))
            .padding(0)
            .on_press(Message::CloseSettings)
            .style({
                let hover = t.surface_sunken;
                move |_iced: &IcedTheme, status| button::Style {
                    background: match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(Background::Color(hover))
                        }
                        _ => None,
                    },
                    text_color: t.ink_faint,
                    border: Border::default(),
                    shadow: iced::Shadow::default(),
                    snap: true,
                }
            }),
    ]
    .spacing(7)
    .align_y(Alignment::Center);

    let toggle = if app.rich_previews {
        row![
            iced::widget::Space::new().width(Length::Fill),
            container(iced::widget::Space::new())
                .width(Length::Fixed(8.0))
                .height(Length::Fixed(8.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(t.accent_ink)),
                    border: Border {
                        color: t.accent_ink,
                        width: 0.0,
                        radius: 999.0.into(),
                    },
                    ..Default::default()
                }),
        ]
    } else {
        row![
            container(iced::widget::Space::new())
                .width(Length::Fixed(8.0))
                .height(Length::Fixed(8.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(t.ink_faint)),
                    border: Border {
                        color: t.ink_faint,
                        width: 0.0,
                        radius: 999.0.into(),
                    },
                    ..Default::default()
                }),
            iced::widget::Space::new().width(Length::Fill),
        ]
    };
    let switch = container(toggle)
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(18.0))
        .padding(Padding::from([4.0, 5.0]))
        .style({
            let track = if app.rich_previews {
                t.accent
            } else {
                t.hairline_strong
            };
            move |_| container::Style {
                background: Some(Background::Color(track)),
                border: Border {
                    color: track,
                    width: 0.0,
                    radius: 999.0.into(),
                },
                ..Default::default()
            }
        });

    let row_style = {
        let hover = t.surface_sunken;
        move |_iced: &IcedTheme, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(Background::Color(hover)),
                _ => None,
            },
            text_color: t.ink,
            border: Border::default(),
            shadow: iced::Shadow::default(),
            snap: true,
        }
    };
    let rich_row = button(
        row![
            column![
                label("Rich previews (uses Windows)", 12.0, Weight::Medium, t.ink),
                label(
                    if app.rich_previews { "On" } else { "Off" },
                    10.0,
                    Weight::Normal,
                    if app.rich_previews {
                        t.accent
                    } else {
                        t.ink_faint
                    }
                ),
            ]
            .spacing(2)
            .width(Length::Fill),
            switch,
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding(Padding::from([0.0, 2.0])),
    )
    .width(Length::Fill)
    .padding(Padding::from([8.0, 10.0]))
    .on_press(Message::ToggleRichPreviews)
    .style(row_style);

    let description = container(
        column![
            label(
                "Off by default. When on, space asks Windows for thumbnails of PDFs, videos, Office documents, fonts and other formats without an in-process decoder. The request runs on a worker thread with a 2 GiB size gate and a 1.8 s timeout. space never executes the file, launches an application for it, or writes to it; a failed request falls back to in-process previews.",
                10.5,
                Weight::Normal,
                t.ink_muted
            ),
            label(
                "When off, every preview is decoded in-process: image thumbnails, text snippets, archive listings, SQLite tables, audio tags, executable version resources, metadata and hex.",
                10.5,
                Weight::Normal,
                t.ink_faint
            ),
        ]
        .spacing(6),
    )
    .padding(Padding::from([8.0, 10.0]));

    let panel = container(column![
        container(header).padding(Padding::from([10.0, 12.0])),
        hline(t.hairline),
        rich_row,
        description,
        hline(t.hairline),
        container(label(
            "Read-only by design. Previews are bounded and cached.",
            9.8,
            Weight::Normal,
            t.ink_faint
        ))
        .padding(Padding::from([8.0, 10.0])),
    ])
    .width(Length::Fixed(width))
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface_raised)),
        border: Border {
            color: t.hairline_strong,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .clip(true);

    let backdrop = MouseArea::new(
        container(
            iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .on_press(Message::CloseSettings);

    let positioned = container(
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::Start)
            .padding(Padding {
                top: y,
                right: (app.window_size.width - x - width).max(0.0),
                bottom: 0.0,
                left: 0.0,
            }),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    Stack::new()
        .width(Length::Fill)
        .height(Length::Fill)
        .push(Element::from(backdrop))
        .push(Element::from(opaque(positioned)))
        .into()
}

pub fn drop_overlay<'a>(t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let box_panel = container(
        column![
            icon(Icon::FolderOpen, 30.0, t.accent),
            label("Drop to scan", 19.0, Weight::Semibold, t.ink),
            label(
                "space reads the folder recursively and never modifies it.",
                11.5,
                Weight::Normal,
                t.ink_muted
            ),
            row![
                keycap("Esc", &t),
                label("cancel", 10.0, Weight::Normal, t.ink_faint),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        ]
        .spacing(9)
        .align_x(Alignment::Center),
    )
    .padding(Padding::from([38.0, 54.0]))
    .max_width(440.0)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border {
            color: t.accent,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    });

    container(
        container(box_panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.scrim)),
        ..Default::default()
    })
    .into()
}

pub fn toast<'a>(app: &'a App, t: &Theme, toast: &'a Toast) -> Element<'a, Message> {
    let t = *t;
    let (dot, text_color) = match toast.kind {
        ToastKind::Info => (t.success, t.ink),
        ToastKind::Error => (t.danger, t.ink),
    };
    let pill = container(
        row![
            container(iced::widget::Space::new())
                .width(Length::Fixed(6.0))
                .height(Length::Fixed(6.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(dot)),
                    border: Border {
                        color: dot,
                        width: 0.0,
                        radius: 999.0.into(),
                    },
                    ..Default::default()
                }),
            label(
                theme::truncated(&toast.text, 94),
                11.5,
                Weight::Medium,
                text_color
            ),
            iced::widget::Space::new().width(Length::Fill),
            keycap("Esc", &t),
        ]
        .spacing(9)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([8.0, 12.0]))
    .max_width(560.0)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface_raised)),
        border: Border {
            color: t.hairline_strong,
            width: 1.0,
            radius: 999.0.into(),
        },
        ..Default::default()
    });

    let _ = app;
    let layer = container(
        container(MouseArea::new(pill).on_press(Message::DismissToast))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: 78.0,
                left: 0.0,
            })
            .align_x(Alignment::Center)
            .align_y(Alignment::End),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    opaque(layer)
}

fn opaque<'a, T>(element: T) -> Element<'a, Message>
where
    T: Into<Element<'a, Message>>,
{
    iced::widget::opaque(element)
}

#[allow(dead_code)]
fn _keep_view_import(_: &Point, _: NodeId) {
    let _ = view::app_bar;
}
