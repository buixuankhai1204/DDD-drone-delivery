use crate::{error::DomainError, value_objects::non_blank};

const EARTH_RADIUS_M: f64 = 6_371_008.8;
const MAX_ADDRESS_LEN: usize = 255;

/// A WGS-84 coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    latitude: f64,
    longitude: f64,
}

impl GeoPoint {
    pub fn new(latitude: f64, longitude: f64) -> Result<Self, DomainError> {
        let valid = latitude.is_finite()
            && longitude.is_finite()
            && (-90.0..=90.0).contains(&latitude)
            && (-180.0..=180.0).contains(&longitude);
        if !valid {
            return Err(DomainError::InvalidCoordinates);
        }
        Ok(Self {
            latitude,
            longitude,
        })
    }

    pub const fn latitude(&self) -> f64 {
        self.latitude
    }

    pub const fn longitude(&self) -> f64 {
        self.longitude
    }

    /// Great-circle distance in metres (haversine formula).
    pub fn distance_to(&self, other: &GeoPoint) -> u32 {
        let (lat1, lat2) = (self.latitude.to_radians(), other.latitude.to_radians());
        let delta_lat = lat2 - lat1;
        let delta_lon = (other.longitude - self.longitude).to_radians();

        let a = (delta_lat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
        let central_angle = 2.0 * a.sqrt().min(1.0).asin();

        (EARTH_RADIUS_M * central_angle).round() as u32
    }
}

/// A place a drone can land: coordinates plus a human readable address.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    coordinates: GeoPoint,
    address: String,
}

impl Location {
    pub fn new(coordinates: GeoPoint, address: impl AsRef<str>) -> Result<Self, DomainError> {
        Ok(Self {
            coordinates,
            address: non_blank("address", address, MAX_ADDRESS_LEN)?,
        })
    }

    pub const fn coordinates(&self) -> &GeoPoint {
        &self.coordinates
    }

    pub fn address(&self) -> &str {
        &self.address
    }
}
