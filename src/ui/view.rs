//! Root layout and the explorer composition.

use iced::font::Weight;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::widget::{button, column, container, row, scrollable, text_input};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Point, Rectangle, Renderer,
    Size, Theme as IcedTheme,
};

use crate::model::{Category, ScanSummary};
use crate::ui::app::{
    APP_BAR_HEIGHT, App, ExplorerMode, Message, STATUS_HEIGHT, SUMMARY_HEIGHT, Screen,
    navigation_availability,
};
use crate::ui::canvas::{CanvasPalette, TreemapCanvas, TreemapGpuProgram};
use crate::ui::components::*;
use crate::ui::icon::{Icon, icon};
use crate::ui::theme::{self, Theme};
use crate::ui::{FILTER_INPUT, fmt};

/// Breadcrumb labels start at the scan root. A drive root gets its trailing
/// separator back (`C:` -> `C:\`); every other root uses the scanned folder's
/// own name rather than a tail-elided absolute path.
pub(crate) fn root_crumb_label(name: &str) -> String {
    if name.len() == 2 && name.ends_with(':') {
        format!("{name}\\")
    } else {
        name.to_string()
    }
}

/// Visible breadcrumb entries. Long lineages collapse to root, an ellipsis,
/// the two nearest ancestors and the current folder. The ellipsis opens the
/// hidden-ancestor menu rather than silently jumping to one level.
pub(crate) fn breadcrumb_entries(
    lineage: &[crate::model::NodeId],
) -> Vec<(crate::model::NodeId, bool, bool)> {
    let Some(&root) = lineage.first() else {
        return Vec::new();
    };
    if lineage.len() > 4 {
        vec![
            (root, false, false),
            (lineage[lineage.len() - 3], true, false),
            (lineage[lineage.len() - 2], false, false),
            (lineage[lineage.len() - 1], false, true),
        ]
    } else {
        lineage
            .iter()
            .enumerate()
            .map(|(index, id)| (*id, false, index + 1 == lineage.len()))
            .collect()
    }
}

/// Ancestors hidden between the root and the last two visible crumbs.
pub(crate) fn hidden_ancestors(lineage: &[crate::model::NodeId]) -> &[crate::model::NodeId] {
    if lineage.len() <= 4 {
        &[]
    } else {
        &lineage[1..lineage.len() - 3]
    }
}

fn folders_note(dirs: u64, errors: u64) -> String {
    if errors > 0 {
        format!("{} skipped", fmt::count(errors))
    } else if dirs == 0 {
        "files only".to_string()
    } else {
        "indexed".to_string()
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    let t = Theme::for_dark(app.dark);
    let base: Element<'_, Message> = match app.screen {
        Screen::Drives => super::drives::view(app, &t),
        Screen::Scan => super::scan::view(app, &t),
        Screen::Explorer => explorer(app, &t),
    };
    let mut stack = iced::widget::Stack::new()
        .width(Length::Fill)
        .height(Length::Fill)
        .push(base);
    if app.palette_open {
        stack = stack.push(super::palette::view(app, &t));
    }
    if let Some(context) = &app.context {
        stack = stack.push(super::overlays::context_menu(app, &t, context));
    }
    if app.crumbs_open {
        stack = stack.push(super::overlays::breadcrumb_menu(app, &t));
    }
    if app.settings_open {
        stack = stack.push(super::overlays::settings_panel(app, &t));
    }
    if app.drag_hover {
        stack = stack.push(super::overlays::drop_overlay(&t));
    }
    if let Some(toast) = &app.toast {
        stack = stack.push(super::overlays::toast(app, &t, toast));
    }
    stack.into()
}

fn explorer<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let Some(output) = &app.output else {
        return drives_fallback(app, &t);
    };

    let toolbar = toolbar(app, &t);
    let map = map_content(app, &t);
    let legend = legend(app, &t, &output.summary);

    let map_panel = container(column![
        toolbar,
        hline(t.hairline),
        map,
        hline(t.hairline),
        legend,
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 7.0.into(),
        },
        ..Default::default()
    });

    let body: Element<'a, Message> = if app.inspector_open {
        row![map_panel, super::inspector::view(app, &t).map(|m| m),]
            .spacing(12)
            .height(Length::Fill)
            .into()
    } else {
        map_panel.into()
    };

    let status = status_bar(app, &t);

    column![
        app_bar(app, &t),
        container(summary_strip(app, &t, &output.summary)).padding(Padding {
            top: 12.0,
            right: 16.0,
            bottom: 0.0,
            left: 16.0
        }),
        container(body)
            .padding(Padding {
                top: 12.0,
                right: 16.0,
                bottom: 0.0,
                left: 16.0
            })
            .height(Length::Fill),
        status,
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn drives_fallback<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let base = column![
        app_bar(app, &t),
        centered(
            column![
                label("No scan loaded", theme::TEXT_LG, Weight::Semibold, t.ink),
                label(
                    "Choose a volume on the drives screen to begin.",
                    theme::TEXT_BASE,
                    Weight::Normal,
                    t.ink_muted
                ),
                outline_button(
                    row![
                        icon(Icon::ArrowLeft, 14.0, t.ink),
                        label("Volumes", 12.0, Weight::Medium, t.ink)
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                    Message::Go(Screen::Drives),
                    &t,
                ),
            ]
            .spacing(10)
            .align_x(Alignment::Center)
        ),
    ];
    base.into()
}

pub fn app_bar<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let nav = |name: &'static str, screen: Screen, active: bool| {
        let mut b = button(label(
            name,
            theme::TEXT_SM,
            if active {
                Weight::Semibold
            } else {
                Weight::Normal
            },
            if active { t.ink } else { t.ink_muted },
        ))
        .padding(Padding::from([5.0, 9.0]))
        .style({
            let hover = t.surface_sunken;
            let ink = if active { t.ink } else { t.ink_muted };
            move |_iced: &IcedTheme, status| button::Style {
                background: match status {
                    button::Status::Hovered => Some(Background::Color(hover)),
                    _ if active => Some(Background::Color(hover)),
                    _ => None,
                },
                text_color: ink,
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: true,
            }
        });
        if !active {
            b = b.on_press(Message::Go(screen));
        }
        b
    };

    let read_only = container(
        row![
            icon(Icon::Shield, 11.0, t.ink_faint),
            label(
                "Read-only",
                theme::TEXT_XS + 0.5,
                Weight::Medium,
                t.ink_muted
            ),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([4.0, 8.0]))
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface_sunken)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 999.0.into(),
        },
        ..Default::default()
    });

    let compact = app.window_size.width < 1180.0;
    let palette = if compact {
        button(
            row![icon(Icon::Command, 12.0, t.ink_muted), keycap("Ctrl K", &t),]
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .padding(Padding::from([5.0, 8.0]))
        .on_press(Message::PaletteToggle)
        .style({
            let surface = t.surface_sunken;
            let hover = t.hairline;
            move |_iced: &IcedTheme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered | button::Status::Pressed => hover,
                    _ => surface,
                })),
                text_color: t.ink_muted,
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: true,
            }
        })
    } else {
        button(
            row![
                icon(Icon::Command, 12.0, t.ink_muted),
                label("Commands", theme::TEXT_SM, Weight::Medium, t.ink_muted),
                keycap("Ctrl K", &t),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([5.0, 8.0]))
        .on_press(Message::PaletteToggle)
        .style({
            let surface = t.surface_sunken;
            let hover = t.hairline;
            let ink = t.ink_muted;
            move |_iced: &IcedTheme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered | button::Status::Pressed => hover,
                    _ => surface,
                })),
                text_color: ink,
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: true,
            }
        })
    };

    let theme_icon = if app.dark { Icon::Sun } else { Icon::Moon };
    let bar = container(
        row![
            row![mark(&t), label("space", 16.0, Weight::Semibold, t.ink),]
                .spacing(8)
                .align_y(Alignment::Center),
            iced::widget::Space::new().width(Length::Fill),
            row![
                nav("Drives", Screen::Drives, app.screen == Screen::Drives),
                nav("Explorer", Screen::Explorer, app.screen == Screen::Explorer),
            ]
            .spacing(2),
            iced::widget::Space::new().width(Length::Fill),
            row![
                read_only,
                icon_button(theme_icon, 15.0, Message::ThemeToggle, t.ink_muted, &t),
                icon_button(
                    Icon::Settings,
                    15.0,
                    Message::ToggleSettings,
                    if app.settings_open {
                        t.ink
                    } else {
                        t.ink_muted
                    },
                    &t
                ),
                palette,
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        ]
        .align_y(Alignment::Center)
        .spacing(14),
    )
    .height(Length::Fixed(APP_BAR_HEIGHT))
    .padding(Padding::from([0.0, 14.0]))
    .width(Length::Fill)
    .style({
        move |_| container::Style {
            background: Some(Background::Color(t.surface)),
            ..Default::default()
        }
    });

    column![bar, hline(t.hairline)].into()
}

fn mark(t: &Theme) -> Element<'static, Message> {
    let t = *t;
    let accent = t.accent;
    let faint = Color {
        a: 0.35,
        ..t.accent
    };
    iced::widget::canvas(Mark { accent, faint })
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0))
        .into()
}

struct Mark {
    accent: Color,
    faint: Color,
}

impl canvas::Program<Message> for Mark {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &IcedTheme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let w = 4.0;
        let gap = 3.0;
        let heights = [8.0, 16.0, 11.0];
        for (i, h) in heights.iter().enumerate() {
            let x = i as f32 * (w + gap);
            let y = bounds.height - h;
            f.fill(
                &Path::rounded_rectangle(Point::new(x, y), Size::new(w, *h), 1.2.into()),
                if i == 1 { self.accent } else { self.faint },
            );
        }
        vec![f.into_geometry()]
    }
}

pub fn summary_strip<'a>(app: &'a App, t: &Theme, summary: &ScanSummary) -> Element<'a, Message> {
    let t = *t;
    let largest = app.tree().and_then(|tree| {
        tree.largest_files(1).first().map(|id| {
            (
                fmt::bytes_short(tree.node(*id).size),
                tree.name(*id).to_string(),
            )
        })
    });
    let free = drive_for(app, &summary.root_path).map(|d| {
        (
            fmt::bytes_short(d.free),
            format!("of {} total", fmt::bytes_short(d.total)),
        )
    });
    let cells = vec![
        summary_cell(
            "Scanned",
            fmt::bytes_short(summary.total_bytes),
            format!("{} files", fmt::count(summary.files)),
            &t,
        ),
        summary_cell(
            "Folders",
            fmt::count(summary.dirs),
            folders_note(summary.dirs, summary.errors),
            &t,
        ),
        summary_cell(
            "Largest file",
            largest
                .as_ref()
                .map(|l| l.0.clone())
                .unwrap_or_else(|| "—".into()),
            largest
                .as_ref()
                .map(|l| fmt::short_path(&l.1, 28))
                .unwrap_or_else(|| "no files".into()),
            &t,
        ),
        summary_cell(
            "Free on volume",
            free.as_ref()
                .map(|f| f.0.clone())
                .unwrap_or_else(|| "—".into()),
            free.as_ref()
                .map(|f| f.1.clone())
                .unwrap_or_else(|| "volume unavailable".into()),
            &t,
        ),
    ];
    let mut row = iced::widget::Row::new().height(Length::Fixed(SUMMARY_HEIGHT));
    for (i, cell) in cells.into_iter().enumerate() {
        if i > 0 {
            row = row.push(vline(t.hairline));
        }
        row = row.push(cell);
    }
    container(row)
        .width(Length::Fill)
        .height(Length::Fixed(SUMMARY_HEIGHT))
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

fn summary_cell<'a>(
    caption: impl Into<String>,
    value: String,
    note: impl Into<String>,
    t: &Theme,
) -> Element<'a, Message> {
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
    .padding(Padding::from([14.0, 20.0]))
    .align_y(Alignment::Center)
    .into()
}

fn find_drive<'a>(app: &'a App, path: &str) -> Option<&'a crate::model::DriveInfo> {
    app.drives
        .iter()
        .filter(|d| {
            let mount = d.mount.trim_end_matches(['\\', '/']);
            path.len() >= mount.len() && path[..mount.len()].eq_ignore_ascii_case(mount)
        })
        .max_by_key(|d| d.mount.len())
}

fn drive_for<'a>(app: &'a App, path: &str) -> Option<&'a crate::model::DriveInfo> {
    find_drive(app, path)
}

fn mode_switch<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let tab = |title: &'static str, mode: ExplorerMode| {
        let active = app.mode == mode;
        button(label(
            title,
            theme::TEXT_SM,
            if active {
                Weight::Semibold
            } else {
                Weight::Normal
            },
            if active { t.ink } else { t.ink_muted },
        ))
        .padding(Padding::from([4.0, 9.0]))
        .on_press(Message::SetMode(mode))
        .style({
            let surface = t.surface;
            let _sunken = t.surface_sunken;
            move |_iced: &IcedTheme, status| button::Style {
                background: Some(Background::Color(if active {
                    surface
                } else if matches!(status, button::Status::Hovered) {
                    t.hairline
                } else {
                    Color::TRANSPARENT
                })),
                text_color: if active { t.ink } else { t.ink_muted },
                border: if active {
                    Border {
                        color: t.hairline,
                        width: 1.0,
                        radius: 3.0.into(),
                    }
                } else {
                    Border::default()
                },
                shadow: iced::Shadow::default(),
                snap: true,
            }
        })
    };
    let _ = (t.surface_sunken,);
    container(
        row![
            tab("Map", ExplorerMode::Map),
            tab("Composition", ExplorerMode::Categories)
        ]
        .spacing(1),
    )
    .padding(1)
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

fn rail_toggle<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let icon_color = if app.inspector_open {
        t.ink
    } else {
        t.ink_faint
    };
    icon_button(Icon::Panel, 15.0, Message::ToggleRail, icon_color, &t).into()
}

fn toolbar<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let Some(tree) = app.tree() else {
        return iced::widget::Space::new().into();
    };
    let root = tree.root();
    let lineage = tree.lineage(app.current);
    let nav = navigation_availability(app.history_pos, app.history.len(), app.current, root);
    let back = navigation_button(Icon::ArrowLeft, Message::Back, nav.can_back, "Back", &t);
    let forward = navigation_button(
        Icon::ArrowRight,
        Message::Forward,
        nav.can_forward,
        "Forward",
        &t,
    );
    let up = navigation_button(
        Icon::UpLeft,
        Message::GoUp,
        nav.can_up,
        "Go up one level",
        &t,
    );

    let root_label = root_crumb_label(tree.name(root));
    let entries = breadcrumb_entries(&lineage);

    let mut crumbs: Vec<Element<'a, Message>> = Vec::new();
    for (index, (id, ellipsis, current)) in entries.iter().enumerate() {
        if index > 0 {
            crumbs.push(icon(Icon::ChevronRight, 11.0, t.ink_faint));
        }
        if *ellipsis {
            crumbs.push(crumb_ellipsis(app, &t));
        } else {
            let name = if *id == root {
                root_label.clone()
            } else {
                tree.name(*id).to_string()
            };
            crumbs.push(crumb_button(name, *id, *current, &t));
        }
    }

    let volumes = button(label("Volumes", 11.5, Weight::Medium, t.ink_muted))
        .padding(Padding::from([4.0, 6.0]))
        .on_press(Message::Go(Screen::Drives))
        .style(quiet_text_style(t));
    let filter = filter_input(app, &t);
    let identity = app.camera.is_identity();
    let refit_color = if identity {
        Color {
            a: 0.38,
            ..t.ink_faint
        }
    } else {
        t.ink_muted
    };
    let mut refit = button(
        row![
            icon(Icon::Maximize, 12.0, refit_color),
            label("Refit", 11.5, Weight::Medium, refit_color),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([4.0, 7.0]))
    .width(Length::Fixed(64.0))
    .style({
        let hover = t.surface_sunken;
        move |_iced: &IcedTheme, status| button::Style {
            background: if !identity
                && matches!(status, button::Status::Hovered | button::Status::Pressed)
            {
                Some(Background::Color(hover))
            } else {
                None
            },
            text_color: refit_color,
            border: Border::default(),
            shadow: iced::Shadow::default(),
            snap: true,
        }
    });
    if !identity {
        refit = refit.on_press(Message::Refit);
    }

    container(
        row![
            row![back, forward, up, volumes]
                .spacing(2)
                .align_y(Alignment::Center),
            row(crumbs).spacing(1).align_y(Alignment::Center),
            iced::widget::Space::new().width(Length::Fill),
            refit,
            filter,
            mode_switch(app, &t),
            rail_toggle(app, &t),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(44.0))
    .padding(Padding::from([0.0, 12.0]))
    .style(|_| container::Style::default())
    .into()
}

fn crumb_ellipsis<'a>(app: &App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let open = app.crumbs_open;
    button(label("…", 15.0, Weight::Semibold, t.ink_muted))
        .width(Length::Fixed(30.0))
        .height(Length::Fixed(28.0))
        .padding(0)
        .on_press(Message::ToggleCrumbMenu)
        .style({
            let hover = t.surface_sunken;
            move |_iced: &IcedTheme, status| button::Style {
                background: if open
                    || matches!(status, button::Status::Hovered | button::Status::Pressed)
                {
                    Some(Background::Color(hover))
                } else {
                    None
                },
                text_color: t.ink_muted,
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: true,
            }
        })
        .into()
}

fn crumb_button<'a>(
    name: String,
    id: crate::model::NodeId,
    current: bool,
    t: &Theme,
) -> Element<'a, Message> {
    let t = *t;
    let display = if id == 0 && name.len() == 2 && name.ends_with(':') {
        format!("{name}\\")
    } else {
        name
    };
    let display = if current {
        theme::truncated(&display, 30)
    } else {
        display
    };
    button(label(
        display,
        if current { 12.5 } else { 12.0 },
        if current {
            Weight::Semibold
        } else {
            Weight::Normal
        },
        if current { t.ink } else { t.ink_muted },
    ))
    .padding(Padding::from([3.0, 4.0]))
    .on_press(Message::Drill(id))
    .style(quiet_text_style(t))
    .into()
}

fn navigation_button<'a>(
    glyph: Icon,
    message: Message,
    enabled: bool,
    tooltip: &'static str,
    t: &Theme,
) -> Element<'a, Message> {
    let t = *t;
    let color = if enabled {
        t.ink_muted
    } else {
        Color {
            a: 0.38,
            ..t.ink_faint
        }
    };
    let glyph = container(icon(glyph, 16.0, color))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    let button = button(glyph)
        .width(Length::Fixed(30.0))
        .height(Length::Fixed(30.0))
        .padding(0)
        .style({
            let hover = t.surface_sunken;
            move |_iced: &IcedTheme, status| button::Style {
                background: if enabled
                    && matches!(status, button::Status::Hovered | button::Status::Pressed)
                {
                    Some(Background::Color(hover))
                } else {
                    None
                },
                text_color: color,
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: true,
            }
        });
    let button = if enabled {
        button.on_press(message)
    } else {
        button
    };
    iced::widget::tooltip(button, tooltip, iced::widget::tooltip::Position::Bottom).into()
}

fn quiet_text_style(t: Theme) -> impl Fn(&IcedTheme, button::Status) -> button::Style + 'static {
    let hover = t.surface_sunken;
    move |_iced: &IcedTheme, status| button::Style {
        background: match status {
            button::Status::Hovered | button::Status::Pressed => Some(Background::Color(hover)),
            _ => None,
        },
        text_color: t.ink_muted,
        border: Border::default(),
        shadow: iced::Shadow::default(),
        snap: true,
    }
}

fn filter_input<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let input = text_input("Filter this level", &app.filter.query)
        .id(FILTER_INPUT)
        .on_input(Message::FilterChanged)
        .padding(Padding::from([4.0, 0.0]))
        .size(11.5)
        .font(theme::ui_font(Weight::Normal))
        .style({
            let ink = t.ink;
            let muted = t.ink_muted;
            move |_iced: &IcedTheme, _status| text_input::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                icon: muted,
                placeholder: t.ink_faint,
                value: ink,
                selection: t.selection,
            }
        })
        .width(Length::Fixed(150.0));
    let clear: Element<'a, Message> = if app.filter.query.is_empty() {
        iced::widget::Space::new().into()
    } else {
        icon_button(Icon::Close, 10.0, Message::ClearFilters, t.ink_faint, &t)
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(20.0))
            .into()
    };
    let hidden = app.filter.hidden_count();
    let tail: Element<'a, Message> = if hidden > 0 {
        button(label(
            format!("{hidden} hidden"),
            10.5,
            Weight::Medium,
            t.ink_muted,
        ))
        .padding(Padding::from([2.0, 4.0]))
        .on_press(Message::ClearFilters)
        .style(quiet_text_style(t))
        .into()
    } else {
        iced::widget::Space::new().into()
    };
    container(
        row![icon(Icon::Search, 12.0, t.ink_faint), input, clear, tail,]
            .spacing(4)
            .align_y(Alignment::Center),
    )
    .height(Length::Fixed(29.0))
    .padding(Padding::from([0.0, 6.0]))
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
}

fn map_content<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let Some(output) = &app.output else {
        return iced::widget::Space::new().into();
    };
    if app.mode == ExplorerMode::Categories {
        return composition_view(app, &t);
    }
    let palette = CanvasPalette {
        bed: t.bg,
        ink: t.ink,
        ink_muted: t.ink_muted,
        ink_faint: t.ink_faint,
        hairline: t.hairline,
        accent: t.accent,
        category_fill: t.category_fill,
        category_ink: t.category_ink,
        category_line: [
            t.category_line(Category::Media),
            t.category_line(Category::Documents),
            t.category_line(Category::Code),
            t.category_line(Category::Caches),
            t.category_line(Category::Apps),
            t.category_line(Category::Archives),
            t.category_line(Category::System),
            t.category_line(Category::Other),
        ],
    };
    let query_active = !app.filter.query.trim().is_empty();
    let no_matches = app
        .filter_view
        .as_ref()
        .map(|view| view.match_count == 0)
        .unwrap_or(!query_active);
    let empty = if query_active && no_matches {
        Some(format!(
            "No entries under this folder match “{}”.",
            theme::truncated(app.filter.query.trim(), 32)
        ))
    } else if !query_active && app.visible_children(app.current).is_empty() {
        Some(if app.filter.hidden_count() > 0 {
            "Every item here is hidden by the category filters.".to_string()
        } else {
            "This folder is empty.".to_string()
        })
    } else {
        None
    };
    let gpu = TreemapGpuProgram {
        output: output.clone(),
        dir: app.current,
        hidden: app.filter.hidden,
        filter: app.filter_view.clone(),
        camera: app.camera,
        palette: palette.clone(),
    };
    let (art, icons) = app.tiles.handles();
    let overlay = TreemapCanvas {
        output: output.clone(),
        dir: app.current,
        hidden: app.filter.hidden,
        query: app.filter.query.clone(),
        filter: app.filter_view.clone(),
        camera: app.camera,
        selected: app.selection,
        palette: palette.clone(),
        empty_hint: empty,
        fills: false,
        art,
        icons,
        report: app.tile_report.clone(),
    };
    let layers: Element<'a, Message> = iced::widget::Stack::new()
        .width(Length::Fill)
        .height(Length::Fill)
        .push(
            iced::widget::shader(gpu)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .push(
            iced::widget::canvas(overlay)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .into();
    let bed = palette.bed;
    container(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(8)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(bed)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn composition_view<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let totals = app.composition;
    let total = totals.total().max(1);
    let Some(_output) = &app.output else {
        return iced::widget::Space::new().into();
    };

    let mut bar = iced::widget::Row::new().height(Length::Fixed(12.0));
    for cat in Category::ALL {
        let bytes = totals.get(cat);
        if bytes == 0 {
            continue;
        }
        let portion = ((bytes as f64 / total as f64) * 1000.0).round().max(1.0) as u16;
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

    let mut rows = iced::widget::Column::new().spacing(0);
    for cat in Category::ALL {
        let bytes = totals.get(cat);
        let _pct = if total > 1 {
            bytes as f32 / total as f32
        } else {
            0.0
        };
        let hidden = app.filter.hidden[cat.id() as usize];
        let swatch = category_swatch(cat, &t, 9.0);
        let name_color = if hidden { t.ink_faint } else { t.ink };
        let pct_text = fmt::percent(bytes, total);
        let toggle = button(
            row![
                swatch,
                label(cat.label(), 12.0, Weight::Medium, name_color),
                iced::widget::Space::new().width(Length::Fill),
                mono(
                    fmt::bytes_short(bytes),
                    11.5,
                    Weight::Medium,
                    if hidden { t.ink_faint } else { t.ink }
                ),
                mono(pct_text, 11.0, Weight::Normal, t.ink_faint),
                label(
                    if hidden { "hidden" } else { "" },
                    10.0,
                    Weight::Normal,
                    t.ink_faint
                ),
            ]
            .spacing(9)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([10.0, 12.0]))
        .width(Length::Fill)
        .on_press(Message::ToggleCategory(cat))
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
        rows = rows.push(toggle);
        rows = rows.push(hline(t.hairline));
    }

    let body = column![
        row![
            column![
                label("Category composition", theme::TEXT_MD, Weight::Semibold, t.ink),
                label(
                    "Exact bytes under the current folder. Click a row to hide or show it on the map.",
                    theme::TEXT_SM,
                    Weight::Normal,
                    t.ink_muted
                ),
            ]
            .spacing(3),
            iced::widget::Space::new().width(Length::Fill),
            mono(fmt::bytes(totals.total()), 18.0, Weight::Semibold, t.ink),
        ]
        .align_y(Alignment::End),
        container(bar).clip(true).style(move |_| container::Style {
            background: Some(Background::Color(t.surface_sunken)),
            border: Border { color: t.hairline, width: 1.0, radius: 3.0.into() },
            ..Default::default()
        }),
        container(rows).style(move |_| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.hairline, width: 1.0, radius: 5.0.into() },
            ..Default::default()
        }),
        if app.composition_loading {
            label("Recomputing exact composition…", 11.0, Weight::Normal, t.ink_faint)
        } else {
            label(
                "Counts and bytes are read from the scan. Nothing on disk is modified.",
                11.0,
                Weight::Normal,
                t.ink_faint,
            )
        },
    ]
    .spacing(14)
    .padding(24);

    scrollable(body)
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

fn legend<'a>(app: &'a App, t: &Theme, _summary: &ScanSummary) -> Element<'a, Message> {
    let t = *t;
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    for cat in Category::ALL {
        let hidden = app.filter.hidden[cat.id() as usize];
        let swatch = category_swatch(cat, &t, 8.0);
        let color = if hidden { t.ink_faint } else { t.ink_muted };
        let b = button(
            row![swatch, label(cat.label(), 10.5, Weight::Medium, color),]
                .spacing(5)
                .align_y(Alignment::Center),
        )
        .padding(Padding::from([3.0, 5.0]))
        .on_press(Message::ToggleCategory(cat))
        .style(quiet_text_style(t));
        items.push(b.into());
    }
    let hidden = app.filter.hidden_count();
    let tail: Element<'a, Message> = if hidden > 0 {
        row![
            label(
                format!("{hidden} hidden"),
                10.5,
                Weight::Medium,
                t.ink_muted
            ),
            button(label("Show all", 10.5, Weight::Medium, t.accent))
                .padding(Padding::from([2.0, 4.0]))
                .on_press(Message::ClearFilters)
                .style(quiet_text_style(t)),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    } else {
        label(
            "Click to hide a category",
            10.5,
            Weight::Normal,
            t.ink_faint,
        )
        .into()
    };
    container(
        row![
            iced::widget::Row::with_children(items).spacing(0),
            iced::widget::Space::new().width(Length::Fill),
            tail,
        ]
        .align_y(Alignment::Center),
    )
    .height(Length::Fixed(34.0))
    .width(Length::Fill)
    .padding(Padding::from([0.0, 12.0]))
    .style(|_| container::Style::default())
    .into()
}

fn status_bar<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let Some(tree) = app.tree() else {
        return iced::widget::Space::new().into();
    };
    let query_active = !app.filter.query.trim().is_empty();
    let children = app.visible_children(app.current);
    let view = app.filter_view.as_deref();
    let visible_bytes: u64 = children.iter().map(|id| tree.node(*id).size).sum();
    let (count, files, bytes, noun, bounded) = if query_active {
        (
            view.map(|view| view.match_count as u64).unwrap_or(0),
            view.map(|view| view.matched_files).unwrap_or(0),
            view.map(|view| view.matched_bytes).unwrap_or(0),
            "matches",
            view.is_some_and(|view| view.truncated),
        )
    } else {
        (
            children.len() as u64,
            tree.node(app.current).file_count,
            visible_bytes,
            "items",
            false,
        )
    };
    let hover = app.hover.or(app.selection);
    let hover_text = hover
        .map(|id| tree.path_string(id))
        .map(|p| fmt::short_path(&p, 58))
        .unwrap_or_else(|| {
            if query_active {
                format!(
                    "Filter “{}” · this subtree{}",
                    theme::truncated(app.filter.query.trim(), 28),
                    if bounded { " · bounded" } else { "" }
                )
            } else if app.filter.hidden_count() > 0 {
                let hidden_noun = if app.filter.hidden_count() == 1 {
                    "category"
                } else {
                    "categories"
                };
                format!(
                    "{} {hidden_noun} hidden · {} of {} shown",
                    app.filter.hidden_count(),
                    children.len(),
                    tree.child_count(app.current)
                )
            } else {
                "Read-only · nothing on disk is modified".into()
            }
        });
    let zoom = (app.camera.scale * 100.0).round() as i32;
    container(
        row![
            mono(
                format!(
                    "{} {noun} · {} files · {}{}",
                    fmt::count(count),
                    fmt::count(files),
                    fmt::bytes_short(bytes),
                    if bounded { " · bounded" } else { "" }
                ),
                10.5,
                Weight::Normal,
                t.ink_faint
            ),
            iced::widget::Space::new().width(Length::Fill),
            mono(hover_text, 10.5, Weight::Normal, t.ink_faint),
            iced::widget::Space::new().width(Length::Fill),
            mono(
                format!(
                    "{zoom}% · {} · read-only",
                    fmt::elapsed(
                        app.output
                            .as_ref()
                            .map(|o| o.summary.elapsed_ms)
                            .unwrap_or(0)
                    )
                ),
                10.5,
                Weight::Normal,
                t.ink_faint
            ),
        ]
        .align_y(Alignment::Center),
    )
    .height(Length::Fixed(STATUS_HEIGHT))
    .width(Length::Fill)
    .padding(Padding::from([0.0, 16.0]))
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

pub fn drives_summary<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let total: u64 = app.drives.iter().map(|d| d.total).sum();
    let free: u64 = app.drives.iter().map(|d| d.free).sum();
    let used = total.saturating_sub(free);
    let cells = row![
        drives_cell(
            "Volumes",
            fmt::count(app.drives.len() as u64),
            "mounted",
            &t
        ),
        vline(t.hairline),
        drives_cell("Capacity", fmt::bytes_short(total), "across volumes", &t),
        vline(t.hairline),
        drives_cell(
            "In use",
            fmt::bytes_short(used),
            fmt::percent(used, total),
            &t
        ),
        vline(t.hairline),
        drives_cell("Free", fmt::bytes_short(free), "available to use", &t),
    ]
    .height(Length::Fixed(SUMMARY_HEIGHT));
    container(cells)
        .width(Length::Fill)
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

fn drives_cell<'a>(
    caption: impl Into<String>,
    value: String,
    note: impl Into<String>,
    t: &Theme,
) -> Element<'a, Message> {
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
    .padding(Padding::from([14.0, 20.0]))
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_crumb_keeps_the_drive_separator_and_folder_name() {
        assert_eq!(root_crumb_label("C:"), "C:\\");
        assert_eq!(root_crumb_label("space"), "space");
        assert_eq!(root_crumb_label("Downloads"), "Downloads");
    }

    #[test]
    fn long_breadcrumbs_keep_root_ellipsis_and_the_nearest_folders() {
        let lineage = vec![0, 1, 2, 3, 4, 5, 6];
        let entries = breadcrumb_entries(&lineage);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].0, 0, "root stays first");
        assert!(!entries[0].2);
        assert!(entries[1].1, "the second entry is the ancestor menu");
        assert_eq!(entries[1].0, 4);
        assert_eq!(entries[2].0, 5);
        assert_eq!(entries[3].0, 6);
        assert!(entries[3].2, "the current folder is emphasized");
        assert_eq!(hidden_ancestors(&lineage), &[1, 2, 3]);

        let short = breadcrumb_entries(&[0, 1, 2]);
        assert_eq!(short.len(), 3);
        assert!(short.iter().all(|(_, ellipsis, _)| !ellipsis));
        assert!(hidden_ancestors(&[0, 1, 2]).is_empty());
    }

    #[test]
    fn zero_folders_no_longer_claims_indexing_happened() {
        assert_eq!(folders_note(0, 0), "files only");
        assert_eq!(folders_note(12, 0), "indexed");
        assert_eq!(folders_note(12, 3), "3 skipped");
    }
}
