use async_trait::async_trait;
use reqwest::StatusCode;
use serde::Deserialize;
use shipping_application::ports::{DroneRequirements, FleetGateway, GatewayError};
use shipping_domain::{DroneId, DroneProfile, ShipmentId};
use uuid::Uuid;

use super::{ensure_success, invalid_response, trim_base_url};

const SERVICE: &str = "fleet";

/// REST client of the Fleet Management context.
///
/// Assumed contract:
///
/// | Call                                            | Success                          |
/// |-------------------------------------------------|----------------------------------|
/// | `GET /api/v1/drones/available?payload_grams&latitude&longitude&limit` | `200 {"drones":[{"id","max_payload_grams","max_range_m"}]}` |
/// | `PUT /api/v1/drones/{drone}/reservations/{shipment}` | `2xx`, `409` if the drone was taken |
/// | `DELETE /api/v1/drones/{drone}/reservations/{shipment}` | `2xx`, `404` counts as released |
pub struct HttpFleetGateway {
    client: reqwest::Client,
    base_url: String,
}

impl HttpFleetGateway {
    pub fn new(client: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: trim_base_url(base_url),
        }
    }

    fn reservation_url(&self, drone_id: DroneId, shipment_id: ShipmentId) -> String {
        format!(
            "{}/api/v1/drones/{drone_id}/reservations/{shipment_id}",
            self.base_url
        )
    }
}

#[derive(Deserialize)]
struct AvailableDronesResponse {
    drones: Vec<DroneResponse>,
}

#[derive(Deserialize)]
struct DroneResponse {
    id: Uuid,
    max_payload_grams: u32,
    max_range_m: u32,
}

impl From<DroneResponse> for DroneProfile {
    fn from(drone: DroneResponse) -> Self {
        Self {
            id: DroneId::from_uuid(drone.id),
            max_payload_grams: drone.max_payload_grams,
            max_range_m: drone.max_range_m,
        }
    }
}

#[async_trait]
impl FleetGateway for HttpFleetGateway {
    async fn find_available_drones(
        &self,
        requirements: &DroneRequirements,
        limit: usize,
    ) -> Result<Vec<DroneProfile>, GatewayError> {
        let response = self
            .client
            .get(format!("{}/api/v1/drones/available", self.base_url))
            .query(&[
                ("payload_grams", requirements.payload_grams.to_string()),
                ("latitude", requirements.pickup.latitude().to_string()),
                ("longitude", requirements.pickup.longitude().to_string()),
                ("limit", limit.to_string()),
            ])
            .send()
            .await;

        let body: AvailableDronesResponse = ensure_success(SERVICE, response)
            .await?
            .json()
            .await
            .map_err(|e| invalid_response(SERVICE, e))?;

        Ok(body.drones.into_iter().map(Into::into).collect())
    }

    async fn reserve_drone(
        &self,
        drone_id: DroneId,
        shipment_id: ShipmentId,
    ) -> Result<(), GatewayError> {
        let response = self
            .client
            .put(self.reservation_url(drone_id, shipment_id))
            .send()
            .await;
        ensure_success(SERVICE, response).await.map(|_| ())
    }

    async fn release_drone(
        &self,
        drone_id: DroneId,
        shipment_id: ShipmentId,
    ) -> Result<(), GatewayError> {
        let response = self
            .client
            .delete(self.reservation_url(drone_id, shipment_id))
            .send()
            .await;

        // Nothing reserved any more is exactly the state we want.
        if matches!(&response, Ok(r) if r.status() == StatusCode::NOT_FOUND) {
            return Ok(());
        }
        ensure_success(SERVICE, response).await.map(|_| ())
    }
}
