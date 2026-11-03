use std::{borrow::Cow, collections::HashMap, fmt, io::Write};

use quick_xml::{
    Writer, escape,
    events::{BytesEnd, BytesStart, BytesText, Event, attributes::Attribute},
    name::{Namespace, QName, ResolveResult},
    reader::NsReader,
};
use thiserror::Error;

const DEFAULT_MAX_XML_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_DEPTH: usize = 64;
const DEFAULT_MAX_NODES: usize = 100_000;
const DEFAULT_MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_XML_BYTES: usize = 64 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 256;
const MAX_XML_NODES: usize = 1_000_000;
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// Bounded limits applied while reading and writing XML documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XmlLimits {
    max_bytes: usize,
    max_depth: usize,
    max_nodes: usize,
    max_text_bytes: usize,
}

impl XmlLimits {
    /// Creates limits within hard parser and allocator bounds.
    pub fn new(
        max_bytes: usize,
        max_depth: usize,
        max_nodes: usize,
        max_text_bytes: usize,
    ) -> Result<Self, XmlError> {
        if max_bytes == 0
            || max_bytes > MAX_XML_BYTES
            || max_depth == 0
            || max_depth > MAX_XML_DEPTH
            || max_nodes == 0
            || max_nodes > MAX_XML_NODES
            || max_text_bytes == 0
            || max_text_bytes > max_bytes
        {
            return Err(XmlError::InvalidLimits);
        }
        Ok(Self {
            max_bytes,
            max_depth,
            max_nodes,
            max_text_bytes,
        })
    }

    fn check_document_size(self, size: usize) -> Result<(), XmlError> {
        if size > self.max_bytes {
            return Err(XmlError::BodyTooLarge {
                limit: self.max_bytes,
            });
        }
        Ok(())
    }
}

impl Default for XmlLimits {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_XML_BYTES,
            max_depth: DEFAULT_MAX_DEPTH,
            max_nodes: DEFAULT_MAX_NODES,
            max_text_bytes: DEFAULT_MAX_TEXT_BYTES,
        }
    }
}

/// Parses one UTF-8 XML document with namespace resolution and strict resource limits.
///
/// DTD declarations and non-predefined entity references are rejected. The default
/// limits are 16 MiB per document, depth 64, 100,000 elements, and 8 MiB of text.
pub fn parse_xml(document: &[u8], limits: XmlLimits) -> Result<XmlElement, XmlError> {
    limits.check_document_size(document.len())?;
    std::str::from_utf8(document).map_err(|_| XmlError::InvalidEncoding)?;

    let mut reader = NsReader::from_reader(document);
    reader.config_mut().trim_text(false);
    reader.config_mut().check_end_names = true;
    reader.resolver_mut().set_max_declarations_per_element(64);
    let mut buffer = Vec::new();
    let mut stack: Vec<XmlElement> = Vec::new();
    let mut root = None;
    let mut budget = XmlBudget::new(limits);

    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| XmlError::Malformed)?;
        match event {
            Event::Start(start) => {
                let element = parsed_element(&reader, &start)?;
                budget.add_node(stack.len() + 1)?;
                stack.push(element);
            }
            Event::Empty(start) => {
                let element = parsed_element(&reader, &start)?;
                budget.add_node(stack.len() + 1)?;
                attach_element(element, &mut stack, &mut root)?;
            }
            Event::End(_) => {
                let element = stack.pop().ok_or(XmlError::InvalidDocument)?;
                attach_element(element, &mut stack, &mut root)?;
            }
            Event::Text(text) => {
                let text = text.xml10_content().map_err(|_| XmlError::Malformed)?;
                append_text(&text, &mut stack, &mut budget)?;
            }
            Event::CData(text) => {
                let text = text.decode().map_err(|_| XmlError::Malformed)?;
                append_text(&text, &mut stack, &mut budget)?;
            }
            Event::GeneralRef(reference) => {
                let character = if let Some(character) = reference
                    .resolve_char_ref()
                    .map_err(|_| XmlError::Malformed)?
                {
                    character
                } else {
                    match reference.as_ref() {
                        b"lt" => '<',
                        b"gt" => '>',
                        b"amp" => '&',
                        b"apos" => '\'',
                        b"quot" => '"',
                        _ => return Err(XmlError::ForbiddenEntity),
                    }
                };
                append_text(&character.to_string(), &mut stack, &mut budget)?;
            }
            Event::DocType(_) => return Err(XmlError::ForbiddenDtd),
            Event::Decl(declaration) => {
                if let Some(encoding) = declaration.encoding() {
                    let encoding = encoding.map_err(|_| XmlError::Malformed)?;
                    if !encoding.eq_ignore_ascii_case(b"UTF-8") {
                        return Err(XmlError::InvalidEncoding);
                    }
                }
            }
            Event::Eof => break,
            Event::Comment(_) | Event::PI(_) => {}
        }
        buffer.clear();
    }

    if !stack.is_empty() {
        return Err(XmlError::InvalidDocument);
    }
    root.ok_or(XmlError::InvalidDocument)
}

/// Safe, non-sensitive XML parsing and encoding failures.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum XmlError {
    /// The XML element or attribute name is not a supported local name.
    #[error("invalid XML name")]
    InvalidName,
    /// A value contains characters that XML 1.0 does not allow.
    #[error("invalid XML character")]
    InvalidCharacter,
    /// XML limits are zero or exceed the supported hard bounds.
    #[error("invalid XML limits")]
    InvalidLimits,
    /// The input exceeds the configured total document size.
    #[error("XML document exceeds the configured limit of {limit} bytes")]
    BodyTooLarge { limit: usize },
    /// The document exceeds the configured nesting depth.
    #[error("XML document exceeds the configured depth limit of {limit}")]
    DepthLimitExceeded { limit: usize },
    /// The document exceeds the configured element count.
    #[error("XML document exceeds the configured node limit of {limit}")]
    NodeLimitExceeded { limit: usize },
    /// The document exceeds the configured aggregate text limit.
    #[error("XML document exceeds the configured text limit of {limit} bytes")]
    TextLimitExceeded { limit: usize },
    /// Input is not valid UTF-8.
    #[error("XML document is not valid UTF-8")]
    InvalidEncoding,
    /// The XML document could not be parsed.
    #[error("malformed XML document")]
    Malformed,
    /// DTD declarations are not allowed.
    #[error("XML document type declarations are not allowed")]
    ForbiddenDtd,
    /// Entity references other than XML's predefined entities are not allowed.
    #[error("XML document contains an unsupported entity reference")]
    ForbiddenEntity,
    /// The document does not contain exactly one complete root element.
    #[error("XML document does not have a single root element")]
    InvalidDocument,
}

/// An XML attribute with a namespace URI and validated local name.
#[derive(Clone, PartialEq, Eq)]
pub struct XmlAttribute {
    namespace_uri: String,
    local_name: String,
    value: String,
}

impl fmt::Debug for XmlAttribute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("XmlAttribute")
            .field("namespace_uri", &self.namespace_uri)
            .field("local_name", &self.local_name)
            .field("value", &"[redacted]")
            .finish()
    }
}

impl XmlAttribute {
    /// Creates an attribute with a namespace URI, local name, and value.
    pub fn new(
        namespace_uri: impl Into<String>,
        local_name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, XmlError> {
        let namespace_uri = namespace_uri.into();
        let local_name = local_name.into();
        if !is_valid_local_name(&local_name)
            || local_name == "xmlns"
            || namespace_uri.len() > 4096
            || namespace_uri == XMLNS_NAMESPACE
        {
            return Err(XmlError::InvalidName);
        }
        validate_xml_chars(&namespace_uri)?;
        let value = value.into();
        validate_xml_chars(&value)?;
        Ok(Self {
            namespace_uri,
            local_name,
            value,
        })
    }

    /// Returns this attribute's namespace URI.
    pub fn namespace_uri(&self) -> &str {
        &self.namespace_uri
    }

    /// Returns this attribute's local name.
    pub fn local_name(&self) -> &str {
        &self.local_name
    }

    /// Returns this attribute's value.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Ordered text or child-element content.
#[derive(Clone, PartialEq, Eq)]
pub enum XmlContent {
    /// Character data, encoded as escaped XML text.
    Text(String),
    /// A nested element.
    Element(XmlElement),
}

impl fmt::Debug for XmlContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(_) => formatter.write_str("Text([redacted])"),
            Self::Element(element) => formatter.debug_tuple("Element").field(element).finish(),
        }
    }
}

/// A namespace-aware XML element suitable for SOAP wire models.
#[derive(Clone, PartialEq, Eq)]
pub struct XmlElement {
    namespace_uri: String,
    local_name: String,
    attributes: Vec<XmlAttribute>,
    content: Vec<XmlContent>,
}

impl XmlElement {
    /// Creates an element with a namespace URI and validated local name.
    pub fn new(
        namespace_uri: impl Into<String>,
        local_name: impl Into<String>,
    ) -> Result<Self, XmlError> {
        let namespace_uri = namespace_uri.into();
        let local_name = local_name.into();
        if !is_valid_local_name(&local_name)
            || namespace_uri.len() > 4096
            || namespace_uri == XMLNS_NAMESPACE
        {
            return Err(XmlError::InvalidName);
        }
        validate_xml_chars(&namespace_uri)?;
        Ok(Self {
            namespace_uri,
            local_name,
            attributes: Vec::new(),
            content: Vec::new(),
        })
    }

    /// Appends an attribute.
    pub fn with_attribute(mut self, attribute: XmlAttribute) -> Self {
        self.attributes.push(attribute);
        self
    }

    /// Appends character data.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.content.push(XmlContent::Text(text.into()));
        self
    }

    /// Appends a nested element.
    pub fn with_child(mut self, child: XmlElement) -> Self {
        self.content.push(XmlContent::Element(child));
        self
    }

    /// Returns this element's namespace URI.
    pub fn namespace_uri(&self) -> &str {
        &self.namespace_uri
    }

    /// Returns this element's local name.
    pub fn local_name(&self) -> &str {
        &self.local_name
    }

    /// Returns attributes in document order.
    pub fn attributes(&self) -> &[XmlAttribute] {
        &self.attributes
    }

    /// Returns child elements in document order.
    pub fn children(&self) -> impl Iterator<Item = &XmlElement> {
        self.content.iter().filter_map(|content| match content {
            XmlContent::Element(element) => Some(element),
            XmlContent::Text(_) => None,
        })
    }

    /// Returns ordered text and child-element content.
    pub fn content(&self) -> &[XmlContent] {
        &self.content
    }

    /// Returns direct text content, concatenated in document order.
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|content| match content {
                XmlContent::Text(text) => Some(text.as_str()),
                XmlContent::Element(_) => None,
            })
            .collect()
    }
}

impl fmt::Debug for XmlElement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("XmlElement")
            .field("namespace_uri", &self.namespace_uri)
            .field("local_name", &self.local_name)
            .field("attributes", &self.attributes)
            .field("content", &self.content)
            .finish()
    }
}

/// Encodes an XML element tree using escaped text and namespace-aware names.
pub fn encode_xml(root: &XmlElement, limits: XmlLimits) -> Result<Vec<u8>, XmlError> {
    let mut budget = XmlBudget::new(limits);
    validate_element(root, 1, &mut budget)?;

    let mut writer = Writer::new(BoundedXmlBuffer::new(limits.max_bytes));
    let mut namespaces = NamespacePrefixes::default();
    write_element(&mut writer, root, &mut namespaces)?;
    let output = writer.into_inner().bytes;
    limits.check_document_size(output.len())?;
    Ok(output)
}

fn parsed_element(
    reader: &NsReader<&[u8]>,
    start: &BytesStart<'_>,
) -> Result<XmlElement, XmlError> {
    let (namespace, local_name) = reader.resolver().resolve_element(start.name());
    let namespace = match namespace {
        ResolveResult::Bound(Namespace(namespace)) => std::str::from_utf8(namespace)
            .map_err(|_| XmlError::InvalidEncoding)?
            .to_owned(),
        ResolveResult::Unbound => String::new(),
        ResolveResult::Unknown(_) => return Err(XmlError::Malformed),
    };
    let local_name =
        std::str::from_utf8(local_name.as_ref()).map_err(|_| XmlError::InvalidEncoding)?;
    let mut element = XmlElement::new(namespace, local_name)?;

    for attribute in start.attributes().with_checks(true) {
        let attribute = attribute.map_err(|_| XmlError::Malformed)?;
        if attribute.key.as_ref() == b"xmlns" || attribute.key.as_ref().starts_with(b"xmlns:") {
            continue;
        }
        let (attribute_namespace, local_name) = reader.resolver().resolve_attribute(attribute.key);
        let namespace = match attribute_namespace {
            ResolveResult::Bound(Namespace(namespace)) => {
                std::str::from_utf8(namespace).map_err(|_| XmlError::InvalidEncoding)?
            }
            ResolveResult::Unbound => "",
            ResolveResult::Unknown(_) => return Err(XmlError::Malformed),
        };
        let local_name =
            std::str::from_utf8(local_name.as_ref()).map_err(|_| XmlError::InvalidEncoding)?;
        let value = attribute
            .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|_| XmlError::Malformed)?;
        element.attributes.push(XmlAttribute::new(
            namespace,
            local_name,
            value.into_owned(),
        )?);
    }
    Ok(element)
}

fn append_text(
    text: &str,
    stack: &mut [XmlElement],
    budget: &mut XmlBudget,
) -> Result<(), XmlError> {
    budget.add_text(text)?;
    if let Some(element) = stack.last_mut() {
        element.content.push(XmlContent::Text(text.to_owned()));
    } else if !text.trim().is_empty() {
        return Err(XmlError::InvalidDocument);
    }
    Ok(())
}

fn attach_element(
    element: XmlElement,
    stack: &mut [XmlElement],
    root: &mut Option<XmlElement>,
) -> Result<(), XmlError> {
    if let Some(parent) = stack.last_mut() {
        parent.content.push(XmlContent::Element(element));
    } else if root.replace(element).is_some() {
        return Err(XmlError::InvalidDocument);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct XmlBudget {
    limits: XmlLimits,
    nodes: usize,
    text_bytes: usize,
}

impl XmlBudget {
    fn new(limits: XmlLimits) -> Self {
        Self {
            limits,
            nodes: 0,
            text_bytes: 0,
        }
    }

    fn add_node(&mut self, depth: usize) -> Result<(), XmlError> {
        if depth > self.limits.max_depth {
            return Err(XmlError::DepthLimitExceeded {
                limit: self.limits.max_depth,
            });
        }
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > self.limits.max_nodes {
            return Err(XmlError::NodeLimitExceeded {
                limit: self.limits.max_nodes,
            });
        }
        Ok(())
    }

    fn add_text(&mut self, text: &str) -> Result<(), XmlError> {
        validate_xml_chars(text)?;
        self.text_bytes = self.text_bytes.saturating_add(text.len());
        if self.text_bytes > self.limits.max_text_bytes {
            return Err(XmlError::TextLimitExceeded {
                limit: self.limits.max_text_bytes,
            });
        }
        Ok(())
    }
}

fn validate_element(
    element: &XmlElement,
    depth: usize,
    budget: &mut XmlBudget,
) -> Result<(), XmlError> {
    budget.add_node(depth)?;
    validate_xml_chars(&element.namespace_uri)?;
    for attribute in &element.attributes {
        validate_xml_chars(&attribute.namespace_uri)?;
        validate_xml_chars(&attribute.value)?;
    }
    for content in &element.content {
        match content {
            XmlContent::Text(text) => budget.add_text(text)?,
            XmlContent::Element(child) => validate_element(child, depth + 1, budget)?,
        }
    }
    Ok(())
}

#[derive(Default)]
struct NamespacePrefixes {
    by_uri: HashMap<String, String>,
    next: usize,
}

impl NamespacePrefixes {
    fn prefix(&mut self, uri: &str) -> Option<String> {
        if uri.is_empty() {
            return None;
        }
        if uri == XML_NAMESPACE {
            return Some("xml".to_owned());
        }
        if let Some(prefix) = self.by_uri.get(uri) {
            return Some(prefix.clone());
        }
        let prefix = match uri {
            "http://schemas.xmlsoap.org/soap/envelope/" => "soapenv".to_owned(),
            "urn:metadata.soap.sforce.com" => "met".to_owned(),
            _ => {
                let prefix = format!("n{}", self.next);
                self.next += 1;
                prefix
            }
        };
        self.by_uri.insert(uri.to_owned(), prefix.clone());
        Some(prefix)
    }
}

fn write_element(
    writer: &mut Writer<BoundedXmlBuffer>,
    element: &XmlElement,
    namespaces: &mut NamespacePrefixes,
) -> Result<(), XmlError> {
    let element_prefix = namespaces.prefix(&element.namespace_uri);
    let element_name = qualified_name(element_prefix.as_deref(), &element.local_name);
    let mut start = BytesStart::new(element_name.as_str());
    let mut declarations = Vec::new();
    if let Some(prefix) = element_prefix.as_deref()
        && prefix != "xml"
    {
        add_namespace_declaration(&mut declarations, prefix, &element.namespace_uri);
    }

    let mut attributes = Vec::with_capacity(element.attributes.len());
    for attribute in &element.attributes {
        let prefix = namespaces.prefix(&attribute.namespace_uri);
        let name = qualified_name(prefix.as_deref(), &attribute.local_name);
        attributes.push((name, escaped_attribute_value(&attribute.value)));
        if let Some(prefix) = prefix.as_deref()
            && prefix != "xml"
        {
            add_namespace_declaration(&mut declarations, prefix, &attribute.namespace_uri);
        }
    }
    for (name, value) in declarations.iter().chain(attributes.iter()) {
        start.push_attribute(Attribute {
            key: QName(name.as_bytes()),
            value: Cow::Owned(value.as_bytes().to_vec()),
        });
    }
    writer
        .write_event(Event::Start(start))
        .map_err(|_| XmlError::BodyTooLarge {
            limit: writer.get_ref().limit,
        })?;

    for content in &element.content {
        match content {
            XmlContent::Text(text) => writer
                .write_event(Event::Text(BytesText::from_escaped(Cow::Owned(
                    escaped_text(text),
                ))))
                .map_err(|_| XmlError::BodyTooLarge {
                    limit: writer.get_ref().limit,
                })?,
            XmlContent::Element(child) => {
                write_element(writer, child, namespaces)?;
            }
        }
    }

    writer
        .write_event(Event::End(BytesEnd::new(element_name.as_str())))
        .map_err(|_| XmlError::BodyTooLarge {
            limit: writer.get_ref().limit,
        })?;
    Ok(())
}

struct BoundedXmlBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl BoundedXmlBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}

impl Write for BoundedXmlBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(std::io::Error::other("XML output limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn add_namespace_declaration(
    declarations: &mut Vec<(String, String)>,
    prefix: &str,
    namespace_uri: &str,
) {
    let name = format!("xmlns:{prefix}");
    if !declarations.iter().any(|(existing, _)| existing == &name) {
        declarations.push((name, escaped_attribute_value(namespace_uri)));
    }
}

fn escaped_attribute_value(value: &str) -> String {
    escape::escape(value)
        .replace('\t', "&#x9;")
        .replace('\n', "&#xA;")
        .replace('\r', "&#xD;")
}

fn escaped_text(value: &str) -> String {
    escape::escape(value).replace('\r', "&#xD;")
}

fn qualified_name(prefix: Option<&str>, local_name: &str) -> String {
    match prefix {
        Some(prefix) => format!("{prefix}:{local_name}"),
        None => local_name.to_owned(),
    }
}

fn is_valid_local_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| {
            matches!(
                byte,
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'
            )
        })
}

fn validate_xml_chars(value: &str) -> Result<(), XmlError> {
    if value.chars().all(|character| {
        matches!(
            character as u32,
            0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
        )
    }) {
        Ok(())
    } else {
        Err(XmlError::InvalidCharacter)
    }
}
