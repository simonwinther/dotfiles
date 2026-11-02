//! Read static desktop menus without starting the controller interpreter.
use crate::shell::Entry;
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use std::{collections::HashMap, fmt, path::PathBuf};

#[derive(Default)]
struct Items(Vec<(String, Vec<String>)>);
impl<'de> Deserialize<'de> for Items {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Ordered;
        impl<'de> Visitor<'de> for Ordered {
            type Value = Items;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("menu labels and command argument lists")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Items, M::Error> {
                let mut items = vec![];
                while let Some(item) = map.next_entry()? {
                    items.push(item);
                }
                Ok(Items(items))
            }
        }
        deserializer.deserialize_map(Ordered)
    }
}
#[derive(Deserialize)]
struct Menu {
    #[serde(default)]
    items: Items,
}
pub fn load(group: &str) -> Result<Vec<Entry>, String> {
    let path = std::env::var_os("DESKTOP_MENUS_FILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .map(|p| p.join("desktop/menus.json"))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".config/desktop/menus.json"))
        })
        .ok_or("Menu configuration path unavailable")?;
    let bytes = crate::assets::read(path).ok_or("Menu configuration unavailable")?;
    let mut entries = parse(&bytes, group)?;
    if group == "main" {
        entries.extend(
            crate::search::launcher_entries()
                .into_iter()
                .map(|mut entry| {
                    entry.category = "launcher".into();
                    entry
                }),
        );
    }
    Ok(entries)
}
fn parse(bytes: &[u8], group: &str) -> Result<Vec<Entry>, String> {
    let mut menus: HashMap<String, Menu> =
        serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    Ok(menus
        .remove(group)
        .map(|menu| menu.items.0)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, command)| Entry {
            id: name.clone(),
            name,
            command,
            ..Default::default()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_order_and_literal_arguments_match_the_configuration() {
        let bytes = br#"{"main":{"title":"Go","items":{"Zebra":["menu","style"],"Alpha":["exec","literal $() with spaces"]}}}"#;
        let entries = parse(bytes, "main").unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["Zebra", "Alpha"]
        );
        assert_eq!(entries[1].command, ["exec", "literal $() with spaces"]);
        assert!(parse(bytes, "missing").unwrap().is_empty());
        assert!(parse(br#"{"main":{"items":{"Bad":"exec true"}}}"#, "main").is_err());
    }
}
