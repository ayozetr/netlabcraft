//! A small, strict XML 1.0 reader and writer for NETCONF messages: elements, attributes,
//! namespaces (prefixes resolved, `xmlns` inherited), text with the five predefined entities and
//! character references, comments and the XML declaration.
//!
//! Hostile input is expected: no DOCTYPE (so no entity expansion), no CDATA, and caps on size
//! ([`MAX_INPUT`]), nesting ([`MAX_DEPTH`]) and element count ([`MAX_ELEMENTS`]).

use crate::{Result, WireError};

pub const MAX_INPUT: usize = 256 * 1024;
pub const MAX_DEPTH: usize = 48;
pub const MAX_ELEMENTS: usize = 8_192;

const WHAT: &str = "XML";

fn bad(why: impl Into<String>) -> WireError {
    WireError::Unsupported(WHAT, why.into())
}

/// An element.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Element {
    /// The local name (`rpc` for `nc:rpc`).
    pub name: String,
    /// The namespace URI in scope for the element, if any.
    pub ns: Option<String>,
    /// Attributes other than namespace declarations: (local name, value).
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    /// The element's own text, entities decoded, trimmed.
    pub text: String,
}

impl Element {
    pub fn new(name: &str) -> Element {
        Element { name: name.to_string(), ..Element::default() }
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// A child's text.
    pub fn text_of(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.as_str())
    }
}

/// Escapes text for an element body or an attribute value.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(s: &str) -> Result<String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or_default());
        let after = rest.get(i + 1..).unwrap_or_default();
        let end = after.find(';').filter(|e| *e <= 10).ok_or_else(|| bad("an unterminated entity"))?;
        let ent = after.get(..end).unwrap_or_default();
        let c = match ent {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let n = if let Some(h) = ent.strip_prefix("#x") {
                    u32::from_str_radix(h, 16).ok()
                } else if let Some(d) = ent.strip_prefix('#') {
                    d.parse().ok()
                } else {
                    None
                };
                n.and_then(char::from_u32).ok_or_else(|| bad(format!("unknown entity &{ent};")))?
            }
        };
        out.push(c);
        rest = after.get(end + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    Ok(out)
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || "_-.:".contains(c)
}

struct Open {
    el: Element,
    qname: String,
    /// Namespace declarations made here: (prefix, or "" for the default; URI).
    decls: Vec<(String, String)>,
}

fn lookup(stack: &[Open], prefix: &str) -> Option<String> {
    stack.iter().rev().find_map(|o| o.decls.iter().find(|(p, _)| p == prefix).map(|(_, u)| u.clone())).filter(|u| !u.is_empty())
}

/// Parses one document; the root element is returned.
pub fn parse(input: &str) -> Result<Element> {
    if input.len() > MAX_INPUT {
        return Err(bad(format!("document larger than {MAX_INPUT} bytes")));
    }
    let mut s = input.trim_start_matches('\u{feff}');
    let mut stack: Vec<Open> = Vec::new();
    let mut root: Option<Element> = None;
    let mut count = 0usize;
    loop {
        let lt = s.find('<');
        let text = match lt {
            Some(i) => s.get(..i).unwrap_or_default(),
            None => s,
        };
        if !text.trim().is_empty() {
            match stack.last_mut() {
                Some(o) => {
                    let t = unescape(text)?;
                    o.el.text.push_str(&t);
                }
                None => return Err(bad("text outside the root element")),
            }
        }
        let Some(i) = lt else { break };
        s = s.get(i..).unwrap_or_default();
        if let Some(r) = s.strip_prefix("<?") {
            let end = r.find("?>").ok_or_else(|| bad("an unterminated processing instruction"))?;
            s = r.get(end + 2..).unwrap_or_default();
            continue;
        }
        if let Some(r) = s.strip_prefix("<!--") {
            let end = r.find("-->").ok_or_else(|| bad("an unterminated comment"))?;
            s = r.get(end + 3..).unwrap_or_default();
            continue;
        }
        if s.starts_with("<!") {
            return Err(bad("DOCTYPE and CDATA are not accepted"));
        }
        if let Some(r) = s.strip_prefix("</") {
            let end = r.find('>').ok_or_else(|| bad("an unterminated end tag"))?;
            let name = r.get(..end).unwrap_or_default().trim();
            let open = stack.pop().ok_or_else(|| bad(format!("an end tag </{name}> with nothing open")))?;
            if open.qname != name {
                return Err(bad(format!("<{}> closed by </{name}>", open.qname)));
            }
            close(open, &mut stack, &mut root)?;
            s = r.get(end + 1..).unwrap_or_default();
            continue;
        }
        // A start tag.
        let r = s.get(1..).unwrap_or_default();
        let end = tag_end(r).ok_or_else(|| bad("an unterminated start tag"))?;
        let inner = r.get(..end).unwrap_or_default();
        s = r.get(end + 1..).unwrap_or_default();
        let (inner, empty) = match inner.strip_suffix('/') {
            Some(x) => (x, true),
            None => (inner, false),
        };
        let qname: String = inner.chars().take_while(|c| is_name_char(*c)).collect();
        if qname.is_empty() || qname.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.') {
            return Err(bad("a start tag without a name"));
        }
        count += 1;
        if count > MAX_ELEMENTS {
            return Err(bad(format!("more than {MAX_ELEMENTS} elements")));
        }
        if stack.len() >= MAX_DEPTH {
            return Err(bad(format!("nested deeper than {MAX_DEPTH}")));
        }
        if root.is_some() {
            return Err(bad("a second root element"));
        }
        let mut decls = Vec::new();
        let mut attrs = Vec::new();
        let mut a = inner.get(qname.len()..).unwrap_or_default();
        loop {
            let t = a.trim_start();
            if t.is_empty() {
                break;
            }
            if t.len() == a.len() {
                return Err(bad("attributes must be separated by spaces"));
            }
            let n: String = t.chars().take_while(|c| is_name_char(*c)).collect();
            let rest = t.get(n.len()..).unwrap_or_default().trim_start();
            let rest = rest.strip_prefix('=').ok_or_else(|| bad(format!("attribute `{n}` has no value")))?.trim_start();
            let q = rest.chars().next().filter(|c| *c == '"' || *c == '\'').ok_or_else(|| bad("an unquoted attribute value"))?;
            let body = rest.get(1..).unwrap_or_default();
            let close = body.find(q).ok_or_else(|| bad("an unterminated attribute value"))?;
            let value = unescape(body.get(..close).unwrap_or_default())?;
            if n.is_empty() {
                return Err(bad("an attribute without a name"));
            }
            if n == "xmlns" {
                decls.push((String::new(), value));
            } else if let Some(p) = n.strip_prefix("xmlns:") {
                decls.push((p.to_string(), value));
            } else {
                let local = n.rsplit(':').next().unwrap_or(&n).to_string();
                if attrs.len() >= 32 {
                    return Err(bad("too many attributes"));
                }
                attrs.push((local, value));
            }
            a = body.get(close + 1..).unwrap_or_default();
        }
        let (prefix, local) = match qname.split_once(':') {
            Some((p, l)) => (p.to_string(), l.to_string()),
            None => (String::new(), qname.clone()),
        };
        let mut open = Open { el: Element { name: local, attrs, ..Element::default() }, qname, decls };
        let ns = open.decls.iter().find(|(p, _)| *p == prefix).map(|(_, u)| u.clone()).filter(|u| !u.is_empty()).or_else(|| lookup(&stack, &prefix));
        if !prefix.is_empty() && ns.is_none() {
            return Err(bad(format!("the prefix `{prefix}` is not declared")));
        }
        open.el.ns = ns;
        if empty {
            close(open, &mut stack, &mut root)?;
        } else {
            stack.push(open);
        }
    }
    if let Some(o) = stack.last() {
        return Err(bad(format!("<{}> is never closed", o.qname)));
    }
    root.ok_or_else(|| bad("no element"))
}

/// Where a start tag's `>` is (outside quoted values).
fn tag_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '>') => return Some(i),
            (None, '<') => return None,
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            _ => {}
        }
    }
    None
}

fn close(mut open: Open, stack: &mut [Open], root: &mut Option<Element>) -> Result<()> {
    open.el.text = open.el.text.trim().to_string();
    match stack.last_mut() {
        Some(parent) => parent.el.children.push(open.el),
        None => *root = Some(open.el),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_netconf_rpc() {
        let doc = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- a comment -->
<nc:rpc message-id="101" xmlns:nc="urn:ietf:params:xml:ns:netconf:base:1.0">
  <nc:edit-config>
    <nc:target><nc:running/></nc:target>
    <nc:config>
      <interfaces xmlns="urn:ietf:params:xml:ns:yang:ietf-interfaces">
        <interface nc:operation="merge"><name>GigabitEthernet0/0</name><description>to &lt;core&gt; &amp; &#65;&#x42;</description></interface>
      </interfaces>
    </nc:config>
  </nc:edit-config>
</nc:rpc>"#;
        let r = parse(doc).unwrap();
        assert_eq!((r.name.as_str(), r.attr("message-id"), r.ns.as_deref()), ("rpc", Some("101"), Some("urn:ietf:params:xml:ns:netconf:base:1.0")));
        let ifs = r.child("edit-config").and_then(|e| e.child("config")).and_then(|c| c.child("interfaces")).unwrap();
        assert_eq!(ifs.ns.as_deref(), Some("urn:ietf:params:xml:ns:yang:ietf-interfaces"));
        let i = ifs.child("interface").unwrap();
        assert_eq!(i.attr("operation"), Some("merge"));
        assert_eq!(i.text_of("description"), Some("to <core> & AB"));
        assert_eq!(i.child("name").unwrap().ns.as_deref(), Some("urn:ietf:params:xml:ns:yang:ietf-interfaces"));
        assert_eq!(escape("<a & 'b'>"), "&lt;a &amp; &apos;b&apos;&gt;");
    }

    #[test]
    fn hostile_documents_are_refused() {
        let deep = "<a>".repeat(MAX_DEPTH + 1) + &"</a>".repeat(MAX_DEPTH + 1);
        let wide = format!("<r>{}</r>", "<a/>".repeat(MAX_ELEMENTS + 1));
        let big = format!("<r>{}</r>", "x".repeat(MAX_INPUT));
        for doc in [
            "",
            "text",
            "<a>",
            "<a></b>",
            "</a>",
            "<a/><b/>",
            "<!DOCTYPE a [<!ENTITY x 'y'>]><a>&x;</a>",
            "<a><![CDATA[x]]></a>",
            "<a b></a>",
            "<a b=c></a>",
            "<a b='c></a>",
            "<p:a/>",
            "<a>&bogus;</a>",
            "<a>&#xFFFFFFFF;</a>",
            "<a>&amp</a>",
            "<1a/>",
            "<a b='1'c='2'/>",
            deep.as_str(),
            wide.as_str(),
            big.as_str(),
        ] {
            assert!(parse(doc).is_err(), "{}", doc.chars().take(60).collect::<String>());
        }
        // Every prefix of a valid document fails cleanly or parses.
        let doc = r#"<a x="1"><b>t&amp;</b><!-- c --><c/></a>"#;
        for cut in 0..doc.len() {
            if let Some(p) = doc.get(..cut) {
                let _ = parse(p);
            }
        }
    }
}
