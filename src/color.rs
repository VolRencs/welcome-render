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

        let component = |text: &str| -> RenderResult<u8> {
            u8::from_str_radix(text, 16)
                .map_err(|_| RenderError::invalid(format!("invalid color component {text:?}")))
        };

        match hex.len() {
            3 => {
                let r = component(&hex[0..1])?;
                let g = component(&hex[1..2])?;
                let b = component(&hex[2..3])?;
                Ok(Self {
                    r: r * 17,
                    g: g * 17,
                    b: b * 17,
                    a: 255,
                })
            }
            4 => {
                let r = component(&hex[0..1])?;
                let g = component(&hex[1..2])?;
                let b = component(&hex[2..3])?;
                let a = component(&hex[3..4])?;
                Ok(Self {
                    r: r * 17,
                    g: g * 17,
                    b: b * 17,
                    a: a * 17,
                })
            }
            6 => Ok(Self {
                r: component(&hex[0..2])?,
                g: component(&hex[2..4])?,
                b: component(&hex[4..6])?,
                a: 255,
            }),
            8 => Ok(Self {
                r: component(&hex[0..2])?,
                g: component(&hex[2..4])?,
                b: component(&hex[4..6])?,
                a: component(&hex[6..8])?,
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
    }
}
