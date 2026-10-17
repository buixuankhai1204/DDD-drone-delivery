use crate::{error::DomainError, value_objects::non_blank};

/// Heaviest parcel any drone in the fleet class can lift.
pub const MAX_PARCEL_WEIGHT_GRAMS: u32 = 5_000;
/// Longest side that still fits the drone cargo bay.
pub const MAX_PARCEL_DIMENSION_MM: u32 = 500;
const MAX_DESCRIPTION_LEN: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dimensions {
    length_mm: u32,
    width_mm: u32,
    height_mm: u32,
}

impl Dimensions {
    pub fn new(length_mm: u32, width_mm: u32, height_mm: u32) -> Result<Self, DomainError> {
        let fits = |side: u32| (1..=MAX_PARCEL_DIMENSION_MM).contains(&side);
        if !(fits(length_mm) && fits(width_mm) && fits(height_mm)) {
            return Err(DomainError::InvalidParcelDimensions {
                max_mm: MAX_PARCEL_DIMENSION_MM,
            });
        }
        Ok(Self {
            length_mm,
            width_mm,
            height_mm,
        })
    }

    pub const fn length_mm(&self) -> u32 {
        self.length_mm
    }

    pub const fn width_mm(&self) -> u32 {
        self.width_mm
    }

    pub const fn height_mm(&self) -> u32 {
        self.height_mm
    }
}

/// The goods being shipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parcel {
    weight_grams: u32,
    dimensions: Dimensions,
    description: String,
    fragile: bool,
}

impl Parcel {
    pub fn new(
        weight_grams: u32,
        dimensions: Dimensions,
        description: impl AsRef<str>,
        fragile: bool,
    ) -> Result<Self, DomainError> {
        if !(1..=MAX_PARCEL_WEIGHT_GRAMS).contains(&weight_grams) {
            return Err(DomainError::InvalidParcelWeight {
                max_grams: MAX_PARCEL_WEIGHT_GRAMS,
            });
        }
        Ok(Self {
            weight_grams,
            dimensions,
            description: non_blank("parcel description", description, MAX_DESCRIPTION_LEN)?,
            fragile,
        })
    }

    pub const fn weight_grams(&self) -> u32 {
        self.weight_grams
    }

    pub const fn dimensions(&self) -> &Dimensions {
        &self.dimensions
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub const fn is_fragile(&self) -> bool {
        self.fragile
    }
}
