//! `knogg config` — read and write knogg.yml in the current directory.

use std::fs;

use anyhow::{anyhow, Result};
use serde_yaml::{Mapping, Value};

use crate::core::config::{CONFIG_FILE};

pub fn cmd_show() -> Result<()> {
    match fs::read_to_string(CONFIG_FILE) {
        Ok(raw) => print!("{raw}"),
        Err(_) => match fs::read_to_string("knogg.toml") {
            Ok(raw) => {
                eprintln!("note: knogg.yml not found, showing knogg.toml (legacy)");
                print!("{raw}");
            }
            Err(_) => println!("# no knogg.yml found — run: knogg config set <key> <value>"),
        },
    }
    Ok(())
}

pub fn cmd_set(key: &str, value: &str) -> Result<()> {
    let raw = fs::read_to_string(CONFIG_FILE).unwrap_or_default();
    let mut doc: Value = if raw.trim().is_empty() {
        Value::Mapping(Mapping::new())
    } else {
        serde_yaml::from_str(&raw).map_err(|e| anyhow!("parsing knogg.yml: {e}"))?
    };

    let parts: Vec<&str> = key.split('.').collect();
    set_nested(&mut doc, &parts, parse_scalar(value));

    fs::write(CONFIG_FILE, serde_yaml::to_string(&doc)?)?;
    println!("{key} = {value}");
    Ok(())
}

pub fn cmd_get(key: &str) -> Result<()> {
    let raw = fs::read_to_string(CONFIG_FILE)
        .or_else(|_| fs::read_to_string("knogg.toml"))
        .unwrap_or_default();
    let doc: Value = if raw.trim().is_empty() {
        Value::Null
    } else {
        serde_yaml::from_str(&raw).unwrap_or(Value::Null)
    };
    let parts: Vec<&str> = key.split('.').collect();
    match get_nested(&doc, &parts) {
        Some(v) => println!("{}", scalar_str(v)),
        None => println!("(not set)"),
    }
    Ok(())
}

fn parse_scalar(s: &str) -> Value {
    if s == "true" {
        return Value::Bool(true);
    }
    if s == "false" {
        return Value::Bool(false);
    }
    if let Ok(n) = s.parse::<i64>() {
        return Value::Number(n.into());
    }
    Value::String(s.to_string())
}

fn scalar_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

fn set_nested(node: &mut Value, keys: &[&str], val: Value) {
    if keys.is_empty() {
        *node = val;
        return;
    }
    if !matches!(node, Value::Mapping(_)) {
        *node = Value::Mapping(Mapping::new());
    }
    if let Value::Mapping(map) = node {
        let k = Value::String(keys[0].to_string());
        if keys.len() == 1 {
            map.insert(k, val);
        } else {
            if !map.contains_key(&k) {
                map.insert(k.clone(), Value::Mapping(Mapping::new()));
            }
            if let Some(child) = map.get_mut(&k) {
                set_nested(child, &keys[1..], val);
            }
        }
    }
}

fn get_nested<'a>(node: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    if keys.is_empty() {
        return Some(node);
    }
    if let Value::Mapping(m) = node {
        let k = Value::String(keys[0].to_string());
        m.get(&k).and_then(|v| get_nested(v, &keys[1..]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_nested_creates_path() {
        let mut doc = Value::Mapping(Mapping::new());
        set_nested(&mut doc, &["mesh", "listen_port"], Value::Number(5051.into()));
        set_nested(&mut doc, &["mesh", "peers", "backend"], Value::String("tcp://localhost:5052".into()));
        set_nested(&mut doc, &["proposals", "autoapply_low"], Value::Bool(true));

        assert_eq!(get_nested(&doc, &["mesh", "listen_port"]), Some(&Value::Number(5051.into())));
        assert_eq!(
            get_nested(&doc, &["mesh", "peers", "backend"]),
            Some(&Value::String("tcp://localhost:5052".into()))
        );
        assert_eq!(get_nested(&doc, &["proposals", "autoapply_low"]), Some(&Value::Bool(true)));
    }

    #[test]
    fn get_nested_missing_returns_none() {
        let doc = Value::Mapping(Mapping::new());
        assert_eq!(get_nested(&doc, &["missing"]), None);
        assert_eq!(get_nested(&doc, &["a", "b", "c"]), None);
    }

    #[test]
    fn parse_scalar_types() {
        assert_eq!(parse_scalar("true"), Value::Bool(true));
        assert_eq!(parse_scalar("false"), Value::Bool(false));
        assert_eq!(parse_scalar("5051"), Value::Number(5051.into()));
        assert_eq!(parse_scalar("hello"), Value::String("hello".into()));
    }
}
