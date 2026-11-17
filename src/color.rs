use crate::error::{RenderError, RenderResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorU8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl ColorU8 {
    pub const BASE: Self = Self {
        r: 17,
        g: 24,
        b: 39,
        a: 255,
    };

    pub const GRADIENT_END: Self = Self {
        r: 49,
        g: 46,
        b: 129,
        a: 255,
    };

    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    pub fn parse(value: &str) -> RenderResult<Self> {
        let hex = value.strip_prefix('#').ok_or_else(|| {
            RenderError::invalid(format!("color must start with '#', got {value:?}"))
        })?;
        let bytes = hex.as_bytes();

        let nibble = |byte: u8| -> RenderResult<u8> {
            match byte {
                b'0'..=b'9' => Ok(byte - b'0'),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                b'A'..=b'F' => Ok(byte - b'A' + 10),
                _ => Err(RenderError::invalid(format!(
                    "invalid color component byte 0x{byte:02x}"
                ))),
            }
        };
        let component = |index: usize| -> RenderResult<u8> {
            Ok(nibble(bytes[index])? * 16 + nibble(bytes[index + 1])?)
        };

        match bytes.len() {
            3 => Ok(Self {
                r: nibble(bytes[0])? * 17,
                g: nibble(bytes[1])? * 17,
                b: nibble(bytes[2])? * 17,
                a: 255,
            }),
            4 => Ok(Self {
                r: nibble(bytes[0])? * 17,
                g: nibble(bytes[1])? * 17,
                b: nibble(bytes[2])? * 17,
                a: nibble(bytes[3])? * 17,
            }),
            6 => Ok(Self {
                r: component(0)?,
                g: component(2)?,
                b: component(4)?,
                a: 255,
            }),
            8 => Ok(Self {
                r: component(0)?,
                g: component(2)?,
                b: component(4)?,
                a: component(6)?,
            }),
            _ => Err(RenderError::invalid(format!(
                "unsupported color format {value:?}"
            ))),
        }
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_and_long_hex() {
        assert_eq!(
            ColorU8::parse("#fff").unwrap(),
            ColorU8 {
                r: 255,
                g: 255,
                b: 255,
                a: 255
            }
        );
        assert_eq!(
            ColorU8::parse("#11223344").unwrap(),
            ColorU8 {
                r: 0x11,
                g: 0x22,
                b: 0x33,
                a: 0x44
            }
        );
    }

    #[test]
    fn rejects_bad_colors() {
        assert!(ColorU8::parse("fff").is_err());
        assert!(ColorU8::parse("#12").is_err());
        assert!(ColorU8::parse("#gggggg").is_err());
        assert!(ColorU8::parse("#éa").is_err());
        assert!(ColorU8::parse("#aé").is_err());
    }

    #[test]
    fn accepts_uppercase_hex() {
        assert_eq!(
            ColorU8::parse("#AbCdEf").unwrap(),
            ColorU8 {
                r: 0xab,
                g: 0xcd,
                b: 0xef,
                a: 255,
            }
        );
    }
}
