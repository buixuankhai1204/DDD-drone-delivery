use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use shipping_application::ports::{GatewayError, RoutePlanner};
use shipping_domain::{DroneId, GeoPoint, RoutePlan};
use uuid::Uuid;

use super::{ensure_success, invalid_response, trim_base_url};

const SERVICE: &str = "flight-planning";

/// REST client of the Flight Planning context.
///
/// Assumed contract: `POST /api/v1/routes/plan` with
/// `{"drone_id", "pickup": {"latitude","longitude"}, "dropoff": {...}}`
/// answers `200 {"distance_m", "duration_secs"}`, or `422` when no flyable
/// route exists.
pub struct HttpRoutePlanner {
    client: reqwest::Client,
    base_url: String,
}

impl HttpRoutePlanner {
    pub fn new(client: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: trim_base_url(base_url),
        }
    }
}

#[derive(Serialize)]
struct PlanRouteRequest {
    drone_id: Uuid,
    pickup: PointDto,
    dropoff: PointDto,
}

#[derive(Serialize)]
struct PointDto {
    latitude: f64,
    longitude: f64,
}

impl From<&GeoPoint> for PointDto {
    fn from(point: &GeoPoint) -> Self {
        Self {
            latitude: point.latitude(),
            longitude: point.longitude(),
        }
    }
}

#[derive(Deserialize)]
struct PlanRouteResponse {
    distance_m: u32,
    duration_secs: u32,
}

#[async_trait]
impl RoutePlanner for HttpRoutePlanner {
    async fn plan_route(
        &self,
        drone_id: DroneId,
        pickup: &GeoPoint,
        dropoff: &GeoPoint,
    ) -> Result<RoutePlan, GatewayError> {
        let response = self
            .client
            .post(format!("{}/api/v1/routes/plan", self.base_url))
            .json(&PlanRouteRequest {
                drone_id: drone_id.as_uuid(),
                pickup: pickup.into(),
                dropoff: dropoff.into(),
            })
            .send()
            .await;

        let body: PlanRouteResponse = ensure_success(SERVICE, response)
            .await?
            .json()
            .await
            .map_err(|e| invalid_response(SERVICE, e))?;

        RoutePlan::new(body.distance_m, body.duration_secs)
            .map_err(|e| invalid_response(SERVICE, e))
    }
}
