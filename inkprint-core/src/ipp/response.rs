use super::types::*;

pub struct IppResponseBuilder {
    version: IppVersion,
    status_code: IppStatusCode,
    request_id: u32,
    groups: Vec<IppAttributeGroup>,
}

impl IppResponseBuilder {
    pub fn new(status_code: IppStatusCode, request_id: u32) -> Self {
        Self {
            version: IppVersion::IPP_2_0,
            status_code,
            request_id,
            groups: vec![],
        }
    }

    pub fn version(mut self, v: IppVersion) -> Self {
        self.version = v;
        self
    }

    pub fn add_group(mut self, group: IppAttributeGroup) -> Self {
        self.groups.push(group);
        self
    }

    pub fn build(self) -> IppResponse {
        IppResponse {
            version: self.version,
            status_code: self.status_code,
            request_id: self.request_id,
            attribute_groups: self.groups,
        }
    }
}

/// Write (name_len name value_len value) — the two length-prefixed fields of an IPP TLV.
fn write_name_value(buf: &mut Vec<u8>, name: &[u8], value: &[u8]) {
    buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
    buf.extend_from_slice(name);
    buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
    buf.extend_from_slice(value);
}

/// Serialize one value with its attribute name into `buf`.
/// `attr_name` is empty (b"") for additional set-of values and for collection members.
/// Handles Collection recursively (begCollection + memberAttrName* + endCollection).
fn serialize_value(buf: &mut Vec<u8>, attr_name: &[u8], value: &IppValue) {
    match value {
        IppValue::Collection(members) => {
            // begCollection: tag=0x34, attr_name, empty value
            buf.push(0x34);
            write_name_value(buf, attr_name, b"");

            for (member_name, member_value) in members {
                // memberAttrName: tag=0x4A, empty name, member_name as the *value* field
                buf.push(0x4A);
                write_name_value(buf, b"", member_name.as_bytes());
                // actual member value (recursive for nested collections)
                serialize_value(buf, b"", member_value);
            }

            // endCollection: tag=0x37, empty name, empty value
            buf.push(0x37);
            write_name_value(buf, b"", b"");
        }
        _ => {
            buf.push(value.value_tag());
            buf.extend_from_slice(&(attr_name.len() as u16).to_be_bytes());
            buf.extend_from_slice(attr_name);
            let vb = value.serialized_value();
            buf.extend_from_slice(&(vb.len() as u16).to_be_bytes());
            buf.extend_from_slice(&vb);
        }
    }
}

/// Serialize an IppResponse to bytes
pub fn serialize_response(resp: &IppResponse) -> Vec<u8> {
    let mut buf = vec![];

    // Version
    buf.push(resp.version.major);
    buf.push(resp.version.minor);

    // Status code
    let status: u16 = resp.status_code as u16;
    buf.extend_from_slice(&status.to_be_bytes());

    // Request ID
    buf.extend_from_slice(&resp.request_id.to_be_bytes());

    // Attribute groups
    for group in &resp.attribute_groups {
        buf.push(group.delimiter as u8);

        for attr in &group.attributes {
            for (i, value) in attr.values.iter().enumerate() {
                let name = if i == 0 { attr.name.as_bytes() } else { b"" };
                serialize_value(&mut buf, name, value);
            }
        }
    }

    // End-of-attributes
    buf.push(0x03);

    buf
}

/// Helper: build a minimal operation-attributes group for responses
pub fn standard_operation_attrs(request_id: u32) -> IppAttributeGroup {
    let _ = request_id;
    let mut group = IppAttributeGroup::new(DelimiterTag::OperationAttributes);
    group.add(IppAttribute::new("attributes-charset", IppValue::Charset("utf-8".to_string())));
    group.add(IppAttribute::new("attributes-natural-language", IppValue::NaturalLanguage("en".to_string())));
    group
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipp::parser::parse_ipp_request;

    #[test]
    fn test_serialize_deserialize_round_trip() {
        // Build a response
        let mut op_group = IppAttributeGroup::new(DelimiterTag::OperationAttributes);
        op_group.add(IppAttribute::new("attributes-charset", IppValue::Charset("utf-8".to_string())));
        op_group.add(IppAttribute::new("attributes-natural-language", IppValue::NaturalLanguage("en".to_string())));

        let mut printer_group = IppAttributeGroup::new(DelimiterTag::PrinterAttributes);
        printer_group.add(IppAttribute::new("printer-name", IppValue::NameWithoutLanguage("InkPrint".to_string())));
        printer_group.add(IppAttribute::new("printer-state", IppValue::Enum(3))); // idle

        let resp = IppResponseBuilder::new(IppStatusCode::SuccessfulOk, 42)
            .add_group(op_group)
            .add_group(printer_group)
            .build();

        let serialized = serialize_response(&resp);

        // Parse the serialized bytes as if it were a request (same format for header+attrs)
        let parsed = parse_ipp_request(&serialized).expect("should parse");

        assert_eq!(parsed.version, IppVersion { major: 2, minor: 0 });
        // status_code is in the same field position as operation_id for requests
        assert_eq!(u16::from(parsed.operation_id), 0x0000u16); // SuccessfulOk
        assert_eq!(parsed.request_id, 42);

        let op = &parsed.attribute_groups[0];
        assert_eq!(*op.get("attributes-charset").unwrap(), IppValue::Charset("utf-8".to_string()));

        let printer = &parsed.attribute_groups[1];
        assert_eq!(*printer.get("printer-name").unwrap(), IppValue::NameWithoutLanguage("InkPrint".to_string()));
        assert_eq!(*printer.get("printer-state").unwrap(), IppValue::Enum(3));
    }

    #[test]
    fn test_multi_value_attribute() {
        let mut group = IppAttributeGroup::new(DelimiterTag::PrinterAttributes);
        group.add(IppAttribute::new_multi(
            "document-format-supported",
            vec![
                IppValue::MimeMediaType("application/pdf".to_string()),
                IppValue::MimeMediaType("application/octet-stream".to_string()),
            ],
        ));

        let resp = IppResponseBuilder::new(IppStatusCode::SuccessfulOk, 1)
            .add_group(group)
            .build();

        let serialized = serialize_response(&resp);
        let parsed = parse_ipp_request(&serialized).unwrap();

        let attr = parsed.attribute_groups[0].attributes.iter()
            .find(|a| a.name == "document-format-supported")
            .unwrap();
        assert_eq!(attr.values.len(), 2);
        assert_eq!(attr.values[0], IppValue::MimeMediaType("application/pdf".to_string()));
        assert_eq!(attr.values[1], IppValue::MimeMediaType("application/octet-stream".to_string()));
    }
}
