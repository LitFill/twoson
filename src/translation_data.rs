use serde::Deserialize;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub enum JsonValue {
    String(String),
    Object(HashMap<String, JsonValue>),
}

pub type JsonData = HashMap<String, JsonValue>;

#[derive(Clone, Debug)]
pub struct TranslationItem {
    pub key: String,
    pub source_text: String,
    pub target_text: Option<String>,
}

impl TranslationItem {
    pub fn is_translated(&self) -> bool {
        self.target_text.is_some()
    }

    pub fn get_display_text(&self) -> String {
        match &self.target_text {
            Some(text) => text.clone(),
            None => format!("[UNTRANSLATED] {}", self.source_text),
        }
    }
}

pub struct TranslationStore {
    pub all_items: HashMap<String, TranslationItem>,
}

impl TranslationStore {
    pub fn new(items: Vec<TranslationItem>) -> Self {
        let all_items = items
            .into_iter()
            .map(|item| (item.key.clone(), item))
            .collect();
        TranslationStore { all_items }
    }

    pub fn load_from_files(
        source_path: &PathBuf,
        output_path: Option<&PathBuf>,
    ) -> Result<Vec<TranslationItem>, Box<dyn Error>> {
        // Load source file
        let source_file = File::open(source_path)?;
        let reader = BufReader::new(source_file);
        let source_data: JsonData = serde_json::from_reader(reader)?;

        // Load target file if provided
        let mut target_data: JsonData = HashMap::new();
        if let Some(path) = output_path {
            if path.exists() {
                let target_file = File::open(path)?;
                let target_reader = BufReader::new(target_file);
                target_data = serde_json::from_reader(target_reader)?;
            }
        }

        let flat_source_data = Self::flatten_json(&source_data);
        let flat_target_data = Self::flatten_json(&target_data);

        // Create TranslationItems
        let mut items: Vec<TranslationItem> = Vec::new();
        for (key, source_text) in flat_source_data {
            let target_text = flat_target_data.get(&key).cloned();
            items.push(TranslationItem {
                key,
                source_text,
                target_text,
            });
        }

        // Sort items by key for consistent display
        items.sort_by(|a, b| a.key.cmp(&b.key));

        Ok(items)
    }

    // Helper function to flatten the nested JsonData
    fn flatten_json(data: &JsonData) -> HashMap<String, String> {
        let mut flat_map = HashMap::new();
        for (key, value) in data {
            Self::flatten_recursive(key, value, &mut flat_map);
        }
        flat_map
    }

    fn flatten_recursive(prefix: &str, value: &JsonValue, flat_map: &mut HashMap<String, String>) {
        match value {
            JsonValue::String(s) => {
                flat_map.insert(prefix.to_string(), s.clone());
            }
            JsonValue::Object(obj) => {
                for (key, inner_value) in obj {
                    let new_prefix = format!("{}.{}", prefix, key);
                    Self::flatten_recursive(&new_prefix, inner_value, flat_map);
                }
            }
        }
    }

    pub fn save_translations(&self, output_path: &PathBuf) -> Result<(), Box<dyn Error>> {
        let json_data = self.unflatten_to_json_value();
        let file = File::create(output_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &json_data)?;
        Ok(())
    }

    fn unflatten_to_json_value(&self) -> serde_json::Value {
        let mut root = serde_json::Value::Object(serde_json::Map::new());

        let mut sorted_keys: Vec<_> = self.all_items.keys().cloned().collect();
        sorted_keys.sort();

        for key in sorted_keys {
            if let Some(item) = self.all_items.get(&key) {
                if let Some(text) = &item.target_text {
                    let mut current = &mut root;
                    let segments: Vec<&str> = key.split('.').collect();
                    for (i, segment) in segments.iter().enumerate() {
                        if i == segments.len() - 1 {
                            if let Some(obj) = current.as_object_mut() {
                                obj.insert(
                                    segment.to_string(),
                                    serde_json::Value::String(text.clone()),
                                );
                            }
                        } else {
                            current = current
                                .as_object_mut()
                                .unwrap()
                                .entry(segment.to_string())
                                .or_insert_with(|| {
                                    serde_json::Value::Object(serde_json::Map::new())
                                });
                        }
                    }
                }
            }
        }
        root
    }
}
