//! Deterministic colors per file extension category.

use eframe::egui::Color32;

use crate::extensions::ExtensionTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Archive,
    Video,
    Audio,
    Image,
    Code,
    DiskImage,
    Binary,
    Other,
}

impl Category {
    #[must_use]
    pub fn color(self) -> Color32 {
        match self {
            Self::Archive => Color32::from_rgb(230, 126, 34),
            Self::Video => Color32::from_rgb(155, 89, 182),
            Self::Audio => Color32::from_rgb(46, 204, 113),
            Self::Image => Color32::from_rgb(52, 152, 219),
            Self::Code => Color32::from_rgb(241, 196, 15),
            Self::DiskImage => Color32::from_rgb(231, 76, 60),
            Self::Binary => Color32::from_rgb(26, 188, 156),
            Self::Other => Color32::from_rgb(127, 140, 141),
        }
    }
}

const ARCHIVES: &[&str] = &[
    "zip", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "tar", "lz4", "lzma", "cab", "rpm", "deb",
];
const VIDEOS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "wmv", "flv", "m4v", "mpg", "mpeg",
];
const AUDIOS: &[&str] = &["mp3", "flac", "wav", "ogg", "opus", "m4a", "aac", "wma"];
const IMAGES: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "tiff", "ico", "heic", "avif", "psd",
];
const CODE: &[&str] = &[
    "rs", "py", "js", "ts", "c", "h", "cpp", "hpp", "java", "go", "rb", "php", "sh", "css", "html",
    "json", "toml", "yaml", "yml", "xml", "md", "kt", "swift", "cs", "lua",
];
const DISK_IMAGES: &[&str] = &[
    "iso", "img", "qcow2", "vmdk", "vdi", "vhd", "vhdx", "dmg", "squashfs",
];
const BINARIES: &[&str] = &[
    "so", "dll", "exe", "bin", "o", "a", "elf", "dylib", "class", "jar", "pyc", "wasm",
];

/// `extension` must already be lowercase, as produced by `ExtensionTable`.
#[must_use]
pub fn category_of(extension: &str) -> Category {
    let groups = [
        (ARCHIVES, Category::Archive),
        (VIDEOS, Category::Video),
        (AUDIOS, Category::Audio),
        (IMAGES, Category::Image),
        (CODE, Category::Code),
        (DISK_IMAGES, Category::DiskImage),
        (BINARIES, Category::Binary),
    ];
    groups
        .iter()
        .find(|(extensions, _)| extensions.contains(&extension))
        .map_or(Category::Other, |&(_, category)| category)
}

/// One color per `ext_id`, so rendering code indexes instead of classifying strings.
#[must_use]
pub fn color_table(extensions: &ExtensionTable) -> Vec<Color32> {
    (0..extensions.len())
        .map(|id| category_of(extensions.name(id as u16)).color())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_representative_extensions() {
        assert_eq!(category_of("mkv"), Category::Video);
        assert_eq!(category_of("zst"), Category::Archive);
        assert_eq!(category_of("so"), Category::Binary);
        assert_eq!(category_of("iso"), Category::DiskImage);
        assert_eq!(category_of("unknown"), Category::Other);
        assert_eq!(category_of(""), Category::Other);
    }

    #[test]
    fn color_table_has_one_entry_per_extension() {
        let mut table = ExtensionTable::default();
        table.intern("a.png");
        let colors = color_table(&table);
        assert_eq!(colors.len(), table.len());
        assert_eq!(
            colors[usize::from(table.intern("b.png"))],
            Category::Image.color()
        );
    }
}
