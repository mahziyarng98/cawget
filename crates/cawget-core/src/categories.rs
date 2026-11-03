//! Download categories by file extension. The table lives in `shared/categories.json` at the workspace root and
//! is shared with the desktop UI, so both sides always agree.

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
pub struct Category {
    pub id: String,
    pub label: String,
    pub ext: Vec<String>,
}

#[derive(Deserialize)]
struct Table {
    categories: Vec<Category>,
}

const TABLE_JSON: &str = include_str!("../../../shared/categories.json");

pub fn all() -> &'static [Category] {
    static T: OnceLock<Vec<Category>> = OnceLock::new();
    T.get_or_init(|| {
        serde_json::from_str::<Table>(TABLE_JSON)
            .expect("shared/categories.json is valid")
            .categories
    })
}

/// Category id of a file name (by extension, case-insensitive), or `None` ("other").
pub fn category_of(name: &str) -> Option<&'static str> {
    let ext = name
        .rsplit_once('.')
        .map(|(stem, e)| (stem, e.to_ascii_lowercase()))?;
    if ext.0.is_empty() {
        return None; // ".hidden" has no extension
    }
    all()
        .iter()
        .find(|c| c.ext.contains(&ext.1))
        .map(|c| c.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_loads_and_classifies() {
        assert_eq!(all().len(), 5);
        assert_eq!(category_of("linux.tar.XZ"), Some("compressed"));
        assert_eq!(category_of("Report.pdf"), Some("documents"));
        assert_eq!(category_of("song.flac"), Some("music"));
        assert_eq!(category_of("tool.AppImage"), Some("programs"));
        assert_eq!(category_of("clip.mkv"), Some("video"));
        assert_eq!(category_of("data.bin"), None);
        assert_eq!(category_of("noext"), None);
        assert_eq!(category_of(".hidden"), None);
    }
}
