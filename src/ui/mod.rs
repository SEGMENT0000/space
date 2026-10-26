//! Native Iced shell: application wiring, screens, overlays and design tokens.

pub mod app;
mod canvas;
mod components;
mod drives;
mod filter;
pub mod fmt;
mod icon;
mod inspector;
mod overlays;
mod palette;
mod platform;
mod scan;
mod theme;
mod view;

use std::time::Duration;

use iced::font::Weight;
use iced::widget::{container, text_input};
use iced::{Background, Border, Color, Element, Length, Size};

pub use app::{App, Message};

/// Focus targets are stable ids so keyboard operations never depend on depth.
pub const FOCUS_SINK: iced::widget::Id = iced::widget::Id::new("space::focus-sink");
pub const FILTER_INPUT: iced::widget::Id = iced::widget::Id::new("space::filter");
pub const PALETTE_INPUT: iced::widget::Id = iced::widget::Id::new("space::palette");

/// The map viewport always starts at the origin; the canvas owns its own
/// coordinate space and the camera transforms content into it.
pub fn rect_for(size: Size) -> crate::treemap::Rect {
    crate::treemap::Rect::new(0.0, 0.0, size.width.max(1.0), size.height.max(1.0))
}

pub fn run() -> iced::Result {
    iced::application(App::new, App::update, view)
        .title(|_: &App| "space".to_string())
        .window(iced::window::Settings {
            size: Size::new(1320.0, 860.0),
            min_size: Some(Size::new(1024.0, 680.0)),
            position: iced::window::Position::Centered,
            ..Default::default()
        })
        .subscription(App::subscription)
        .theme(App::theme)
        .font(include_bytes!("../../assets/fonts/Geist-Regular.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/Geist-Medium.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/Geist-SemiBold.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/Geist-Bold.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/GeistMono-Regular.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/GeistMono-Medium.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/GeistMono-SemiBold.ttf").as_slice())
        .default_font(theme::ui_font(Weight::Normal))
        .antialiasing(true)
        .run()
}

fn view(app: &App) -> Element<'_, Message> {
    if app.frame_log {
        app.frames.borrow_mut().record();
    }
    let t = theme::Theme::for_dark(app.dark);
    let base = container(view::view(app))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.bg)),
            text_color: Some(t.ink),
            ..Default::default()
        });

    // A 1×1 transparent input is the focus sink. Iced 0.14 has no unfocus
    // operation; moving focus here is the deterministic way to release a text
    // field without leaving the keyboard trapped. It is visually clipped.
    let sink = text_input("", "")
        .id(FOCUS_SINK)
        .on_input(Message::SinkInput)
        .width(Length::Fixed(1.0))
        .padding(0)
        .size(1.0)
        .style(|_, _| text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            icon: Color::TRANSPARENT,
            placeholder: Color::TRANSPARENT,
            value: Color::TRANSPARENT,
            selection: Color::TRANSPARENT,
        });

    iced::widget::stack![base, sink]
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .into()
}

// Keep the standard imports honest: these names document the intended
// subscription cadence for the shell.
#[allow(dead_code)]
const TICK_IDLE: Duration = Duration::from_millis(220);
#[allow(dead_code)]
const TICK_SCANNING: Duration = Duration::from_millis(60);
