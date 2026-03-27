//! Output formatting — table, JSON, YAML, wide.

use serde::Serialize;
use tabled::{Table, Tabled};

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum OutputFormat {
    Table,
    Wide,
    Json,
}

/// Print a list of items in the requested format.
pub fn print_list<T: Serialize + Tabled>(items: &[T], format: &OutputFormat, no_header: bool) {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(items).unwrap_or_default());
        }
        OutputFormat::Table | OutputFormat::Wide => {
            if items.is_empty() {
                println!("No items found.");
                return;
            }
            let mut table = Table::new(items);
            if no_header {
                table.with(tabled::settings::Style::empty());
            } else {
                table.with(tabled::settings::Style::sharp());
            }
            println!("{}", table);
        }
    }
}

/// Print a single item in the requested format.
pub fn print_item<T: Serialize>(item: &T, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(item).unwrap_or_default());
        }
        OutputFormat::Table | OutputFormat::Wide => {
            // For single items, pretty-print as key: value
            if let Ok(val) = serde_json::to_value(item) {
                if let Some(obj) = val.as_object() {
                    let max_key = obj.keys().map(|k| k.len()).max().unwrap_or(0);
                    for (key, val) in obj {
                        let val_str = match val {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Null => "-".into(),
                            other => other.to_string(),
                        };
                        println!("{:<width$}  {}", format!("{}:", key), val_str, width = max_key + 1);
                    }
                } else {
                    println!("{}", serde_json::to_string_pretty(item).unwrap_or_default());
                }
            }
        }
    }
}

/// Print a success message (not in JSON mode).
pub fn print_ok(msg: &str, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::json!({"ok": true, "message": msg}));
        }
        _ => println!("{}", msg),
    }
}

/// Print a simple status line.
pub fn println_status(label: &str, value: &str) {
    println!("{:<20} {}", format!("{}:", label), value);
}
