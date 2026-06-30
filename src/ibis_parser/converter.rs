// =============================================================================
// IBIS → TOML converter — order-preserving manual builder
// =============================================================================

use std::fmt::Write as FmtWrite;

use pest::Parser;

use crate::ibis_parser::parser::IbisParser;
use crate::ibis_parser::parser::Rule;

fn split_kw(line: &str) -> Option<(String, Option<String>)> {
    let t = line.trim();
    let close = t.find(']')?;
    let kw_part = &t[..=close];
    let rest = t[close+1..].trim();
    let rest = if rest.is_empty() { None } else { Some(rest.to_string()) };
    IbisParser::parse(Rule::keyword_header, kw_part).ok().and_then(|pairs| {
        pairs.into_iter().next().and_then(|p| {
            p.into_inner().next().map(|i| (i.as_str().trim().to_string(), rest.clone()))
        })
    })
}

fn clean(line: &str) -> Option<String> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('|') { return None; }
    let mut b = false;
    let mut o = String::new();
    for ch in t.chars() {
        match ch { '[' => { b = true; o.push(ch); } ']' => { b = false; o.push(ch); } '|' if !b => break, _ => o.push(ch), }
    }
    let r = o.trim().to_string();
    if r.is_empty() { None } else { Some(r) }
}

fn esc(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn sec_name(kw: &str) -> String { kw.replace(' ', "_") }

// ---------------------------------------------------------------------------
// Ordered tree
// ---------------------------------------------------------------------------

enum SecType { Once, Array }

struct Section {
    name: String,
    sec_type: SecType,
    content: Vec<String>,
    children: Vec<Section>,
}

fn build_tree(blocks: Vec<(String, Vec<String>)>) -> Vec<Section> {
    let mut roots = Vec::new();
    let mut i = 0;
    while i < blocks.len() {
        let (kw, lines) = &blocks[i];
        match kw.as_str() {
            "Component" | "Model" => {
                let is_model = kw == "Model";
                let mut sec = Section {
                    name: if is_model { "Model".into() } else { "Component".into() },
                    sec_type: SecType::Array,
                    content: lines.clone(),
                    children: Vec::new(),
                };
                i += 1;
                while i < blocks.len() {
                    let (sk, sl) = &blocks[i];
                    if matches!(sk.as_str(), "Component" | "Model" | "End") { break; }
                    sec.children.push(Section {
                        name: sec_name(sk),
                        sec_type: SecType::Once,
                        content: sl.clone(),
                        children: Vec::new(),
                    });
                    i += 1;
                }
                roots.push(sec);
            }
            "End" => { i += 1; }
            _ => {
                roots.push(Section {
                    name: sec_name(kw),
                    sec_type: SecType::Once,
                    content: lines.clone(),
                    children: Vec::new(),
                });
                i += 1;
            }
        }
    }
    roots
}

// ---------------------------------------------------------------------------
// Manual TOML serializer
// ---------------------------------------------------------------------------

fn key_name(name: &str) -> String {
    name.to_lowercase()
}

fn write_sec(sec: &Section, path: &str, out: &mut String) {
    let full = if path.is_empty() { sec.name.clone() }
               else { format!("{}.{}", path, sec.name) };
    let key = key_name(&sec.name);

    match sec.sec_type {
        SecType::Once => { let _ = writeln!(out, "[{}]", full); }
        SecType::Array => { let _ = writeln!(out, "[[{}]]", full); }
    }

    if sec.content.len() == 1 {
        let _ = writeln!(out, "{} = {}", key, esc(&sec.content[0]));
    } else if sec.content.len() > 1 {
        let _ = writeln!(out, "{} = [", key);
        for (i, line) in sec.content.iter().enumerate() {
            let c = if i < sec.content.len() - 1 { "," } else { "" };
            let _ = writeln!(out, "    {}{}", esc(line), c);
        }
        let _ = writeln!(out, "]");
    }
    let _ = writeln!(out);

    for child in &sec.children {
        write_sec(child, &full, out);
    }
}

pub fn ibs2toml(content: &str) -> Result<String, String> {
    // Split into blocks
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    let mut kw: Option<String> = None;
    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        let Some(cl) = clean(line) else { continue };
        if let Some((name, rest)) = split_kw(&cl) {
            if let Some(old) = kw.take() { blocks.push((old, lines.clone())); lines.clear(); }
            kw = Some(name);
            if let Some(r) = rest { lines.push(r); }
        } else {
            lines.push(cl);
        }
    }
    if let Some(k) = kw.take() { blocks.push((k, lines)); }

    let tree = build_tree(blocks);
    let mut out = String::new();
    for sec in &tree {
        write_sec(sec, "", &mut out);
    }
    Ok(out)
}
