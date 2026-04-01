use super::types::*;

pub struct IppParser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> IppParser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn read_u8(&mut self) -> Result<u8, IppError> {
        if self.remaining() < 1 {
            return Err(IppError::UnexpectedEof { need: 1, have: self.remaining() });
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn read_u16(&mut self) -> Result<u16, IppError> {
        if self.remaining() < 2 {
            return Err(IppError::UnexpectedEof { need: 2, have: self.remaining() });
        }
        let v = u16::from_be_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn read_u32(&mut self) -> Result<u32, IppError> {
        if self.remaining() < 4 {
            return Err(IppError::UnexpectedEof { need: 4, have: self.remaining() });
        }
        let v = u32::from_be_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], IppError> {
        if self.remaining() < n {
            return Err(IppError::UnexpectedEof { need: n, have: self.remaining() });
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn peek_u8(&self) -> Result<u8, IppError> {
        if self.remaining() < 1 {
            return Err(IppError::UnexpectedEof { need: 1, have: 0 });
        }
        Ok(self.data[self.pos])
    }

    fn parse_value(&mut self, tag: u8, value_bytes: &[u8]) -> Result<IppValue, IppError> {
        match tag {
            0x21 => {
                // Integer (4 bytes big-endian signed)
                if value_bytes.len() < 4 {
                    return Err(IppError::UnexpectedEof { need: 4, have: value_bytes.len() });
                }
                Ok(IppValue::Integer(i32::from_be_bytes([
                    value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3],
                ])))
            }
            0x22 => {
                // Boolean (1 byte)
                Ok(IppValue::Boolean(value_bytes.first().copied().unwrap_or(0) != 0))
            }
            0x23 => {
                // Enum (4 bytes big-endian signed)
                if value_bytes.len() < 4 {
                    return Err(IppError::UnexpectedEof { need: 4, have: value_bytes.len() });
                }
                Ok(IppValue::Enum(i32::from_be_bytes([
                    value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3],
                ])))
            }
            0x41 => Ok(IppValue::TextWithoutLanguage(String::from_utf8(value_bytes.to_vec())?)),
            0x42 => Ok(IppValue::NameWithoutLanguage(String::from_utf8(value_bytes.to_vec())?)),
            0x44 => Ok(IppValue::Keyword(String::from_utf8(value_bytes.to_vec())?)),
            0x45 => Ok(IppValue::Uri(String::from_utf8(value_bytes.to_vec())?)),
            0x46 => Ok(IppValue::UriScheme(String::from_utf8(value_bytes.to_vec())?)),
            0x47 => Ok(IppValue::Charset(String::from_utf8(value_bytes.to_vec())?)),
            0x48 => Ok(IppValue::NaturalLanguage(String::from_utf8(value_bytes.to_vec())?)),
            0x49 => Ok(IppValue::MimeMediaType(String::from_utf8(value_bytes.to_vec())?)),
            0x30 => Ok(IppValue::OctetString(value_bytes.to_vec())),
            0x31 => {
                // DateTime: year(2) month(1) day(1) hour(1) min(1) sec(1) dsec(1) dir(1) utch(1) utcm(1)
                if value_bytes.len() < 11 {
                    return Err(IppError::UnexpectedEof { need: 11, have: value_bytes.len() });
                }
                Ok(IppValue::DateTime {
                    year: u16::from_be_bytes([value_bytes[0], value_bytes[1]]),
                    month: value_bytes[2],
                    day: value_bytes[3],
                    hour: value_bytes[4],
                    minutes: value_bytes[5],
                    seconds: value_bytes[6],
                    deci_seconds: value_bytes[7],
                    direction_from_utc: value_bytes[8],
                    hours_from_utc: value_bytes[9],
                    minutes_from_utc: value_bytes[10],
                })
            }
            0x32 => {
                // Resolution: cross_feed(4) feed(4) units(1)
                if value_bytes.len() < 9 {
                    return Err(IppError::UnexpectedEof { need: 9, have: value_bytes.len() });
                }
                Ok(IppValue::Resolution {
                    cross_feed: i32::from_be_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]]),
                    feed: i32::from_be_bytes([value_bytes[4], value_bytes[5], value_bytes[6], value_bytes[7]]),
                    units: value_bytes[8],
                })
            }
            0x33 => {
                // RangeOfInteger: lower(4) upper(4)
                if value_bytes.len() < 8 {
                    return Err(IppError::UnexpectedEof { need: 8, have: value_bytes.len() });
                }
                Ok(IppValue::RangeOfInteger {
                    lower: i32::from_be_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]]),
                    upper: i32::from_be_bytes([value_bytes[4], value_bytes[5], value_bytes[6], value_bytes[7]]),
                })
            }
            0x13 => Ok(IppValue::NoValue),
            0x10 => Ok(IppValue::Unsupported),
            _ => Ok(IppValue::Unknown(value_bytes.to_vec())),
        }
    }

    fn parse_attribute_group(&mut self, delimiter: DelimiterTag) -> Result<IppAttributeGroup, IppError> {
        let mut group = IppAttributeGroup::new(delimiter);

        loop {
            // Peek at the next tag
            let tag = self.peek_u8()?;

            // If it's a delimiter tag (0x01-0x05), the next group starts (or end-of-attrs)
            if tag <= 0x0F {
                break;
            }

            // Consume the tag
            self.read_u8()?;

            // Name length
            let name_len = self.read_u16()? as usize;
            let name_bytes = self.read_bytes(name_len)?;
            let name = String::from_utf8(name_bytes.to_vec())?;

            // Value length + value
            let value_len = self.read_u16()? as usize;
            let value_bytes = self.read_bytes(value_len)?.to_vec();
            let value = self.parse_value(tag, &value_bytes)?;

            if name.is_empty() {
                // Additional value for the previous attribute (1setOf)
                if let Some(last) = group.attributes.last_mut() {
                    last.values.push(value);
                }
            } else {
                group.attributes.push(IppAttribute { name, values: vec![value] });
            }
        }

        Ok(group)
    }

    pub fn parse(mut self) -> Result<IppRequest, IppError> {
        // Header: version(2) + operation-id(2) + request-id(4) = 8 bytes
        let major = self.read_u8()?;
        let minor = self.read_u8()?;
        let version = IppVersion { major, minor };

        let operation_raw = self.read_u16()?;
        let operation_id = IppOperationId::from(operation_raw);

        let request_id = self.read_u32()?;

        // Parse attribute groups
        let mut attribute_groups = vec![];

        loop {
            let tag = self.peek_u8()?;
            match tag {
                0x03 => {
                    // end-of-attributes-tag — consume and stop
                    self.read_u8()?;
                    break;
                }
                0x01..=0x05 => {
                    self.read_u8()?; // consume the delimiter
                    let delimiter = DelimiterTag::try_from(tag)?;
                    let group = self.parse_attribute_group(delimiter)?;
                    attribute_groups.push(group);
                }
                _ => {
                    // Treat unexpected byte as end of attribute section
                    break;
                }
            }
        }

        // Remaining bytes are document data
        let document_data = self.data[self.pos..].to_vec();

        Ok(IppRequest {
            version,
            operation_id,
            request_id,
            attribute_groups,
            document_data,
        })
    }
}

pub fn parse_ipp_request(data: &[u8]) -> Result<IppRequest, IppError> {
    IppParser::new(data).parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a raw Get-Printer-Attributes IPP request
    fn build_get_printer_attributes_raw() -> Vec<u8> {
        let mut buf = vec![];
        // version 1.1
        buf.push(1);
        buf.push(1);
        // operation-id: Get-Printer-Attributes = 0x000B
        buf.extend_from_slice(&0x000Bu16.to_be_bytes());
        // request-id: 42
        buf.extend_from_slice(&42u32.to_be_bytes());

        // operation-attributes group tag
        buf.push(0x01);

        // attributes-charset: utf-8
        buf.push(0x47); // charset tag
        let name = b"attributes-charset";
        buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
        buf.extend_from_slice(name);
        let value = b"utf-8";
        buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
        buf.extend_from_slice(value);

        // attributes-natural-language: en
        buf.push(0x48); // natural-language tag
        let name = b"attributes-natural-language";
        buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
        buf.extend_from_slice(name);
        let value = b"en";
        buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
        buf.extend_from_slice(value);

        // printer-uri
        buf.push(0x45); // uri tag
        let name = b"printer-uri";
        buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
        buf.extend_from_slice(name);
        let value = b"ipp://192.168.1.100:631/ipp/print";
        buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
        buf.extend_from_slice(value);

        // end-of-attributes
        buf.push(0x03);

        buf
    }

    #[test]
    fn test_parse_get_printer_attributes() {
        let raw = build_get_printer_attributes_raw();
        let req = parse_ipp_request(&raw).expect("should parse successfully");

        assert_eq!(req.version, IppVersion { major: 1, minor: 1 });
        assert_eq!(req.operation_id, IppOperationId::GetPrinterAttributes);
        assert_eq!(req.request_id, 42);
        assert_eq!(req.attribute_groups.len(), 1);
        assert!(req.document_data.is_empty());

        let op_group = &req.attribute_groups[0];
        assert_eq!(op_group.delimiter, DelimiterTag::OperationAttributes);

        let charset = op_group.get("attributes-charset").unwrap();
        assert_eq!(*charset, IppValue::Charset("utf-8".to_string()));

        let lang = op_group.get("attributes-natural-language").unwrap();
        assert_eq!(*lang, IppValue::NaturalLanguage("en".to_string()));

        let uri = op_group.get("printer-uri").unwrap();
        assert_eq!(*uri, IppValue::Uri("ipp://192.168.1.100:631/ipp/print".to_string()));
    }

    #[test]
    fn test_parse_with_document_data() {
        let mut raw = build_get_printer_attributes_raw();
        // Append fake document data after end-of-attributes
        raw.extend_from_slice(b"%PDF-1.4 fake document");

        let req = parse_ipp_request(&raw).expect("should parse");
        assert_eq!(req.document_data, b"%PDF-1.4 fake document");
    }

    #[test]
    fn test_parse_integer_and_boolean() {
        let mut buf = vec![];
        buf.push(1); buf.push(1); // version 1.1
        buf.extend_from_slice(&0x000Bu16.to_be_bytes()); // GetPrinterAttributes
        buf.extend_from_slice(&1u32.to_be_bytes()); // request-id

        // operation-attributes group
        buf.push(0x01);

        // integer attribute
        buf.push(0x21); // Integer tag
        let name = b"test-int";
        buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
        buf.extend_from_slice(name);
        buf.extend_from_slice(&4u16.to_be_bytes()); // value length
        buf.extend_from_slice(&42i32.to_be_bytes()); // value

        // boolean attribute
        buf.push(0x22); // Boolean tag
        let name = b"test-bool";
        buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
        buf.extend_from_slice(name);
        buf.extend_from_slice(&1u16.to_be_bytes()); // value length
        buf.push(1u8); // true

        buf.push(0x03); // end-of-attributes

        let req = parse_ipp_request(&buf).unwrap();
        let g = &req.attribute_groups[0];
        assert_eq!(*g.get("test-int").unwrap(), IppValue::Integer(42));
        assert_eq!(*g.get("test-bool").unwrap(), IppValue::Boolean(true));
    }
}
