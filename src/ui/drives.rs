//! Drives ledger: every mounted volume, honest capacity math, one scan action.

use iced::font::Weight;
use iced::widget::{Space, button, column, container, row, scrollable};
use iced::{Alignment, Background, Border, Element, Length, Padding, Theme as IcedTheme};

use crate::model::DriveInfo;
use crate::ui::app::{App, Message};
use crate::ui::components::*;
use crate::ui::icon::{Icon, icon};
use crate::ui::theme::{self, Theme};
use crate::ui::{fmt, view as shell};

pub fn view<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let hero = column![
        label(
            if app.drives.is_empty() {
                "Volumes"
            } else {
                "Mounted volumes"
            },
            10.5,
            Weight::Medium,
            t.ink_faint
        ),
        label(headline(app), 27.0, Weight::Semibold, t.ink),
        label(
            "Pick a volume to scan. space only reads metadata and file bytes; nothing is changed.",
            12.5,
            Weight::Normal,
            t.ink_muted
        ),
    ]
    .spacing(7);

    let content = column![
        hero,
        shell::drives_summary(app, &t),
        volumes_panel(app, &t),
        row![
            icon(Icon::Shield, 11.0, t.ink_faint),
            label(
                "Read-only by construction — no delete, move, rename or cleanup action exists.",
                10.5,
                Weight::Normal,
                t.ink_faint
            ),
            iced::widget::Space::new().width(Length::Fill),
            label(
                "Drop a folder anywhere to scan it",
                10.5,
                Weight::Normal,
                t.ink_faint
            ),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    ]
    .spacing(14)
    .max_width(1080.0);

    column![
        shell::app_bar(app, &t),
        container(
            container(content)
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .padding(Padding {
                    top: 26.0,
                    right: 24.0,
                    bottom: 18.0,
                    left: 24.0
                })
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::Start)
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn headline(app: &App) -> String {
    if app.drives.is_empty() {
        return "Choose a volume to begin.".into();
    }
    let low = app
        .drives
        .iter()
        .filter(|d| d.total > 0 && d.used_fraction() >= 0.8)
        .max_by(|a, b| a.used_fraction().total_cmp(&b.used_fraction()));
    if let Some(drive) = low {
        let label = display_drive(drive);
        format!("{} is {} used", label, fmt::fraction(drive.used_fraction()))
    } else {
        let free: u64 = app.drives.iter().map(|d| d.free).sum();
        format!(
            "{} {}, {} free",
            fmt::count(app.drives.len() as u64),
            if app.drives.len() == 1 {
                "volume"
            } else {
                "volumes"
            },
            fmt::bytes_short(free)
        )
    }
}

fn display_drive(d: &DriveInfo) -> String {
    let name = d.name.trim();
    let mount = d.mount.trim_end_matches(['\\', '/']);
    let name = name.trim_end_matches(['\\', '/']);
    if name.is_empty() || name.eq_ignore_ascii_case(mount) {
        mount.to_string()
    } else {
        name.to_string()
    }
}

fn volumes_panel<'a>(app: &'a App, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let header = container(
        row![
            column![
                label("Volumes", theme::TEXT_SM, Weight::Semibold, t.ink),
                label(
                    if app.drives.is_empty() {
                        "Reading mounted disks…".to_string()
                    } else {
                        format!(
                            "{} mounted · sorted by mount point",
                            fmt::count(app.drives.len() as u64)
                        )
                    },
                    theme::TEXT_XS + 0.5,
                    Weight::Normal,
                    t.ink_muted
                ),
            ]
            .spacing(1),
            iced::widget::Space::new().width(Length::Fill),
            icon_button(Icon::Refresh, 13.0, Message::RefreshDrives, t.ink_muted, &t),
            outline_button(
                row![
                    icon(Icon::FolderOpen, 12.0, t.ink),
                    label("Scan a folder", 11.5, Weight::Medium, t.ink),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
                Message::PickFolder,
                &t,
            ),
        ]
        .align_y(Alignment::Center)
        .spacing(7),
    )
    .height(Length::Fixed(54.0))
    .padding(Padding::from([0.0, 14.0]));

    let mut list = iced::widget::Column::new().spacing(0).width(Length::Fill);
    if app.drives_loading && app.drives.is_empty() {
        list = list.push(
            container(
                row![
                    icon(Icon::Refresh, 14.0, t.ink_faint),
                    label("Reading volumes…", 12.0, Weight::Normal, t.ink_muted),
                ]
                .spacing(8),
            )
            .padding(28),
        );
    } else if app.drives.is_empty() {
        list = list.push(
            container(
                column![
                    label("No volumes reported", 13.0, Weight::Semibold, t.ink),
                    label(
                        "The system did not expose a mounted disk. Refresh to try again.",
                        11.5,
                        Weight::Normal,
                        t.ink_muted
                    ),
                ]
                .spacing(4),
            )
            .padding(28),
        );
    } else {
        for (i, drive) in app.drives.iter().enumerate() {
            if i > 0 {
                list = list.push(hline(t.hairline));
            }
            list = list.push(volume_row(drive, &t));
        }
    }

    container(column![
        header,
        hline(t.hairline),
        scrollable(list).height(Length::Fill),
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
    })
    .into()
}

fn volume_row<'a>(drive: &'a DriveInfo, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    let name = display_drive(drive);
    let mount = drive.mount.trim_end_matches(['\\', '/']);
    let name_is_mount = name.eq_ignore_ascii_case(mount);
    let meta = match (name_is_mount, drive.fs.is_empty()) {
        (true, true) => kind_label(drive).to_string(),
        (true, false) => format!("{} · {}", drive.fs.to_ascii_uppercase(), kind_label(drive)),
        (false, true) => format!("{mount} · {}", kind_label(drive)),
        (false, false) => format!(
            "{mount} · {} · {}",
            drive.fs.to_ascii_uppercase(),
            kind_label(drive)
        ),
    };
    let used = drive.used();
    let fraction = drive.used_fraction();
    let warning = fraction >= 0.9;
    let _low = fraction >= 0.8;

    let action = row![
        label(
            if warning { "Low space" } else { "Scan" },
            10.5,
            Weight::Medium,
            if warning { t.danger } else { t.ink_muted }
        ),
        icon(
            Icon::ArrowRight,
            12.0,
            if warning { t.danger } else { t.ink_faint }
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let content = row![
        container(icon(Icon::Drive, 18.0, t.ink_muted))
            .width(Length::Fixed(34.0))
            .height(Length::Fixed(34.0))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| container::Style {
                background: Some(Background::Color(t.surface_sunken)),
                border: Border {
                    color: t.hairline,
                    width: 1.0,
                    radius: 5.0.into()
                },
                ..Default::default()
            }),
        column![
            row![
                label(theme::truncated(&name, 34), 13.5, Weight::Semibold, t.ink),
                label(
                    theme::truncated(&meta, 52),
                    10.5,
                    Weight::Normal,
                    t.ink_faint
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                usage_bar(fraction, &t, 6.0),
                label(
                    format!("{} free", fmt::bytes_short(drive.free)),
                    10.5,
                    Weight::Normal,
                    if warning { t.danger } else { t.ink_muted }
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(8)
        .width(Length::Fill),
        column![
            mono(fmt::bytes_short(used), 14.0, Weight::Semibold, t.ink),
            label(
                format!("{} used", fmt::percent(used, drive.total)),
                10.0,
                Weight::Normal,
                t.ink_faint
            ),
        ]
        .spacing(2)
        .width(Length::Fixed(96.0))
        .align_x(Alignment::End),
        column![
            mono(
                fmt::bytes_short(drive.total),
                12.0,
                Weight::Normal,
                t.ink_muted
            ),
            label("capacity", 9.8, Weight::Normal, t.ink_faint),
        ]
        .spacing(2)
        .width(Length::Fixed(78.0))
        .align_x(Alignment::End),
        container(action)
            .width(Length::Fixed(68.0))
            .align_x(Alignment::End),
    ]
    .spacing(14)
    .padding(Padding::from([12.0, 14.0]))
    .align_y(Alignment::Center)
    .width(Length::Fill);

    button(content)
        .padding(0)
        .width(Length::Fill)
        .on_press(Message::StartScan(std::path::PathBuf::from(&drive.mount)))
        .style({
            let hover = t.surface_raised;
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
        })
        .into()
}

fn kind_label(drive: &DriveInfo) -> &'static str {
    if drive.is_removable || drive.kind == "removable" {
        "removable"
    } else {
        match drive.kind.as_str() {
            "ssd" => "SSD",
            "hdd" => "HDD",
            _ => "disk",
        }
    }
}

fn _unused(_: Space) {}
