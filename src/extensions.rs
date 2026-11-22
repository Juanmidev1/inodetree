//! Interning of file extensions into compact `u16` ids.

use std::collections::HashMap;

/// Id for files and directories without an extension.
pub const NO_EXTENSION: u16 = 0;
/// Id shared by every extension seen after the `u16` id space is exhausted.
pub const OTHER_EXTENSION: u16 = 1;

#[derive(Debug)]
pub struct ExtensionTable {
    names: Vec<String>,
    ids: HashMap<String, u16>,
}

impl Default for ExtensionTable {
    fn default() -> Self {
        Self {
            names: vec![String::new(), String::from("other")],
            ids: HashMap::new(),
        }
    }
}

impl ExtensionTable {
    /// Returns the id for the extension of `file_name`, registering it on first sight.
    pub fn intern(&mut self, file_name: &str) -> u16 {
        let Some(extension) = extension_of(file_name) else {
            return NO_EXTENSION;
        };
        if extension.bytes().any(|byte| byte.is_ascii_uppercase()) {
            self.intern_normalized(&extension.to_ascii_lowercase())
        } else {
            self.intern_normalized(extension)
        }
    }

    fn intern_normalized(&mut self, extension: &str) -> u16 {
        if let Some(&id) = self.ids.get(extension) {
            return id;
        }
        let Ok(id) = u16::try_from(self.names.len()) else {
            return OTHER_EXTENSION;
        };
        self.names.push(extension.to_owned());
        self.ids.insert(extension.to_owned(), id);
        id
    }

    #[must_use]
    pub fn name(&self, id: u16) -> &str {
        self.names.get(usize::from(id)).map_or("", String::as_str)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// Dotfiles such as `.bashrc` have no extension: the dot must follow a non-empty stem.
fn extension_of(file_name: &str) -> Option<&str> {
    let (stem, extension) = file_name.rsplit_once('.')?;
    (!stem.is_empty() && !extension.is_empty()).then_some(extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_extensions_case_insensitively() {
        let mut table = ExtensionTable::default();
        let lower = table.intern("movie.mkv");
        assert_eq!(table.intern("OTHER.MKV"), lower);
        assert_eq!(table.name(lower), "mkv");
    }

    #[test]
    fn files_without_extension_use_the_reserved_id() {
        let mut table = ExtensionTable::default();
        assert_eq!(table.intern("Makefile"), NO_EXTENSION);
        assert_eq!(table.intern(".bashrc"), NO_EXTENSION);
        assert_eq!(table.intern("trailing."), NO_EXTENSION);
    }

    #[test]
    fn overflow_falls_back_to_the_other_id() {
        let mut table = ExtensionTable::default();
        for index in 0..=u32::from(u16::MAX) {
            table.intern(&format!("file.e{index}"));
        }
        assert_eq!(table.intern("file.brandnew"), OTHER_EXTENSION);
    }
}
