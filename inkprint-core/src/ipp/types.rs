use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct IppVersion {
    pub major: u8,
    pub minor: u8,
}

impl IppVersion {
    pub const IPP_1_1: IppVersion = IppVersion { major: 1, minor: 1 };
    pub const IPP_2_0: IppVersion = IppVersion { major: 2, minor: 0 };
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u16)]
pub enum IppOperationId {
    PrintJob = 0x0002,
    PrintUri = 0x0003,
    ValidateJob = 0x0004,
    CreateJob = 0x0005,
    SendDocument = 0x0006,
    CancelJob = 0x0008,
    GetJobAttributes = 0x0009,
    GetJobs = 0x000A,
    GetPrinterAttributes = 0x000B,
    HoldJob = 0x000C,
    ReleaseJob = 0x000D,
    RestartJob = 0x000E,
    PausePrinter = 0x0010,
    ResumePrinter = 0x0011,
    PurgeJobs = 0x0012,
    Unknown(u16),
}

impl From<u16> for IppOperationId {
    fn from(v: u16) -> Self {
        match v {
            0x0002 => Self::PrintJob,
            0x0003 => Self::PrintUri,
            0x0004 => Self::ValidateJob,
            0x0005 => Self::CreateJob,
            0x0006 => Self::SendDocument,
            0x0008 => Self::CancelJob,
            0x0009 => Self::GetJobAttributes,
            0x000A => Self::GetJobs,
            0x000B => Self::GetPrinterAttributes,
            0x000C => Self::HoldJob,
            0x000D => Self::ReleaseJob,
            0x000E => Self::RestartJob,
            0x0010 => Self::PausePrinter,
            0x0011 => Self::ResumePrinter,
            0x0012 => Self::PurgeJobs,
            other => Self::Unknown(other),
        }
    }
}

impl From<IppOperationId> for u16 {
    fn from(op: IppOperationId) -> u16 {
        match op {
            IppOperationId::PrintJob => 0x0002,
            IppOperationId::PrintUri => 0x0003,
            IppOperationId::ValidateJob => 0x0004,
            IppOperationId::CreateJob => 0x0005,
            IppOperationId::SendDocument => 0x0006,
            IppOperationId::CancelJob => 0x0008,
            IppOperationId::GetJobAttributes => 0x0009,
            IppOperationId::GetJobs => 0x000A,
            IppOperationId::GetPrinterAttributes => 0x000B,
            IppOperationId::HoldJob => 0x000C,
            IppOperationId::ReleaseJob => 0x000D,
            IppOperationId::RestartJob => 0x000E,
            IppOperationId::PausePrinter => 0x0010,
            IppOperationId::ResumePrinter => 0x0011,
            IppOperationId::PurgeJobs => 0x0012,
            IppOperationId::Unknown(v) => v,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u16)]
pub enum IppStatusCode {
    SuccessfulOk = 0x0000,
    SuccessfulOkIgnoredOrSubstituted = 0x0001,
    SuccessfulOkConflictingAttributes = 0x0002,
    ClientErrorBadRequest = 0x0400,
    ClientErrorForbidden = 0x0401,
    ClientErrorNotAuthenticated = 0x0402,
    ClientErrorNotAuthorized = 0x0403,
    ClientErrorNotPossible = 0x0404,
    ClientErrorTimeout = 0x0405,
    ClientErrorNotFound = 0x0406,
    ClientErrorGone = 0x0407,
    ClientErrorRequestEntityTooLarge = 0x0408,
    ClientErrorRequestValueTooLong = 0x0409,
    ClientErrorDocumentFormatNotSupported = 0x040A,
    ClientErrorAttributesOrValuesNotSupported = 0x040B,
    ClientErrorUriSchemeNotSupported = 0x040C,
    ClientErrorCharsetNotSupported = 0x040D,
    ClientErrorConflictingAttributes = 0x040E,
    ServerErrorInternalError = 0x0500,
    ServerErrorOperationNotSupported = 0x0501,
    ServerErrorServiceUnavailable = 0x0502,
    ServerErrorVersionNotSupported = 0x0503,
    ServerErrorDeviceError = 0x0504,
    ServerErrorTemporaryError = 0x0505,
    ServerErrorNotAcceptingJobs = 0x0506,
    ServerErrorBusy = 0x0507,
    ServerErrorJobCanceled = 0x0508,
    ServerErrorMultipleDocumentJobsNotSupported = 0x0509,
}

impl From<IppStatusCode> for u16 {
    fn from(s: IppStatusCode) -> u16 {
        s as u16
    }
}

/// IPP attribute delimiter tags (group delimiters)
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum DelimiterTag {
    OperationAttributes = 0x01,
    JobAttributes = 0x02,
    EndOfAttributes = 0x03,
    PrinterAttributes = 0x04,
    UnsupportedAttributes = 0x05,
}

impl TryFrom<u8> for DelimiterTag {
    type Error = IppError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0x01 => Ok(Self::OperationAttributes),
            0x02 => Ok(Self::JobAttributes),
            0x03 => Ok(Self::EndOfAttributes),
            0x04 => Ok(Self::PrinterAttributes),
            0x05 => Ok(Self::UnsupportedAttributes),
            other => Err(IppError::UnknownDelimiter(other)),
        }
    }
}

/// IPP value tags
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum ValueTag {
    // Out-of-band
    Unsupported = 0x10,
    Unknown = 0x12,
    NoValue = 0x13,
    // Integer types
    Integer = 0x21,
    Boolean = 0x22,
    Enum = 0x23,
    // Octet-string types
    OctetStringUnspecified = 0x30,
    DateTime = 0x31,
    Resolution = 0x32,
    RangeOfInteger = 0x33,
    BegCollection = 0x34,
    TextWithLanguage = 0x35,
    NameWithLanguage = 0x36,
    EndCollection = 0x37,
    // Character-string types
    TextWithoutLanguage = 0x41,
    NameWithoutLanguage = 0x42,
    Keyword = 0x44,
    Uri = 0x45,
    UriScheme = 0x46,
    Charset = 0x47,
    NaturalLanguage = 0x48,
    MimeMediaType = 0x49,
    MemberAttrName = 0x4A,
}

impl TryFrom<u8> for ValueTag {
    type Error = IppError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0x10 => Ok(Self::Unsupported),
            0x12 => Ok(Self::Unknown),
            0x13 => Ok(Self::NoValue),
            0x21 => Ok(Self::Integer),
            0x22 => Ok(Self::Boolean),
            0x23 => Ok(Self::Enum),
            0x30 => Ok(Self::OctetStringUnspecified),
            0x31 => Ok(Self::DateTime),
            0x32 => Ok(Self::Resolution),
            0x33 => Ok(Self::RangeOfInteger),
            0x34 => Ok(Self::BegCollection),
            0x35 => Ok(Self::TextWithLanguage),
            0x36 => Ok(Self::NameWithLanguage),
            0x37 => Ok(Self::EndCollection),
            0x41 => Ok(Self::TextWithoutLanguage),
            0x42 => Ok(Self::NameWithoutLanguage),
            0x44 => Ok(Self::Keyword),
            0x45 => Ok(Self::Uri),
            0x46 => Ok(Self::UriScheme),
            0x47 => Ok(Self::Charset),
            0x48 => Ok(Self::NaturalLanguage),
            0x49 => Ok(Self::MimeMediaType),
            0x4A => Ok(Self::MemberAttrName),
            other => Err(IppError::UnknownValueTag(other)),
        }
    }
}

/// IPP attribute value
#[derive(Debug, Clone, PartialEq)]
pub enum IppValue {
    Integer(i32),
    Boolean(bool),
    Enum(i32),
    TextWithoutLanguage(String),
    NameWithoutLanguage(String),
    Keyword(String),
    Uri(String),
    UriScheme(String),
    Charset(String),
    NaturalLanguage(String),
    MimeMediaType(String),
    OctetString(Vec<u8>),
    DateTime {
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minutes: u8,
        seconds: u8,
        deci_seconds: u8,
        direction_from_utc: u8,
        hours_from_utc: u8,
        minutes_from_utc: u8,
    },
    Resolution {
        cross_feed: i32,
        feed: i32,
        units: u8,
    },
    RangeOfInteger {
        lower: i32,
        upper: i32,
    },
    /// IPP collection (begCollection/memberAttrName/endCollection).
    /// Each element is (member_name, member_value); nested collections are supported.
    Collection(Vec<(String, IppValue)>),
    NoValue,
    Unsupported,
    Unknown(Vec<u8>),
}

impl IppValue {
    pub fn value_tag(&self) -> u8 {
        match self {
            Self::Integer(_) => 0x21,
            Self::Boolean(_) => 0x22,
            Self::Enum(_) => 0x23,
            Self::TextWithoutLanguage(_) => 0x41,
            Self::NameWithoutLanguage(_) => 0x42,
            Self::Keyword(_) => 0x44,
            Self::Uri(_) => 0x45,
            Self::UriScheme(_) => 0x46,
            Self::Charset(_) => 0x47,
            Self::NaturalLanguage(_) => 0x48,
            Self::MimeMediaType(_) => 0x49,
            Self::OctetString(_) => 0x30,
            Self::DateTime { .. } => 0x31,
            Self::Resolution { .. } => 0x32,
            Self::RangeOfInteger { .. } => 0x33,
            Self::Collection(_) => 0x34, // begCollection
            Self::NoValue => 0x13,
            Self::Unsupported => 0x10,
            Self::Unknown(_) => 0x12,
        }
    }

    pub fn serialized_value(&self) -> Vec<u8> {
        match self {
            Self::Integer(v) | Self::Enum(v) => v.to_be_bytes().to_vec(),
            Self::Boolean(v) => vec![if *v { 1 } else { 0 }],
            Self::TextWithoutLanguage(s)
            | Self::NameWithoutLanguage(s)
            | Self::Keyword(s)
            | Self::Uri(s)
            | Self::UriScheme(s)
            | Self::Charset(s)
            | Self::NaturalLanguage(s)
            | Self::MimeMediaType(s) => s.as_bytes().to_vec(),
            Self::OctetString(v) | Self::Unknown(v) => v.clone(),
            Self::DateTime {
                year, month, day, hour, minutes, seconds,
                deci_seconds, direction_from_utc, hours_from_utc, minutes_from_utc,
            } => {
                let mut v = vec![];
                v.extend_from_slice(&year.to_be_bytes());
                v.push(*month);
                v.push(*day);
                v.push(*hour);
                v.push(*minutes);
                v.push(*seconds);
                v.push(*deci_seconds);
                v.push(*direction_from_utc);
                v.push(*hours_from_utc);
                v.push(*minutes_from_utc);
                v
            }
            Self::Resolution { cross_feed, feed, units } => {
                let mut v = vec![];
                v.extend_from_slice(&cross_feed.to_be_bytes());
                v.extend_from_slice(&feed.to_be_bytes());
                v.push(*units);
                v
            }
            Self::RangeOfInteger { lower, upper } => {
                let mut v = vec![];
                v.extend_from_slice(&lower.to_be_bytes());
                v.extend_from_slice(&upper.to_be_bytes());
                v
            }
            // Collection encoding is handled by the serializer; serialized_value() is unused.
            Self::Collection(_) => vec![],
            Self::NoValue | Self::Unsupported => vec![],
        }
    }
}

/// A single IPP attribute (name + one or more values)
#[derive(Debug, Clone, PartialEq)]
pub struct IppAttribute {
    pub name: String,
    pub values: Vec<IppValue>,
}

impl IppAttribute {
    pub fn new(name: impl Into<String>, value: IppValue) -> Self {
        Self { name: name.into(), values: vec![value] }
    }

    pub fn new_multi(name: impl Into<String>, values: Vec<IppValue>) -> Self {
        Self { name: name.into(), values }
    }
}

/// A group of attributes with the same delimiter tag
#[derive(Debug, Clone, PartialEq)]
pub struct IppAttributeGroup {
    pub delimiter: DelimiterTag,
    pub attributes: Vec<IppAttribute>,
}

impl IppAttributeGroup {
    pub fn new(delimiter: DelimiterTag) -> Self {
        Self { delimiter, attributes: vec![] }
    }

    pub fn add(&mut self, attr: IppAttribute) {
        self.attributes.push(attr);
    }

    pub fn get(&self, name: &str) -> Option<&IppValue> {
        self.attributes.iter()
            .find(|a| a.name == name)
            .and_then(|a| a.values.first())
    }
}

/// A parsed IPP request
#[derive(Debug, Clone)]
pub struct IppRequest {
    pub version: IppVersion,
    pub operation_id: IppOperationId,
    pub request_id: u32,
    pub attribute_groups: Vec<IppAttributeGroup>,
    pub document_data: Vec<u8>,
}

impl IppRequest {
    pub fn get_operation_attributes(&self) -> Option<&IppAttributeGroup> {
        self.attribute_groups.iter()
            .find(|g| g.delimiter == DelimiterTag::OperationAttributes)
    }

    pub fn get_job_attributes(&self) -> Option<&IppAttributeGroup> {
        self.attribute_groups.iter()
            .find(|g| g.delimiter == DelimiterTag::JobAttributes)
    }

    pub fn get_attr_str<'a>(&'a self, group: DelimiterTag, name: &str) -> Option<&'a str> {
        self.attribute_groups.iter()
            .find(|g| g.delimiter == group)
            .and_then(|g| g.get(name))
            .and_then(|v| match v {
                IppValue::TextWithoutLanguage(s) |
                IppValue::NameWithoutLanguage(s) |
                IppValue::Keyword(s) |
                IppValue::Uri(s) |
                IppValue::Charset(s) |
                IppValue::MimeMediaType(s) |
                IppValue::NaturalLanguage(s) => Some(s.as_str()),
                _ => None,
            })
    }
}

/// A built IPP response
#[derive(Debug, Clone)]
pub struct IppResponse {
    pub version: IppVersion,
    pub status_code: IppStatusCode,
    pub request_id: u32,
    pub attribute_groups: Vec<IppAttributeGroup>,
}

#[derive(Debug, Error)]
pub enum IppError {
    #[error("Unexpected end of data (need {need} bytes, have {have})")]
    UnexpectedEof { need: usize, have: usize },
    #[error("Unknown delimiter tag: 0x{0:02x}")]
    UnknownDelimiter(u8),
    #[error("Unknown value tag: 0x{0:02x}")]
    UnknownValueTag(u8),
    #[error("Invalid UTF-8 string in attribute")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
    #[error("Invalid IPP version: {major}.{minor}")]
    InvalidVersion { major: u8, minor: u8 },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
