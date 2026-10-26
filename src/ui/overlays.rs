//! Floating layers: context menu, drop target and transient toasts.

use iced::font::Weight;
use iced::widget::{MouseArea, Stack, column, container, row};
use iced::{Alignment, Background, Border, Element, Length, Padding, Point, Theme as IcedTheme};

use crate::model::NodeId;
use crate::ui::app::{App, Command, ContextMenu, Message, Toast, ToastKind};
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
