//! The tag gallery: the desktop notebook's built-in tags and the user's custom ones, in the order
//! Customize Tags gives them. The first nine take Ctrl+1 … Ctrl+9.

use std::borrow::Cow;

use crate::db::{DbResult, Library};
use crate::icons;
use crate::model::{CustomTag, LineTag, Rgb, TagMark};

/// A tag in the gallery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagKey {
    Todo,
    Line(LineTag),
    /// An index into `TagGallery::custom`.
    Custom(u16),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TagGallery {
    pub order: Vec<TagKey>,
    /// Custom tags as Customize Tags defines them (never `checked`).
    pub custom: Vec<CustomTag>,
    /// Bumped on every change, so cached menu widths follow.
    pub revision: u64,
}

impl Default for TagGallery {
    fn default() -> Self {
        let mut order = vec![TagKey::Todo];
        order.extend(LineTag::ALL.map(TagKey::Line));
        Self {
            order,
            custom: Vec::new(),
            revision: 0,
        }
    }
}

/// Reads a JSON setting, or `None` when it is missing or cannot be read (said on stderr).
fn json_setting<T: serde::de::DeserializeOwned>(library: &Library, key: &str) -> Option<T> {
    let json = match library.setting(key) {
        Ok(json) => json?,
        Err(err) => {
            eprintln!("folio: could not read the setting {key}: {err}");
            return None;
        }
    };
    serde_json::from_str(&json)
        .map_err(|err| eprintln!("folio: the setting {key} is not valid: {err}"))
        .ok()
}

impl TagGallery {
    /// The gallery as the settings `custom_tags` and `tag_order` keep it. Keys that no
    /// longer resolve are skipped, and tags missing from the order (a newer Folio's
    /// built-ins, say) are added after it: built-ins in the desktop notebook's order, then custom tags.
    pub fn load(library: &Library) -> Self {
        let mut custom: Vec<CustomTag> = json_setting(library, "custom_tags").unwrap_or_default();
        custom.retain(|tag| !tag.name.trim().is_empty());
        for tag in &mut custom {
            tag.checked = false;
        }
        let names: Vec<String> = json_setting(library, "tag_order").unwrap_or_default();
        let resolve = |name: &str| -> Option<TagKey> {
            if name == "todo" {
                return Some(TagKey::Todo);
            }
            if let Some(name) = name.strip_prefix("custom:") {
                let index = custom.iter().position(|tag| tag.name == name)?;
                return u16::try_from(index).ok().map(TagKey::Custom);
            }
            serde_json::from_value(serde_json::Value::String(name.to_string()))
                .ok()
                .map(TagKey::Line)
        };
        let mut order: Vec<TagKey> = Vec::new();
        let builtins = Self::default().order;
        let customs = (0..custom.len()).filter_map(|index| u16::try_from(index).ok());
        let keys = names
            .iter()
            .filter_map(|name| resolve(name))
            .chain(builtins)
            .chain(customs.map(TagKey::Custom));
        for key in keys {
            if !order.contains(&key) {
                order.push(key);
            }
        }
        Self {
            order,
            custom,
            revision: 0,
        }
    }

    /// Stores the custom tags and the order in the settings `custom_tags` and `tag_order`.
    pub fn save(&self, library: &mut Library) -> DbResult<()> {
        let order: Vec<String> = self
            .order
            .iter()
            .filter_map(|key| self.stored_name(*key))
            .collect();
        library.set_setting("custom_tags", &serde_json::to_string(&self.custom)?)?;
        library.set_setting("tag_order", &serde_json::to_string(&order)?)
    }

    /// How `tag_order` names a key: "todo", a built-in's stored name, or "custom:<name>".
    fn stored_name(&self, key: TagKey) -> Option<String> {
        match key {
            TagKey::Todo => Some("todo".to_string()),
            TagKey::Line(tag) => serde_json::to_value(tag)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string)),
            TagKey::Custom(index) => self.custom(index).map(|tag| format!("custom:{}", tag.name)),
        }
    }

    /// The digit of the key's Ctrl shortcut: its place in the gallery, for the first nine.
    pub fn digit(&self, key: TagKey) -> Option<u8> {
        let position = self.order.iter().position(|k| *k == key)?;
        (position < 9).then(|| position as u8 + 1)
    }

    /// The tag Ctrl+`digit` applies (`digit` is 1 to 9).
    pub fn at_digit(&self, digit: u8) -> Option<TagKey> {
        if !(1..=9).contains(&digit) {
            return None;
        }
        self.order.get(usize::from(digit) - 1).copied()
    }

    pub fn label(&self, key: TagKey) -> Cow<'static, str> {
        match key {
            TagKey::Todo => TagMark::Todo.label(),
            TagKey::Line(tag) => tag.label(),
            TagKey::Custom(index) => Cow::Owned(
                self.custom(index)
                    .map(|tag| tag.name.clone())
                    .unwrap_or_default(),
            ),
        }
    }

    pub fn icon(&self, key: TagKey) -> &'static str {
        match key {
            TagKey::Todo => TagMark::Todo.icon(),
            TagKey::Line(tag) => tag.icon(),
            TagKey::Custom(index) => self
                .custom(index)
                .and_then(|tag| tag.symbol.icon())
                .unwrap_or(icons::TAG),
        }
    }

    /// `None` means the theme accent.
    pub fn color(&self, key: TagKey) -> Option<Rgb> {
        match key {
            TagKey::Todo => None,
            TagKey::Line(tag) => Some(tag.color()),
            TagKey::Custom(index) => self.custom(index).and_then(|tag| tag.symbol.color()),
        }
    }

    pub fn custom(&self, index: u16) -> Option<&CustomTag> {
        self.custom.get(usize::from(index))
    }

    pub fn bump(&mut self) {
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TagSymbol;

    #[test]
    fn the_default_order_is_notebooks() {
        let gallery = TagGallery::default();
        assert_eq!(gallery.order.len(), 29);
        assert_eq!(gallery.order[0], TagKey::Todo);
        assert_eq!(gallery.digit(TagKey::Todo), Some(1));
        assert_eq!(gallery.digit(TagKey::Line(LineTag::Important)), Some(2));
        assert_eq!(gallery.digit(TagKey::Line(LineTag::Phone)), Some(9));
        assert_eq!(gallery.digit(TagKey::Line(LineTag::WebSite)), None);
        assert_eq!(gallery.at_digit(9), Some(TagKey::Line(LineTag::Phone)));
        assert_eq!(gallery.at_digit(0), None);
    }

    #[test]
    fn order_and_custom_tags_survive_a_reload() {
        let mut lib = Library::open_in_memory().unwrap();
        let mut gallery = TagGallery::load(&lib);
        assert_eq!(gallery, TagGallery::default());
        gallery.custom.push(CustomTag {
            name: "Homework".into(),
            symbol: TagSymbol::Checkbox,
            font_color: None,
            highlight: None,
            checked: false,
        });
        gallery.order.insert(0, TagKey::Custom(0));
        gallery.save(&mut lib).unwrap();

        let loaded = TagGallery::load(&lib);
        assert_eq!(loaded.order, gallery.order);
        assert_eq!(loaded.custom, gallery.custom);
        assert_eq!(loaded.digit(TagKey::Custom(0)), Some(1));
        assert_eq!(loaded.digit(TagKey::Todo), Some(2));
        assert_eq!(loaded.label(TagKey::Custom(0)), "Homework");
        assert_eq!(loaded.icon(TagKey::Custom(0)), icons::CHECK_SQUARE);
    }

    #[test]
    fn unknown_and_missing_keys_are_mended() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.set_setting(
            "tag_order",
            r#"["question","nonsense","custom:gone","todo"]"#,
        )
        .unwrap();
        let gallery = TagGallery::load(&lib);
        assert_eq!(gallery.order.len(), 29);
        assert_eq!(gallery.order[0], TagKey::Line(LineTag::Question));
        assert_eq!(gallery.order[1], TagKey::Todo);
        let rest: Vec<TagKey> = LineTag::ALL
            .into_iter()
            .filter(|tag| *tag != LineTag::Question)
            .map(TagKey::Line)
            .collect();
        assert_eq!(gallery.order[2..], rest[..]);

        // A setting that is not JSON gives the default gallery, never a panic.
        lib.set_setting("custom_tags", "{oops").unwrap();
        lib.set_setting("tag_order", "[1, 2").unwrap();
        assert_eq!(TagGallery::load(&lib), TagGallery::default());
    }
}
