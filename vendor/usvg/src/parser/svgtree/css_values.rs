// LumaPaint static CSS adapter. Bounded substitution and scalar/unit arithmetic.
// Copyright 2026 LumaPaint contributors. SPDX-License-Identifier: Apache-2.0 OR MIT
use std::collections::{HashMap, HashSet};
pub type Properties = HashMap<String, String>;
const LIMIT: usize = 65536;

fn function_end(s: &str, start: usize) -> Option<usize> {
    let mut depth = 1;
    let mut quote = None;
    let mut escaped = false;
    for (offset, c) in s[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            continue;
        }
        if c == '(' {
            depth += 1;
        }
        if c == ')' {
            depth -= 1;
            if depth == 0 {
                return Some(start + offset);
            }
        }
    }
    None
}
fn split_fallback(s: &str) -> (&str, Option<&str>) {
    let mut depth = 0;
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if q == c {
                quote = None;
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            continue;
        }
        if c == '(' {
            depth += 1;
        }
        if c == ')' {
            depth -= 1;
        }
        if c == ',' && depth == 0 {
            return (&s[..i], Some(&s[i + 1..]));
        }
    }
    (s, None)
}
fn next_function(s: &str, name: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            continue;
        }
        if s[i..].starts_with(name)
            && (i == 0
                || !s[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_'))
        {
            return Some(i);
        }
    }
    None
}
pub fn substitute(value: &str, properties: &Properties) -> Option<String> {
    fn expand(value: &str, p: &Properties, depth: u8) -> Option<String> {
        if depth > 32 || value.len() > LIMIT {
            return None;
        }
        let mut remaining = value;
        let mut out = String::new();
        while let Some(i) = next_function(remaining, "var(") {
            out.push_str(&remaining[..i]);
            let end = function_end(remaining, i + 4)?;
            let (name, fallback) = split_fallback(&remaining[i + 4..end]);
            let name = name.trim();
            if !name.starts_with("--") || name.len() == 2 {
                return None;
            }
            let replacement = p.get(name).map(String::as_str).or(fallback)?;
            out.push_str(&expand(replacement, p, depth + 1)?);
            if out.len() > LIMIT {
                return None;
            }
            remaining = &remaining[end + 1..];
        }
        out.push_str(remaining);
        if out.len() > LIMIT {
            None
        } else {
            Some(out)
        }
    }
    expand(value, properties, 0)
}
fn dependencies(value: &str, out: &mut HashSet<String>) {
    let mut remaining = value;
    while let Some(i) = next_function(remaining, "var(") {
        let Some(end) = function_end(remaining, i + 4) else {
            return;
        };
        let (name, fallback) = split_fallback(&remaining[i + 4..end]);
        out.insert(name.trim().into());
        if let Some(f) = fallback {
            dependencies(f, out);
        }
        remaining = &remaining[end + 1..];
    }
}
pub fn compute(mut properties: Properties) -> Option<Properties> {
    let graph: HashMap<String, HashSet<String>> = properties
        .iter()
        .map(|(n, v)| {
            let mut edges = HashSet::new();
            dependencies(v, &mut edges);
            (n.clone(), edges)
        })
        .collect();
    fn cyclic(
        current: &str,
        start: &str,
        g: &HashMap<String, HashSet<String>>,
        seen: &mut HashSet<String>,
    ) -> bool {
        if !seen.insert(current.into()) {
            return false;
        }
        g.get(current).is_some_and(|edges| {
            edges
                .iter()
                .any(|n| n == start || cyclic(n, start, g, seen))
        })
    }
    let bad: Vec<_> = graph
        .keys()
        .filter(|n| cyclic(n, n, &graph, &mut HashSet::new()))
        .cloned()
        .collect();
    for n in bad {
        properties.remove(&n);
    }
    let mut result = Properties::new();
    let mut bytes = 0usize;
    for (name, value) in &properties {
        if let Some(value) = substitute(value, &properties) {
            bytes = bytes.checked_add(name.len() + value.len())?;
            if bytes > LIMIT {
                return None;
            }
            result.insert(name.clone(), value);
        }
    }
    Some(result)
}

#[derive(Clone)]
struct Quantity {
    value: f64,
    unit: String,
}
struct Expression<'a> {
    s: &'a str,
    depth: u8,
}
impl Expression<'_> {
    fn ws(&mut self) {
        self.s = self.s.trim_start();
    }
    fn expr(&mut self) -> Option<Quantity> {
        let mut a = self.product()?;
        loop {
            self.ws();
            let op = self.s.chars().next();
            if !matches!(op, Some('+') | Some('-')) {
                return Some(a);
            }
            self.s = &self.s[1..];
            let b = self.product()?;
            if a.unit != b.unit {
                return None;
            }
            a.value += if op == Some('+') { b.value } else { -b.value };
        }
    }
    fn product(&mut self) -> Option<Quantity> {
        let mut a = self.atom()?;
        loop {
            self.ws();
            let op = self.s.chars().next();
            if !matches!(op, Some('*') | Some('/')) {
                return Some(a);
            }
            self.s = &self.s[1..];
            let b = self.atom()?;
            if op == Some('*') {
                if !a.unit.is_empty() && !b.unit.is_empty() {
                    return None;
                }
                if a.unit.is_empty() {
                    a.unit = b.unit;
                }
                a.value *= b.value;
            } else {
                if !b.unit.is_empty() || b.value == 0. {
                    return None;
                }
                a.value /= b.value;
            }
        }
    }
    fn atom(&mut self) -> Option<Quantity> {
        self.ws();
        if self.depth >= 32 {
            return None;
        }
        let mut sign = 1.;
        while self.s.starts_with(['+', '-']) {
            if self.s.starts_with('-') {
                sign = -sign;
            }
            self.s = &self.s[1..];
            self.ws();
        }
        if self.s.starts_with("calc(") {
            self.s = &self.s[4..];
        }
        if self.s.starts_with('(') {
            self.s = &self.s[1..];
            self.depth += 1;
            let mut q = self.expr()?;
            self.ws();
            self.s = self.s.strip_prefix(')')?;
            self.depth -= 1;
            q.value *= sign;
            return Some(q);
        }
        let mut end = 0;
        for (i, c) in self.s.char_indices() {
            if c.is_ascii_digit() || c == '.' {
                end = i + 1;
            } else {
                break;
            }
        }
        if end == 0 {
            return None;
        }
        if self.s[end..].starts_with(['e', 'E']) {
            let start = end;
            let mut n = end + 1;
            if self.s[n..].starts_with(['+', '-']) {
                n += 1;
            }
            let before = n;
            while self.s.as_bytes().get(n).is_some_and(u8::is_ascii_digit) {
                n += 1;
            }
            if n > before {
                end = n;
            } else {
                end = start;
            }
        }
        let value = self.s[..end].parse::<f64>().ok()? * sign;
        self.s = &self.s[end..];
        let mut end = 0;
        for (i, c) in self.s.char_indices() {
            if c.is_ascii_alphabetic() || c == '%' {
                end = i + 1;
            } else {
                break;
            }
        }
        let unit = &self.s[..end];
        self.s = &self.s[end..];
        let (factor, unit) = match unit {
            "" => (1., ""),
            "px" => (1., "px"),
            "in" => (96., "px"),
            "cm" => (96. / 2.54, "px"),
            "mm" => (96. / 25.4, "px"),
            "pt" => (96. / 72., "px"),
            "pc" => (16., "px"),
            "%" => (1., "%"),
            "em" => (1., "em"),
            "ex" => (1., "ex"),
            "deg" => (1., "deg"),
            "rad" => (180. / std::f64::consts::PI, "deg"),
            "grad" => (0.9, "deg"),
            "turn" => (360., "deg"),
            _ => return None,
        };
        Some(Quantity {
            value: value * factor,
            unit: unit.into(),
        })
    }
}
pub fn calculate(value: &str) -> Option<String> {
    if value.len() > LIMIT {
        return None;
    }
    let mut remaining = value;
    let mut out = String::new();
    while let Some(i) = next_function(remaining, "calc(") {
        out.push_str(&remaining[..i]);
        let end = function_end(remaining, i + 5)?;
        let mut expression = Expression {
            s: &remaining[i + 5..end],
            depth: 0,
        };
        let q = expression.expr()?;
        if !expression.s.trim().is_empty() || !q.value.is_finite() || q.value.abs() > 1e7 {
            return None;
        }
        out.push_str(&format!("{}{}", q.value, q.unit));
        remaining = &remaining[end + 1..];
    }
    out.push_str(remaining);
    if out.len() > LIMIT {
        None
    } else {
        Some(out)
    }
}
