//! Generic editor for a `serde_json::Value` tree: objects and arrays as collapsible sections,
//! numbers as drag values, strings as text fields, booleans as checkboxes.

use eframe::egui;
use serde_json::{Map, Number, Value};

/// Draw an editor for `v`; returns true if anything changed.
pub fn edit(ui: &mut egui::Ui, id: egui::Id, v: &mut Value) -> bool {
    match v {
        Value::Object(map) => edit_map(ui, id, map),
        other => edit_scalar_or_array(ui, id, "", other, 0),
    }
}

fn edit_map(ui: &mut egui::Ui, id: egui::Id, map: &mut Map<String, Value>) -> bool {
    let mut changed = false;
    for (k, v) in map.iter_mut() {
        changed |= edit_entry(ui, id.with(k.as_str()), k, v, 0);
    }
    changed
}

fn is_scalar(v: &Value) -> bool {
    !matches!(v, Value::Array(_) | Value::Object(_))
}

/// A short one-line summary for collapsed containers.
fn summary(v: &Value) -> String {
    match v {
        Value::Array(a) if a.iter().all(is_scalar) && a.len() <= 6 => serde_json::to_string(a).unwrap_or_default(),
        Value::Array(a) => format!("[{} items]", a.len()),
        Value::Object(m) => match m.get("type").and_then(Value::as_str) {
            Some(t) => format!("{t} {{…}}"),
            None => format!("{{{} fields}}", m.len()),
        },
        v => v.to_string(),
    }
}

fn edit_entry(ui: &mut egui::Ui, id: egui::Id, key: &str, v: &mut Value, depth: usize) -> bool {
    match v {
        Value::Object(map) => {
            let mut changed = false;
            egui::CollapsingHeader::new(format!("{key}  {}", egui::RichText::new(summary(&Value::Object(map.clone()))).weak().text()))
                .id_salt(id)
                .default_open(depth == 0 || (depth == 1 && map.len() <= 8))
                .show(ui, |ui| {
                    for (k, item) in map.iter_mut() {
                        changed |= edit_entry(ui, id.with(k.as_str()), k, item, depth + 1);
                    }
                });
            changed
        }
        Value::Array(items) if items.iter().all(is_scalar) && items.len() <= 8 => {
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                ui.label(key);
                for (i, item) in items.iter_mut().enumerate() {
                    changed |= scalar(ui, id.with(i), item);
                }
            });
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            let title = format!("{key}  [{}]", items.len());
            egui::CollapsingHeader::new(title).id_salt(id).default_open(false).show(ui, |ui| {
                let mut remove = None;
                for (i, item) in items.iter_mut().enumerate() {
                    ui.horizontal_top(|ui| {
                        if ui.small_button("✕").on_hover_text("remove item").clicked() {
                            remove = Some(i);
                        }
                        ui.vertical(|ui| {
                            changed |= edit_entry(ui, id.with(i), &format!("{i}"), item, depth + 1);
                        });
                    });
                }
                if let Some(i) = remove {
                    items.remove(i);
                    changed = true;
                }
                if let Some(last) = items.last().cloned()
                    && ui.small_button("+ duplicate last").clicked()
                {
                    items.push(last);
                    changed = true;
                }
            });
            changed
        }
        other => edit_scalar_or_array(ui, id, key, other, depth),
    }
}

fn edit_scalar_or_array(ui: &mut egui::Ui, id: egui::Id, key: &str, v: &mut Value, depth: usize) -> bool {
    if !is_scalar(v) {
        return edit_entry(ui, id, key, v, depth);
    }
    ui.horizontal(|ui| {
        if !key.is_empty() {
            ui.label(key);
        }
        scalar(ui, id, v)
    })
    .inner
}

fn scalar(ui: &mut egui::Ui, id: egui::Id, v: &mut Value) -> bool {
    match v {
        Value::Bool(b) => ui.checkbox(b, "").changed(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                let mut x = i;
                let r = ui.push_id(id, |ui| ui.add(egui::DragValue::new(&mut x))).inner;
                if r.changed() {
                    *n = Number::from(x);
                    return true;
                }
            } else if let Some(u) = n.as_u64() {
                let mut x = u;
                let r = ui.push_id(id, |ui| ui.add(egui::DragValue::new(&mut x))).inner;
                if r.changed() {
                    *n = Number::from(x);
                    return true;
                }
            } else if let Some(f) = n.as_f64() {
                let mut x = f;
                let r = ui.push_id(id, |ui| ui.add(egui::DragValue::new(&mut x).speed(0.01).max_decimals(8))).inner;
                if r.changed()
                    && let Some(nn) = Number::from_f64(x)
                {
                    *n = nn;
                    return true;
                }
            }
            false
        }
        Value::String(s) => ui.push_id(id, |ui| ui.add(egui::TextEdit::singleline(s).desired_width(180.0))).inner.changed(),
        Value::Null => {
            ui.weak("null");
            false
        }
        _ => false,
    }
}
