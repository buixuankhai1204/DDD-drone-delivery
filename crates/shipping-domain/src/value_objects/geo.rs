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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_out_of_range_and_non_finite_coordinates() {
        assert!(GeoPoint::new(90.1, 0.0).is_err());
        assert!(GeoPoint::new(0.0, -180.5).is_err());
        assert!(GeoPoint::new(f64::NAN, 0.0).is_err());
        assert!(GeoPoint::new(21.03, 105.85).is_ok());
    }

    #[test]
    fn haversine_distance_is_plausible() {
        // Hanoi Opera House -> Hoan Kiem Lake (~ 600 m apart).
        let a = GeoPoint::new(21.0245, 105.8576).unwrap();
        let b = GeoPoint::new(21.0287, 105.8522).unwrap();
        let d = a.distance_to(&b);
        assert!((500..800).contains(&d), "unexpected distance {d}");
        assert_eq!(a.distance_to(&a), 0);
    }

    #[test]
    fn address_is_trimmed_and_required() {
        let p = GeoPoint::new(0.0, 0.0).unwrap();
        assert_eq!(
            Location::new(p, "  1 Main St ").unwrap().address(),
            "1 Main St"
        );
        assert_eq!(
            Location::new(p, "   ").unwrap_err(),
            DomainError::BlankField("address")
        );
    }
}
