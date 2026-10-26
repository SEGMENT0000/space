//! Small styled primitives shared by every screen.

use iced::font::Weight;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::widget::{Button, Container, Text, button, container, text};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Point, Rectangle, Renderer,
    Shadow, Size, Theme as IcedTheme,
};

use super::Message;
use super::icon::{Icon, icon};
use super::theme::{self, Theme};

pub fn label<'a>(content: impl Into<String>, size: f32, weight: Weight, color: Color) -> Text<'a> {
    text(content.into())
        .size(size)
        .font(theme::ui_font(weight))
        .color(color)
}

pub fn mono<'a>(content: impl Into<String>, size: f32, weight: Weight, color: Color) -> Text<'a> {
    text(content.into())
        .size(size)
        .font(theme::mono_font(weight))
        .color(color)
}

pub fn hline(color: Color) -> Element<'static, Message> {
    container(container(iced::widget::Space::new()))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            ..Default::default()
        })
        .into()
}

pub fn vline(color: Color) -> Element<'static, Message> {
    container(iced::widget::Space::new())
        .width(Length::Fixed(1.0))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            ..Default::default()
        })
        .into()
}

pub fn keycap<'a>(content: impl Into<String>, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    container(mono(
        content,
        theme::TEXT_XS - 0.5,
        Weight::Medium,
        t.ink_muted,
    ))
    .padding(Padding::from([2.0, 5.0]))
    .style(move |_| container::Style {
        background: Some(Background::Color(t.surface_sunken)),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    })
    .into()
}

pub fn icon_button<'a>(
    glyph: Icon,
    size: f32,
    message: Message,
    color: Color,
    t: &Theme,
) -> Button<'a, Message> {
    let t = *t;
    let hover = t.surface_sunken;
    button(icon(glyph, size, color))
        .width(Length::Fixed(30.0))
        .height(Length::Fixed(30.0))
        .padding(0)
        .on_press(message)
        .style(move |_iced: &IcedTheme, status| {
            let background = match status {
                button::Status::Hovered => Some(Background::Color(hover)),
                button::Status::Pressed => Some(Background::Color(hover)),
                _ => None,
            };
            button::Style {
                background,
                text_color: color,
                border: Border::default(),
                shadow: Shadow::default(),
                snap: true,
            }
        })
}

pub fn primary_button<'a>(
    content: impl Into<Element<'a, Message>>,
    message: Message,
    t: &Theme,
) -> Button<'a, Message> {
    let t = *t;
    let base = t.accent;
    let ink = t.accent_ink;
    let dark = t.dark;
    button(content)
        .padding(Padding::from([7.0, 13.0]))
        .on_press(message)
        .style(move |_iced: &IcedTheme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    if dark {
                        Color {
                            r: (base.r - 0.06).max(0.0),
                            g: (base.g - 0.06).max(0.0),
                            b: (base.b - 0.06).max(0.0),
                            a: 1.0,
                        }
                    } else {
                        Color {
                            r: (base.r * 0.88),
                            g: (base.g * 0.88),
                            b: (base.b * 0.88),
                            a: 1.0,
                        }
                    }
                }
                _ => base,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                text_color: ink,
                border: Border {
                    color: base,
                    width: 0.0,
                    radius: 4.0.into(),
                },
                shadow: Shadow::default(),
                snap: true,
            }
        })
}

pub fn outline_button<'a>(
    content: impl Into<Element<'a, Message>>,
    message: Message,
    t: &Theme,
) -> Button<'a, Message> {
    let t = *t;
    let ink = t.ink;
    let hairline = t.hairline_strong;
    let hover = t.surface_sunken;
    let surface = t.surface;
    button(content)
        .padding(Padding::from([6.0, 11.0]))
        .on_press(message)
        .style(move |_iced: &IcedTheme, status| {
            let background = match status {
                button::Status::Hovered | button::Status::Pressed => hover,
                _ => surface,
            };
            button::Style {
                background: Some(Background::Color(background)),
                text_color: ink,
                border: Border {
                    color: hairline,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                shadow: Shadow::default(),
                snap: true,
            }
        })
}

pub fn category_swatch(
    cat: crate::model::Category,
    t: &Theme,
    size: f32,
) -> Element<'static, Message> {
    let t = *t;
    let color = t.category(cat);
    let border = Color {
        a: 0.55,
        ..t.category_ink(cat)
    };
    container(iced::widget::Space::new())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color: border,
                width: 0.5,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
}

pub fn section_heading<'a>(title: impl Into<String>, t: &Theme) -> Element<'a, Message> {
    let t = *t;
    label(title, theme::TEXT_SM, Weight::Semibold, t.ink).into()
}

/// A compact utilization ribbon shared by the drives ledger and summary cells.
pub struct UsageBar {
    pub fraction: f32,
    pub track: Color,
    pub fill: Color,
    pub warning: Color,
    pub danger: Color,
    pub ticks: Color,
}

impl canvas::Program<Message> for UsageBar {
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
        let h = bounds.height;
        let radius = 2.0;
        frame.fill(
            &Path::rounded_rectangle(Point::ORIGIN, Size::new(bounds.width, h), radius.into()),
            self.track,
        );
        let pct = self.fraction.clamp(0.0, 1.0);
        let color = if pct >= 0.92 {
            self.danger
        } else if pct >= 0.8 {
            self.warning
        } else {
            self.fill
        };
        if pct > 0.0 {
            let w = (bounds.width * pct).max(2.0);
            frame.fill(
                &Path::rounded_rectangle(Point::ORIGIN, Size::new(w, h), radius.into()),
                color,
            );
        }
        let tick_count = 36;
        for i in 1..tick_count {
            let x = bounds.width * i as f32 / tick_count as f32;
            if x < bounds.width - 1.0 {
                frame.fill_rectangle(
                    Point::new(x, 1.0),
                    Size::new(1.0, (h - 2.0).max(0.0)),
                    self.ticks,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

pub fn usage_bar(fraction: f32, t: &Theme, height: f32) -> Element<'static, Message> {
    let t = *t;
    let fill = if t.dark { t.accent } else { t.ink };
    iced::widget::canvas(UsageBar {
        fraction,
        track: t.surface_sunken,
        fill,
        warning: t.warning,
        danger: t.danger,
        ticks: Color {
            a: 0.38,
            ..t.surface
        },
    })
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .into()
}

pub fn centered<'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
}
