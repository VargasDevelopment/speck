//! Validated logical framebuffer dimensions shared by compilation and presentation.

pub const MAX_DIMENSION: u16 = 4096;

/// Logical dimensions whose construction enforces the portable framebuffer limits.
///
/// ```compile_fail,E0451
/// let resolution = speck::Resolution { width: 0, height: 180 };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    width: u16,
    height: u16,
}

impl Resolution {
    pub const DEFAULT: Self = Self {
        width: 320,
        height: 180,
    };

    pub fn new(width: u16, height: u16) -> Result<Self, String> {
        for (name, dimension) in [("width", width), ("height", height)] {
            if !(1..=MAX_DIMENSION).contains(&dimension) {
                return Err(format!(
                    "resolution {name} must be between 1 and {MAX_DIMENSION}, found {dimension}"
                ));
            }
        }
        Ok(Self { width, height })
    }

    pub const fn width(self) -> u16 {
        self.width
    }
    pub const fn height(self) -> u16 {
        self.height
    }

    /// Byte length of a tightly packed RGB8 framebuffer.
    pub const fn payload_bytes(self) -> usize {
        self.width as usize * self.height as usize * 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_and_payload_are_validated() {
        assert_eq!(Resolution::DEFAULT, Resolution::new(320, 180).unwrap());
        assert_eq!(Resolution::new(1, 1).unwrap().payload_bytes(), 3);
        assert_eq!(
            Resolution::new(4096, 4096).unwrap().payload_bytes(),
            50_331_648
        );
        for (width, height) in [(0, 1), (1, 0), (4097, 1), (1, 4097), (u16::MAX, u16::MAX)] {
            assert!(Resolution::new(width, height).is_err());
        }
    }
}
