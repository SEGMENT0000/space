//! Right-hand inspector: identity, actions, safe preview, composition and the
//! largest files in the scan.

use iced::font::Weight;
use iced::widget::{button, column, container, image, row, scrollable, text};
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Padding, Theme as IcedTheme,
};

use crate::model::{Category, CategoryTotals, NodeId};
use crate::preview::{
    DbPreview, HexPreview, ImagePreview, ListPreview, MediaPreview, MetaPreview, Preview,
    TextPreview,
};
use crate::ui::app::{App, Message, RAIL_WIDTH, SortMode};
use crate::ui::components::*;
use crate::ui::fmt;
use crate::ui::icon::{Icon, file_icon, icon};
use crate::ui::theme::{self, Theme};

pub fn view<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let header = container(
        row![
            column![
                label("Inspector", theme::TEXT_MD, Weight::Semibold, t.ink),
                label(
                    "Read-only details",
                    theme::TEXT_SM,
                    Weight::Normal,
                    t.ink_muted
                ),
            ]
            .spacing(1),
            iced::widget::Space::new().width(Length::Fill),
            icon_button(Icon::Close, 14.0, Message::ToggleRail, t.ink_faint, &t),
        ]
        .align_y(Alignment::Center),
    )
    .height(Length::Fixed(60.0))
    .padding(Padding::from([0.0, 14.0]));

    let mut content = iced::widget::Column::new().spacing(16).width(Length::Fill);
    if let Some(tree) = app.tree() {
        let id = app.selection.unwrap_or(app.current);
        if id as usize >= tree.len() {
            return empty_rail(&t, header.into());
        }
        let node = tree.node(id);
        content = content.push(identity(app, &t, id));
        content = content.push(metrics(app, &t, id));
        content = content.push(actions(app, &t, id));
        if let Some(preview) = &app.preview {
            if !node.is_dir && app.preview_for == Some(id) {
                content = content.push(preview_section(app, &t, preview));
            }
        } else if app.preview_loading && !node.is_dir && app.preview_for == Some(id) {
            content = content.push(
                column![
                    section_head("Preview", &t, None),
                    container(label(
                        "Reading a bounded sample…",
                        11.0,
                        Weight::Normal,
                        t.ink_faint
                    ))
                    .padding(12),
                ]
                .spacing(8),
            );
        }
        content = content.push(hline(t.hairline));
        content = content.push(composition_section(app, &t, &app.composition));
        content = content.push(hline(t.hairline));
        content = content.push(largest_section(app, &t));
    } else {
        content = content.push(label(
            "No scan is loaded.",
            12.0,
            Weight::Normal,
            t.ink_muted,
        ));
    }

    container(column![
        header,
        hline(t.hairline),
        scrollable(container(content).padding(Padding {
            top: 14.0,
            right: 10.0,
            bottom: 18.0,
            left: 14.0
        }))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(8.0)
                .scroller_width(4.0)
                .margin(2.0)
                .spacing(4.0)
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
                        radius: 3.0.into()
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
                    radius: 4.0.into()
                },
                shadow: iced::Shadow::default(),
                icon: t.ink_muted,
            },
        }),
    ])
    .width(Length::Fixed(RAIL_WIDTH))
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 7.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn empty_rail<'a>(t: &Theme, header: Element<'a, Message>) -> Element<'a, Message> {
    let t = *t;
    container(column![
        header,
        hline(t.hairline),
        container(label(
            "Nothing selected.",
            12.0,
            Weight::Normal,
            t.ink_muted
        ))
        .padding(16),
    ])
    .width(Length::Fixed(RAIL_WIDTH))
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 7.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn identity<'a>(app: &'a App, t: &Theme, id: NodeId) -> Element<'a, Message> {
    let t = *t;
    let tree = app.tree().unwrap();
    let node = tree.node(id);
    let name = tree.name(id);
    let path = tree.path_string(id);
    let cat = node.cat;
    let glyph = if node.is_dir {
        Icon::Folder
    } else {
        file_icon(name)
    };
    let icon_box = container(icon(
        glyph,
        if node.is_dir { 18.0 } else { 16.0 },
        t.category_ink(cat),
    ))
    .width(Length::Fixed(36.0))
    .height(Length::Fixed(36.0))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.category(cat))),
        border: Border {
            color: t.hairline,
            width: 0.5,
            radius: 5.0.into(),
        },
        ..Default::default()
    });

    let chip = container(
        row![
            category_swatch(cat, &t, 7.0),
            label(cat.label(), 10.5, Weight::Medium, t.ink_muted),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([2.0, 6.0]))
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface_sunken)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 999.0.into(),
        },
        ..Default::default()
    });

    column![
        row![
            icon_box,
            column![
                label(theme::truncated(name, 26), 14.5, Weight::Semibold, t.ink),
                chip,
            ]
            .spacing(5)
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        label(fmt::short_path(&path, 44), 9.4, Weight::Normal, t.ink_faint,)
            .font(theme::mono_font(Weight::Normal)),
    ]
    .spacing(9)
    .into()
}

fn metrics<'a>(app: &'a App, t: &Theme, id: NodeId) -> Element<'a, Message> {
    let t = *t;
    let tree = app.tree().unwrap();
    let node = tree.node(id);
    let direct = tree.child_count(id);
    let mut cells: Vec<(&'static str, String)> = Vec::new();
    cells.push(("Size", fmt::bytes(node.size)));
    if node.is_dir {
        cells.push(("Files", fmt::count(node.file_count)));
        cells.push(("Items", fmt::count(direct as u64)));
        cells.push((
            "Share",
            fmt::percent(node.size, tree.node(app.current).size.max(1)),
        ));
    } else {
        cells.push(("Type", file_type(tree.name(id))));
        cells.push(("Items", "—".into()));
        cells.push((
            "Share",
            fmt::percent(node.size, tree.node(app.current).size.max(1)),
        ));
    }
    let mut grid = iced::widget::Column::new().spacing(0);
    for pair in cells.chunks(2) {
        let mut r = iced::widget::Row::new().spacing(0);
        for (i, (k, v)) in pair.iter().enumerate() {
            if i > 0 {
                r = r.push(vline(t.hairline));
            }
            r = r.push(
                container(
                    column![
                        label(*k, 10.0, Weight::Normal, t.ink_faint),
                        mono(v.clone(), 12.0, Weight::Medium, t.ink),
                    ]
                    .spacing(3),
                )
                .width(Length::FillPortion(1))
                .padding(Padding::from([8.0, 9.0])),
            );
        }
        grid = grid.push(r);
        grid = grid.push(hline(t.hairline));
    }
    container(grid)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.surface_raised)),
            border: Border {
                color: t.hairline,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn actions<'a>(app: &'a App, t: &Theme, id: NodeId) -> Element<'a, Message> {
    let t = *t;
    let is_file = app
        .tree()
        .map(|tree| !tree.node(id).is_dir)
        .unwrap_or(false);
    let open_label = if is_file { "Open" } else { "Open folder" };
    let action =
        |glyph: Icon, label_text: &'static str, message: Message| -> Element<'a, Message> {
            button(
                row![
                    icon(glyph, 12.5, t.ink_muted),
                    label(label_text, 10.5, Weight::Medium, t.ink_muted),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([5.0, 6.0]))
            .on_press(message)
            .style({
                let hover = t.surface_sunken;
                move |_iced: &IcedTheme, status| button::Style {
                    background: match status {
                        button::Status::Hovered => Some(Background::Color(hover)),
                        _ => None,
                    },
                    text_color: t.ink_muted,
                    border: Border::default(),
                    shadow: iced::Shadow::default(),
                    snap: true,
                }
            })
            .width(Length::FillPortion(1))
            .into()
        };
    row![
        action(
            if is_file {
                Icon::External
            } else {
                Icon::FolderOpen
            },
            open_label,
            Message::OpenSelected
        ),
        vline(t.hairline),
        action(Icon::Search, "Reveal", Message::RevealSelected),
        vline(t.hairline),
        action(Icon::Copy, "Copy", Message::CopyPath),
    ]
    .spacing(1)
    .into()
}

fn preview_section<'a>(_app: &'a App, t: &Theme, preview: &'a Preview) -> Element<'a, Message> {
    let t = *t;
    let mut body = iced::widget::Column::new().spacing(8).width(Length::Fill);
    match preview {
        Preview::Text(p) => body = body.push(text_preview(p, &t)),
        Preview::Hex(p) => body = body.push(hex_preview(p, &t)),
        Preview::Image(p) => body = body.push(image_preview(p, &t)),
        Preview::List(p) => body = body.push(list_preview(p, &t)),
        Preview::Meta(p) => body = body.push(meta_preview(p, &t)),
        Preview::Db(p) => body = body.push(db_preview(p, &t)),
        Preview::Media(p) => body = body.push(media_preview(p, &t)),
        Preview::Unavailable { reason } => {
            body = body.push(label(reason.clone(), 11.0, Weight::Normal, t.ink_muted))
        }
    }
    let kind = format!("{} preview", preview.kind());
    column![section_head("Preview", &t, Some(kind)), body]
        .spacing(8)
        .into()
}

fn section_head(
    title: impl Into<String>,
    t: &Theme,
    badge: Option<String>,
) -> Element<'static, Message> {
    let t = *t;
    let mut r = row![section_heading(title.into(), &t)].align_y(Alignment::Center);
    if let Some(badge) = badge {
        r = r.push(iced::widget::Space::new().width(Length::Fill)).push(
            container(label(badge, 9.5, Weight::Medium, t.ink_faint))
                .padding(Padding::from([2.0, 6.0]))
                .style(move |_| container::Style {
                    background: Some(Background::Color(t.surface_sunken)),
                    border: Border {
                        color: t.hairline,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }),
        );
    }
    r.into()
}

fn preview_surface<'a>(
    content: impl Into<Element<'a, Message>>,
    t: &Theme,
) -> Element<'a, Message> {
    let t = *t;
    container(content)
        .width(Length::Fill)
        .padding(10)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.surface_sunken)),
            border: Border {
                color: t.hairline,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn text_preview<'a>(p: &TextPreview, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let lines = p.lines.iter().take(14);
    let mut col = iced::widget::Column::new().spacing(1);
    for line in lines {
        let shown = if line.chars().count() > 58 {
            format!("{}…", line.chars().take(57).collect::<String>())
        } else {
            line.clone()
        };
        col = col.push(
            text(shown)
                .size(9.8)
                .font(theme::mono_font(Weight::Normal))
                .color(t.ink),
        );
    }
    let foot = match (p.truncated, p.total_lines) {
        (true, Some(total)) => format!(
            "{} lines total · showing a bounded sample",
            fmt::count(total)
        ),
        (true, None) => "Bounded sample · file continues".into(),
        _ => p
            .encoding
            .clone()
            .map(|e| format!("{e} · {} lines", p.lines.len()))
            .unwrap_or_else(|| format!("{} lines", p.lines.len())),
    };
    column![
        preview_surface(col, &t),
        label(foot, 9.8, Weight::Normal, t.ink_faint),
    ]
    .spacing(5)
    .into()
}

fn hex_preview<'a>(p: &HexPreview, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let mut col = iced::widget::Column::new().spacing(1);
    const BYTES_PER_LINE: usize = 8;
    const MAX_ROWS: usize = 10;
    let mut shown_bytes = 0usize;
    for row in p.rows.iter().take(MAX_ROWS) {
        let bytes: Vec<&str> = row.hex.split_whitespace().collect();
        let ascii: Vec<char> = row.ascii.chars().collect();
        shown_bytes += bytes.len();
        for (chunk, start) in bytes
            .chunks(BYTES_PER_LINE)
            .zip((0..bytes.len()).step_by(BYTES_PER_LINE))
        {
            let hex = chunk.join(" ");
            let ascii_text: String = ascii.iter().skip(start).take(chunk.len()).collect();
            let line = format!(
                "{:08x}  {:<23}  {}",
                row.offset + start as u64,
                hex,
                ascii_text
            );
            col = col.push(
                text(line)
                    .size(8.6)
                    .font(theme::mono_font(Weight::Normal))
                    .color(t.ink),
            );
        }
    }
    column![
        preview_surface(col, &t),
        label(
            format!(
                "{} · first {} bytes",
                fmt::bytes(p.total),
                fmt::bytes(shown_bytes as u64)
            ),
            9.8,
            Weight::Normal,
            t.ink_faint
        ),
    ]
    .spacing(5)
    .into()
}

fn image_preview<'a>(p: &ImagePreview, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let handle = image::Handle::from_rgba(p.width, p.height, p.rgba.to_vec());
    let img = image(handle)
        .content_fit(ContentFit::Contain)
        .width(Length::Fill);
    container(img)
        .width(Length::Fill)
        .height(Length::Fixed(168.0))
        .padding(4)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.surface_sunken)),
            border: Border {
                color: t.hairline,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn list_preview<'a>(p: &ListPreview, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let mut col = iced::widget::Column::new().spacing(0);
    for entry in p.entries.iter().take(9) {
        let glyph = if entry.kind == crate::preview::ListKind::Dir {
            Icon::Folder
        } else {
            file_icon(&entry.name)
        };
        let size = entry
            .size
            .map(fmt::bytes_short)
            .unwrap_or_else(|| "—".into());
        col = col.push(
            row![
                icon(glyph, 11.0, t.ink_faint),
                label(
                    theme::truncated(&entry.name, 32),
                    10.2,
                    Weight::Normal,
                    t.ink
                ),
                iced::widget::Space::new().width(Length::Fill),
                mono(size, 9.8, Weight::Normal, t.ink_faint),
            ]
            .spacing(5)
            .align_y(Alignment::Center)
            .padding(Padding::from([2.0, 0.0])),
        );
    }
    let foot = format!(
        "{} · {}{}",
        p.kind,
        fmt::count(p.entries.len() as u64),
        if p.truncated { " shown" } else { " entries" }
    );
    column![
        preview_surface(col, &t),
        label(foot, 9.8, Weight::Normal, t.ink_faint),
    ]
    .spacing(5)
    .into()
}

fn meta_preview<'a>(p: &MetaPreview, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let mut col = iced::widget::Column::new().spacing(2);
    for pair in p.pairs.iter().take(12) {
        col = col.push(
            row![
                label(pair.key.clone(), 9.8, Weight::Normal, t.ink_faint),
                iced::widget::Space::new().width(Length::Fill),
                mono(
                    theme::truncated(&pair.value, 32),
                    9.8,
                    Weight::Medium,
                    t.ink
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    preview_surface(col, &t)
}

fn db_preview<'a>(p: &DbPreview, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let mut col = iced::widget::Column::new().spacing(7);
    for table in p.tables.iter().take(4) {
        let mut r = row![
            icon(Icon::Database, 11.0, t.ink_faint),
            label(table.name.clone(), 10.4, Weight::Medium, t.ink),
            iced::widget::Space::new().width(Length::Fill),
            mono(
                format!("{} cols", table.cols),
                9.5,
                Weight::Normal,
                t.ink_faint
            ),
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        if let Some(rows) = &table.rows
            && let Some(first) = rows.first()
        {
            let preview: Vec<String> = first
                .iter()
                .take(3)
                .map(|c| theme::truncated(c, 10))
                .collect();
            r = r.push(mono(preview.join(" · "), 9.5, Weight::Normal, t.ink_faint));
        }
        col = col.push(r);
    }
    column![
        preview_surface(col, &t),
        label(
            if p.truncated {
                "Bounded schema sample"
            } else {
                "Schema and sample rows"
            },
            9.8,
            Weight::Normal,
            t.ink_faint
        ),
    ]
    .spacing(5)
    .into()
}

fn media_preview<'a>(p: &MediaPreview, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let mut col = iced::widget::Column::new().spacing(8);
    if let Some(thumb) = &p.thumbnail {
        let handle = image::Handle::from_rgba(thumb.width, thumb.height, thumb.rgba.to_vec());
        col = col.push(
            container(
                image(handle)
                    .content_fit(ContentFit::Contain)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fixed(150.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(t.surface_sunken)),
                border: Border {
                    color: t.hairline,
                    width: 1.0,
                    radius: 5.0.into(),
                },
                ..Default::default()
            }),
        );
    }
    let mut facts = iced::widget::Column::new().spacing(2);
    for pair in p.pairs.iter().take(8) {
        facts = facts.push(
            row![
                label(pair.key.clone(), 9.6, Weight::Normal, t.ink_faint),
                iced::widget::Space::new().width(Length::Fill),
                mono(
                    theme::truncated(&pair.value, 26),
                    9.6,
                    Weight::Medium,
                    t.ink
                ),
            ]
            .spacing(6),
        );
    }
    col = col.push(preview_surface(facts, &t));
    if let Some(note) = &p.note {
        col = col.push(label(note.clone(), 9.6, Weight::Normal, t.ink_faint));
    }
    col.into()
}

fn composition_section<'a>(
    app: &'a App,
    t: &Theme,
    totals: &CategoryTotals,
) -> Element<'a, Message> {
    let t = *t;
    let total = totals.total();
    let mut bar = iced::widget::Row::new().height(Length::Fixed(6.0));
    for cat in Category::ALL {
        let bytes = totals.get(cat);
        if bytes == 0 {
            continue;
        }
        let portion = ((bytes as f64 / total.max(1) as f64) * 1000.0)
            .round()
            .max(1.0) as u16;
        bar = bar.push(
            container(iced::widget::Space::new())
                .width(Length::FillPortion(portion))
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(t.category(cat))),
                    ..Default::default()
                }),
        );
    }
    let mut list = iced::widget::Column::new().spacing(3);
    for cat in Category::ALL {
        let bytes = totals.get(cat);
        if bytes == 0 {
            continue;
        }
        let pct = if total > 0 {
            bytes as f32 / total as f32 * 100.0
        } else {
            0.0
        };
        list = list.push(
            row![
                category_swatch(cat, &t, 7.0),
                label(cat.label(), 10.0, Weight::Normal, t.ink_muted),
                iced::widget::Space::new().width(Length::Fill),
                mono(
                    format!("{} · {pct:.1}%", fmt::bytes_short(bytes)),
                    9.6,
                    Weight::Normal,
                    t.ink_faint
                ),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        );
    }
    if app.composition_loading {
        list = list.push(label("Computing…", 10.0, Weight::Normal, t.ink_faint));
    }
    column![
        section_head(
            format!("Composition · {}", fmt::bytes_short(total)),
            &t,
            None
        ),
        container(bar).clip(true).style(move |_| container::Style {
            background: Some(Background::Color(t.surface_sunken)),
            border: Border {
                color: t.hairline,
                width: 1.0,
                radius: 2.0.into()
            },
            ..Default::default()
        }),
        list,
    ]
    .spacing(7)
    .into()
}

fn largest_section<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let tree = app.tree().unwrap();
    let root_path = tree.path_string(0);
    let mut ids: Vec<NodeId> = tree.largest_files(24).to_vec();
    ids.retain(|id| app.filter.allows(tree.node(*id).cat));
    if app.sort == SortMode::Name {
        ids.sort_by(|a, b| {
            tree.name(*a)
                .to_lowercase()
                .cmp(&tree.name(*b).to_lowercase())
        });
    }
    let mut list = iced::widget::Column::new().spacing(0);
    for id in ids.into_iter().take(5) {
        let node = tree.node(id);
        let name = tree.name(id);
        let parent = tree.path_string(node.parent);
        let relative = parent
            .strip_prefix(&root_path)
            .map(|rest| rest.trim_start_matches(['\\', '/']))
            .filter(|rest| !rest.is_empty())
            .unwrap_or("scan root");
        let b = button(
            column![
                row![
                    icon(file_icon(name), 12.0, t.ink_faint),
                    label(theme::truncated(name, 30), 10.5, Weight::Medium, t.ink),
                    iced::widget::Space::new().width(Length::Fill),
                    mono(
                        fmt::bytes_short(node.size),
                        10.0,
                        Weight::Normal,
                        t.ink_muted
                    ),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                label(
                    fmt::short_path(relative, 46),
                    9.2,
                    Weight::Normal,
                    t.ink_faint,
                )
                .font(theme::mono_font(Weight::Normal)),
            ]
            .spacing(1)
            .width(Length::Fill),
        )
        .padding(Padding::from([5.0, 4.0]))
        .width(Length::Fill)
        .on_press(Message::RevealHit(id))
        .style({
            let hover = t.surface_sunken;
            move |_iced: &IcedTheme, status| button::Style {
                background: match status {
                    button::Status::Hovered => Some(Background::Color(hover)),
                    _ => None,
                },
                text_color: t.ink,
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: true,
            }
        });
        list = list.push(b);
    }
    let sort_label = app.sort.label();
    let sort_arrow = if app.sort == SortMode::Size {
        "↓"
    } else {
        "↑"
    };
    let head = row![
        section_heading("Largest files", &t),
        iced::widget::Space::new().width(Length::Fill),
        button(label(
            format!("{sort_label} {sort_arrow}"),
            9.8,
            Weight::Medium,
            t.ink_faint
        ))
        .padding(Padding::from([2.0, 5.0]))
        .on_press(Message::ToggleSort)
        .style({
            let hover = t.surface_sunken;
            move |_iced: &IcedTheme, status| button::Style {
                background: match status {
                    button::Status::Hovered => Some(Background::Color(hover)),
                    _ => None,
                },
                text_color: t.ink_faint,
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: true,
            }
        }),
    ]
    .align_y(Alignment::Center);
    column![head, list].spacing(5).into()
}

fn file_type(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() && ext.len() <= 12 => {
            format!(".{}", ext.to_ascii_uppercase())
        }
        _ => "File".into(),
    }
}
