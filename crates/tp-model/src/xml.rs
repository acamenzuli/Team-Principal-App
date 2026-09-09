//! Editing XML config files without damaging them.
//!
//! The same job as [`crate::ini`], for the games that use XML: Automobilista 2,
//! RaceRoom, the DiRT titles. And the same rule, for the same reason — **it
//! will not invent elements or attributes.** A tag name that is almost right is
//! silently ignored by the game, leaving an app that reports success and
//! changed nothing.
//!
//! ## Not a parser
//!
//! This does not build a document tree. It finds one element by name and
//! rewrites the smallest possible span of text — the value of an attribute, or
//! the text between a tag and its closer. Everything else, including comments,
//! declarations, namespaces, self-closing tags, attribute order and indentation,
//! comes out exactly as it went in.
//!
//! That is worth more here than in the INI case. These files are long, deeply
//! nested and largely made of settings this app has no opinion about, and a
//! round-trip through a real XML library reformats all of it — turning a
//! two-value change into a diff nobody can review.
//!
//! ## What it deliberately does not handle
//!
//! Namespaced lookups, XPath, repeated elements at different depths, CDATA and
//! entity-encoded values. None of the sim config files in scope need them, and
//! supporting them properly means the document tree this is avoiding.

/// One XML file, held as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xml {
    text: String,
}

/// Where a value lives, or why it does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlError {
    NoSuchElement { element: String },
    NoSuchAttribute { element: String, attribute: String },
}

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XmlError::NoSuchElement { element } => {
                write!(f, "this file has no <{element}> element")
            }
            XmlError::NoSuchAttribute { element, attribute } => {
                write!(f, "<{element}> has no {attribute} attribute")
            }
        }
    }
}

impl std::error::Error for XmlError {}

impl Xml {
    pub fn parse(text: &str) -> Xml {
        Xml {
            text: text.to_string(),
        }
    }

    pub fn to_text(&self) -> String {
        self.text.clone()
    }

    /// The text inside `<element>…</element>`, or an attribute's value when
    /// `attribute` is given.
    pub fn get(&self, element: &str, attribute: Option<&str>) -> Option<String> {
        let tag = self.find_tag(element)?;
        match attribute {
            Some(name) => attribute_span(&self.text[tag.open_start..tag.open_end], name)
                .map(|s| self.text[tag.open_start + s.start..tag.open_start + s.end].to_string()),
            None => tag.content.map(|(a, b)| self.text[a..b].trim().to_string()),
        }
    }

    /// Change a value in place. Fails rather than creating anything.
    pub fn set(
        &mut self,
        element: &str,
        attribute: Option<&str>,
        value: &str,
    ) -> Result<(), XmlError> {
        let tag = self
            .find_tag(element)
            .ok_or_else(|| XmlError::NoSuchElement {
                element: element.to_string(),
            })?;

        let span = match attribute {
            Some(name) => {
                let relative = attribute_span(&self.text[tag.open_start..tag.open_end], name)
                    .ok_or_else(|| XmlError::NoSuchAttribute {
                        element: element.to_string(),
                        attribute: name.to_string(),
                    })?;
                (
                    tag.open_start + relative.start,
                    tag.open_start + relative.end,
                )
            }
            None => tag.content.ok_or_else(|| XmlError::NoSuchElement {
                element: element.to_string(),
            })?,
        };

        // Keep the surrounding whitespace: these files indent their values, and
        // collapsing that turns a one-value change into a whole-line diff.
        let existing = &self.text[span.0..span.1];
        let lead: String = existing.chars().take_while(|c| c.is_whitespace()).collect();
        let trail: String = existing
            .chars()
            .rev()
            .take_while(|c| c.is_whitespace())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let replacement = if attribute.is_some() {
            value.to_string()
        } else {
            format!("{lead}{value}{trail}")
        };

        self.text.replace_range(span.0..span.1, &replacement);
        Ok(())
    }

    pub fn has(&self, element: &str, attribute: Option<&str>) -> bool {
        self.get(element, attribute).is_some()
    }

    /// Every element name in the file, in order, deduplicated.
    ///
    /// What the UI shows when a key is missing: "your file has these elements"
    /// is diagnosable, "element not found" is not.
    pub fn elements(&self) -> Vec<String> {
        let mut names = Vec::new();
        let bytes = self.text.as_bytes();
        let mut i = 0usize;
        while let Some(open) = self.text[i..].find('<') {
            let at = i + open;
            i = at + 1;
            // Skip comments, declarations, doctypes and closing tags.
            if matches!(bytes.get(i), Some(b'!') | Some(b'?') | Some(b'/')) {
                continue;
            }
            let name: String = self.text[i..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
                .collect();
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// Locate the first element with this name.
    fn find_tag(&self, element: &str) -> Option<Tag> {
        let mut from = 0usize;
        loop {
            let at = from + self.text[from..].find(&format!("<{element}"))?;
            from = at + 1;

            // `<Screen` must not match `<ScreenWidth`. The character after the
            // name has to end it.
            let after = at + 1 + element.len();
            let next = self.text[after..].chars().next()?;
            if !(next.is_whitespace() || next == '>' || next == '/') {
                continue;
            }

            // A comment is not an element.
            if self.text[..at]
                .rfind("<!--")
                .is_some_and(|c| self.text[c..at].find("-->").is_none())
            {
                continue;
            }

            let open_end = at + self.text[at..].find('>')? + 1;
            // Self-closing: `<Thing a="1" />` has no text content.
            let self_closing = self.text[at..open_end].trim_end_matches('>').ends_with('/');
            let content = if self_closing {
                None
            } else {
                let close = format!("</{element}>");
                self.text[open_end..]
                    .find(&close)
                    .map(|c| (open_end, open_end + c))
            };

            return Some(Tag {
                open_start: at,
                open_end,
                content,
            });
        }
    }
}

struct Tag {
    /// Byte index of `<`.
    open_start: usize,
    /// Byte index just past `>`.
    open_end: usize,
    /// Byte range of the text between the tags, when there is any.
    content: Option<(usize, usize)>,
}

struct Span {
    start: usize,
    end: usize,
}

/// The byte range of an attribute's value inside an opening tag, quotes
/// excluded.
fn attribute_span(open_tag: &str, name: &str) -> Option<Span> {
    let mut from = 0usize;
    loop {
        let at = from + open_tag[from..].find(name)?;
        from = at + 1;

        // Must be preceded by whitespace, or `type` would match `subtype`.
        if at == 0 || !open_tag[..at].ends_with(char::is_whitespace) {
            continue;
        }
        let rest = &open_tag[at + name.len()..];
        let trimmed = rest.trim_start();
        if !trimmed.starts_with('=') {
            continue;
        }
        let after_eq = at + name.len() + (rest.len() - trimmed.len()) + 1;
        let value_part = &open_tag[after_eq..];
        let quote_offset = value_part.find(['"', '\''])?;
        let quote = value_part.as_bytes()[quote_offset] as char;
        let value_start = after_eq + quote_offset + 1;
        let value_end = value_start + open_tag[value_start..].find(quote)?;
        return Some(Span {
            start: value_start,
            end: value_end,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped like RaceRoom's graphics_options.xml: typed elements, indented,
    /// with a comment and a self-closing tag.
    const SAMPLE: &str = "<?xml version=\"1.0\"?>\r\n\
        <graphics>\r\n\
        \x20 <!-- resolution -->\r\n\
        \x20 <screenWidth type=\"uint32\">2560</screenWidth>\r\n\
        \x20 <screenHeight type=\"uint32\">1080</screenHeight>\r\n\
        \x20 <fullscreen type=\"bool\">1</fullscreen>\r\n\
        \x20 <vsync enabled=\"0\" />\r\n\
        </graphics>\r\n";

    #[test]
    fn a_file_read_and_written_unchanged_is_byte_identical() {
        assert_eq!(Xml::parse(SAMPLE).to_text(), SAMPLE);
    }

    #[test]
    fn element_text_is_read_and_written() {
        let mut xml = Xml::parse(SAMPLE);
        assert_eq!(xml.get("screenWidth", None), Some("2560".into()));
        xml.set("screenWidth", None, "5760").unwrap();
        assert!(xml
            .to_text()
            .contains("<screenWidth type=\"uint32\">5760</screenWidth>"));
    }

    #[test]
    fn an_attribute_is_read_and_written() {
        let mut xml = Xml::parse(SAMPLE);
        assert_eq!(xml.get("screenWidth", Some("type")), Some("uint32".into()));
        xml.set("vsync", Some("enabled"), "1").unwrap();
        assert!(xml.to_text().contains("<vsync enabled=\"1\" />"));
    }

    #[test]
    fn changing_one_value_leaves_the_rest_byte_identical() {
        // The whole reason this is not a document tree: a round-trip through a
        // real XML library reformats everything and turns a two-value change
        // into a diff nobody can review.
        let mut xml = Xml::parse(SAMPLE);
        xml.set("screenHeight", None, "1440").unwrap();
        let out = xml.to_text();
        assert!(out.contains("<!-- resolution -->"), "comment kept");
        assert!(out.contains("\r\n"), "CRLF kept");
        assert!(out.contains("<?xml version=\"1.0\"?>"), "declaration kept");
        // 1080 and 1440 are both four characters, so nothing else moved.
        assert_eq!(SAMPLE.len(), out.len());
        assert_eq!(SAMPLE.replace("1080", "1440"), out);
    }

    #[test]
    fn a_missing_element_is_an_error_not_a_new_tag() {
        let mut xml = Xml::parse(SAMPLE);
        assert_eq!(
            xml.set("refreshRate", None, "120").unwrap_err(),
            XmlError::NoSuchElement {
                element: "refreshRate".into()
            }
        );
        assert_eq!(xml.to_text(), SAMPLE, "nothing was written");
    }

    #[test]
    fn a_missing_attribute_blames_the_attribute_not_the_element() {
        let mut xml = Xml::parse(SAMPLE);
        assert_eq!(
            xml.set("vsync", Some("interval"), "1").unwrap_err(),
            XmlError::NoSuchAttribute {
                element: "vsync".into(),
                attribute: "interval".into()
            }
        );
    }

    #[test]
    fn a_name_that_is_a_prefix_of_another_does_not_match_it() {
        // <screen> must not find <screenWidth>. This is the bug that writes a
        // resolution into whatever element happened to sort first.
        let xml = Xml::parse(SAMPLE);
        assert_eq!(xml.get("screen", None), None);
        assert_eq!(xml.get("screenWidth", None), Some("2560".into()));
    }

    #[test]
    fn an_attribute_name_that_is_a_suffix_of_another_does_not_match_it() {
        let xml = Xml::parse("<a subtype=\"x\" type=\"y\" />");
        assert_eq!(xml.get("a", Some("type")), Some("y".into()));
    }

    #[test]
    fn a_commented_out_element_is_not_an_element() {
        let xml = Xml::parse("<r>\n  <!-- <width>1</width> -->\n  <width>2</width>\n</r>");
        assert_eq!(xml.get("width", None), Some("2".into()));
    }

    #[test]
    fn single_quoted_attributes_work_too() {
        let mut xml = Xml::parse("<a b='1' />");
        assert_eq!(xml.get("a", Some("b")), Some("1".into()));
        xml.set("a", Some("b"), "2").unwrap();
        assert_eq!(xml.to_text(), "<a b='2' />");
    }

    #[test]
    fn indentation_around_a_value_survives() {
        let mut xml = Xml::parse("<r>\n  <w>\n    1920\n  </w>\n</r>");
        xml.set("w", None, "5760").unwrap();
        assert_eq!(xml.to_text(), "<r>\n  <w>\n    5760\n  </w>\n</r>");
    }

    #[test]
    fn elements_lists_what_the_file_actually_has() {
        let xml = Xml::parse(SAMPLE);
        let names = xml.elements();
        assert!(names.contains(&"screenWidth".to_string()));
        assert!(names.contains(&"vsync".to_string()));
        // The declaration is not an element.
        assert!(!names.iter().any(|n| n.starts_with("xml")));
    }
}
