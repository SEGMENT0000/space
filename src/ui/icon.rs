//! One stroke-icon language, drawn from bundled Lucide assets (ISC).

use iced::widget::svg::{self, Svg};
use iced::{Color, Element, Length};

use super::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Drive,
    Folder,
    FolderOpen,
    File,
    Text,
    Image,
    Database,
    Archive,
    Code,
    Music,
    Film,
    App,
    Search,
    Sun,
    Moon,
    Command,
    Copy,
    External,
    ArrowLeft,
    ArrowRight,
    ChevronRight,
    Close,
    Refresh,
    Panel,
    Alert,
    Maximize,
    UpLeft,
    Clock,
    Shield,
    Settings,
}

/// Raw bundled SVG source, used by the on-tile raster cache. The bytes are
/// compile-time assets; nothing is fetched or executed.
pub(crate) fn svg_source(icon: Icon) -> &'static [u8] {
    bytes(icon)
}

fn bytes(icon: Icon) -> &'static [u8] {
    match icon {
        Icon::Drive => include_bytes!("../../assets/icons/hard-drive.svg"),
        Icon::Folder => include_bytes!("../../assets/icons/folder.svg"),
        Icon::FolderOpen => include_bytes!("../../assets/icons/folder-open.svg"),
        Icon::File => include_bytes!("../../assets/icons/file.svg"),
        Icon::Text => include_bytes!("../../assets/icons/file-text.svg"),
        Icon::Image => include_bytes!("../../assets/icons/image.svg"),
        Icon::Database => include_bytes!("../../assets/icons/database.svg"),
        Icon::Archive => include_bytes!("../../assets/icons/archive.svg"),
        Icon::Code => include_bytes!("../../assets/icons/code.svg"),
        Icon::Music => include_bytes!("../../assets/icons/music.svg"),
        Icon::Film => include_bytes!("../../assets/icons/film.svg"),
        Icon::App => include_bytes!("../../assets/icons/app-window.svg"),
        Icon::Search => include_bytes!("../../assets/icons/search.svg"),
        Icon::Sun => include_bytes!("../../assets/icons/sun.svg"),
        Icon::Moon => include_bytes!("../../assets/icons/moon.svg"),
        Icon::Command => include_bytes!("../../assets/icons/command.svg"),
        Icon::Copy => include_bytes!("../../assets/icons/copy.svg"),
        Icon::External => include_bytes!("../../assets/icons/external-link.svg"),
        Icon::ArrowLeft => include_bytes!("../../assets/icons/arrow-left.svg"),
        Icon::ArrowRight => include_bytes!("../../assets/icons/arrow-right.svg"),
        Icon::ChevronRight => include_bytes!("../../assets/icons/chevron-right.svg"),
        Icon::Close => include_bytes!("../../assets/icons/x.svg"),
        Icon::Refresh => include_bytes!("../../assets/icons/refresh-cw.svg"),
        Icon::Panel => include_bytes!("../../assets/icons/panel-right.svg"),
        Icon::Alert => include_bytes!("../../assets/icons/triangle-alert.svg"),
        Icon::Maximize => include_bytes!("../../assets/icons/maximize.svg"),
        Icon::UpLeft => include_bytes!("../../assets/icons/corner-up-left.svg"),
        Icon::Clock => include_bytes!("../../assets/icons/clock.svg"),
        Icon::Shield => include_bytes!("../../assets/icons/shield.svg"),
        Icon::Settings => include_bytes!("../../assets/icons/sliders-horizontal.svg"),
    }
}

pub fn icon(icon: Icon, size: f32, color: Color) -> Element<'static, Message> {
    let handle = svg::Handle::from_memory(bytes(icon));
    Svg::new(handle)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_theme, _status| svg::Style { color: Some(color) })
        .into()
}

pub fn category_icon(cat: crate::model::Category) -> Icon {
    use crate::model::Category as C;
    match cat {
        C::Media => Icon::Film,
        C::Documents => Icon::Text,
        C::Code => Icon::Code,
        C::Caches => Icon::Clock,
        C::Apps => Icon::App,
        C::Archives => Icon::Archive,
        C::System => Icon::Shield,
        C::Other => Icon::File,
    }
}

pub fn file_icon(name: &str) -> Icon {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "svg" => {
            Icon::Image
        }
        "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" | "opus" => Icon::Music,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => Icon::Film,
        "zip" | "tar" | "gz" | "xz" | "7z" | "rar" | "iso" | "dmg" => Icon::Archive,
        "db" | "sqlite" | "sqlite3" => Icon::Database,
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "c" | "cpp" | "h" | "hpp" | "java"
        | "cs" | "rb" | "php" | "swift" | "kt" | "toml" | "json" | "yaml" | "yml" | "css"
        | "html" => Icon::Code,
        "txt" | "md" | "pdf" | "doc" | "docx" | "odt" | "rtf" | "log" | "csv" => Icon::Text,
        "exe" | "msi" | "app" | "dmg_stub" => Icon::App,
        _ => Icon::File,
    }
}
