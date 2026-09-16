// SPDX-License-Identifier: GPL-3.0-only
//! Pure menu model built from dbusmenu layouts.

use std::collections::HashMap;

use zvariant::{OwnedValue, Value};

use super::dbusmenu::LayoutItem;

/// Guard against malicious or buggy exporters sending cyclic-looking or
/// absurdly deep trees.
const MAX_DEPTH: usize = 16;
const MAX_CHILDREN: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    None,
    Check(bool),
    Radio(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Separator,
    Entry(Entry),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub toggle: Toggle,
    pub shortcut: Option<String>,
    /// `true` if this entry opens a submenu, even if its children are loaded lazily.
    pub submenu: bool,
    pub children: Vec<Item>,
}

/// A menu bar: the top-level entries (File, Edit, …).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Menu {
    pub titles: Vec<Entry>,
}

impl Menu {
    /// Find an entry anywhere in the tree.
    #[must_use]
    pub fn find(&self, id: i32) -> Option<&Entry> {
        fn walk(items: &[Item], id: i32) -> Option<&Entry> {
            items.iter().find_map(|item| match item {
                Item::Entry(e) if e.id == id => Some(e),
                Item::Entry(e) => walk(&e.children, id),
                Item::Separator => None,
            })
        }
        self.titles
            .iter()
            .find(|e| e.id == id)
            .or_else(|| self.titles.iter().find_map(|t| walk(&t.children, id)))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.titles.is_empty()
    }
}

fn prop_str(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| match &**v {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    })
}

fn prop_bool(props: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    props.get(key).and_then(|v| match &**v {
        Value::Bool(b) => Some(*b),
        _ => None,
    })
}

fn prop_i32(props: &HashMap<String, OwnedValue>, key: &str) -> Option<i32> {
    props.get(key).and_then(|v| match &**v {
        Value::I32(i) => Some(*i),
        _ => None,
    })
}

/// Remove mnemonic markers: `_File` → `File`, `Save__As` → `Save_As`.
#[must_use]
pub fn strip_mnemonic(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Render a dbusmenu `shortcut` property (`aas`, e.g. `[["Control","S"]]`).
#[must_use]
pub fn format_shortcut(value: &Value<'_>) -> Option<String> {
    let Value::Array(chords) = value else {
        return None;
    };
    let first = chords.iter().next()?;
    let Value::Array(keys) = first else {
        return None;
    };
    let keys: Vec<String> = keys
        .iter()
        .filter_map(|k| match k {
            Value::Str(s) => Some(match s.as_str() {
                "Control" => "Ctrl".to_owned(),
                "Super" => "Super".to_owned(),
                other => other.to_owned(),
            }),
            _ => None,
        })
        .collect();
    (!keys.is_empty()).then(|| keys.join("+"))
}

fn tidy_separators(items: Vec<Item>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    for item in items {
        if matches!(item, Item::Separator) && matches!(out.last(), None | Some(Item::Separator)) {
            continue;
        }
        out.push(item);
    }
    while matches!(out.last(), Some(Item::Separator)) {
        out.pop();
    }
    out
}

fn parse_item(layout: &LayoutItem, depth: usize) -> Option<Item> {
    let props = &layout.properties;
    if prop_bool(props, "visible") == Some(false) {
        return None;
    }
    if prop_str(props, "type").as_deref() == Some("separator") {
        return Some(Item::Separator);
    }
    let label = strip_mnemonic(&prop_str(props, "label").unwrap_or_default());

    let children = if depth < MAX_DEPTH {
        tidy_separators(
            layout
                .children
                .iter()
                .take(MAX_CHILDREN)
                .filter_map(|child| LayoutItem::try_from(child.try_clone().ok()?).ok())
                .filter_map(|child| parse_item(&child, depth + 1))
                .collect(),
        )
    } else {
        Vec::new()
    };
    let submenu =
        !children.is_empty() || prop_str(props, "children-display").as_deref() == Some("submenu");
    if label.trim().is_empty() && !submenu {
        return None;
    }

    let state = prop_i32(props, "toggle-state") == Some(1);
    let toggle = match prop_str(props, "toggle-type").as_deref() {
        Some("checkmark") => Toggle::Check(state),
        Some("radio") => Toggle::Radio(state),
        _ => Toggle::None,
    };

    Some(Item::Entry(Entry {
        id: layout.id,
        label,
        enabled: prop_bool(props, "enabled").unwrap_or(true),
        toggle,
        shortcut: props.get("shortcut").and_then(|v| format_shortcut(v)),
        submenu,
        children,
    }))
}

/// Build a menu bar from the root layout (id 0). Hidden, unlabeled and
/// separator entries at the top level are dropped.
#[must_use]
pub fn parse(root: &LayoutItem) -> Menu {
    let titles = root
        .children
        .iter()
        .take(MAX_CHILDREN)
        .filter_map(|child| LayoutItem::try_from(child.try_clone().ok()?).ok())
        .filter_map(|child| match parse_item(&child, 1) {
            Some(Item::Entry(entry)) if !entry.label.trim().is_empty() => Some(entry),
            _ => None,
        })
        .collect();
    Menu { titles }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn node(
        id: i32,
        props: &[(&str, Value<'static>)],
        children: Vec<LayoutItem>,
    ) -> LayoutItem {
        LayoutItem {
            id,
            properties: props
                .iter()
                .map(|(k, v)| {
                    (
                        (*k).to_owned(),
                        OwnedValue::try_from(v.try_clone().unwrap()).unwrap(),
                    )
                })
                .collect(),
            children: children
                .into_iter()
                .map(|c| OwnedValue::try_from(Value::from(c)).unwrap())
                .collect(),
        }
    }

    pub fn sample() -> LayoutItem {
        let shortcut = Value::from(vec![vec!["Control".to_owned(), "S".to_owned()]]);
        node(
            0,
            &[],
            vec![
                node(
                    1,
                    &[
                        ("label", "_File".into()),
                        ("children-display", "submenu".into()),
                    ],
                    vec![
                        node(10, &[("type", "separator".into())], vec![]),
                        node(
                            11,
                            &[("label", "_Save".into()), ("shortcut", shortcut)],
                            vec![],
                        ),
                        node(
                            12,
                            &[("label", "Hidden".into()), ("visible", false.into())],
                            vec![],
                        ),
                        node(13, &[("type", "separator".into())], vec![]),
                        node(14, &[("type", "separator".into())], vec![]),
                        node(
                            15,
                            &[
                                ("label", "Word _Wrap".into()),
                                ("toggle-type", "checkmark".into()),
                                ("toggle-state", 1i32.into()),
                            ],
                            vec![],
                        ),
                        node(
                            16,
                            &[("label", "Recent".into())],
                            vec![node(
                                160,
                                &[("label", "a__b.txt".into()), ("enabled", false.into())],
                                vec![],
                            )],
                        ),
                        node(17, &[("type", "separator".into())], vec![]),
                    ],
                ),
                node(2, &[("label", "".into())], vec![]),
                node(
                    3,
                    &[
                        ("label", "_Help".into()),
                        ("children-display", "submenu".into()),
                    ],
                    vec![],
                ),
            ],
        )
    }

    #[test]
    fn parses_titles_items_and_properties() {
        let menu = parse(&sample());
        let titles: Vec<_> = menu.titles.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(titles, ["File", "Help"], "empty labels are dropped");
        assert!(menu.titles[1].submenu, "lazy submenus are kept");

        let file = &menu.titles[0].children;
        assert_eq!(
            file.len(),
            4,
            "hidden entries and redundant separators removed: {file:?}"
        );
        let Item::Entry(save) = &file[0] else {
            panic!("leading separator not trimmed")
        };
        assert_eq!(
            (save.label.as_str(), save.shortcut.as_deref()),
            ("Save", Some("Ctrl+S"))
        );
        assert_eq!(file[1], Item::Separator);
        let Item::Entry(wrap) = &file[2] else {
            panic!()
        };
        assert_eq!(wrap.toggle, Toggle::Check(true));

        let recent = menu.find(160).unwrap();
        assert_eq!(recent.label, "a_b.txt");
        assert!(!recent.enabled);
        assert!(menu.find(16).unwrap().submenu);
        assert!(
            !matches!(file.last(), Some(Item::Separator)),
            "trailing separator trimmed"
        );
    }

    #[test]
    fn mnemonics_are_stripped() {
        assert_eq!(strip_mnemonic("_Open…"), "Open…");
        assert_eq!(strip_mnemonic("Save__As"), "Save_As");
        assert_eq!(strip_mnemonic("plain"), "plain");
    }

    #[test]
    fn deep_trees_are_truncated() {
        let mut leaf = node(100, &[("label", "leaf".into())], vec![]);
        for i in 0..40 {
            leaf = node(i, &[("label", "x".into())], vec![leaf]);
        }
        let root = node(0, &[], vec![leaf]);
        let menu = parse(&root);
        assert!(menu.find(100).is_none());
        assert_eq!(menu.titles.len(), 1);
    }
}
