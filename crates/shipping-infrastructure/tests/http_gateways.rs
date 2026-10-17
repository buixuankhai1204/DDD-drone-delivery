//! The HTTP adapters against throw-away in-process servers that play the
//! Fleet and Flight Planning contexts.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use shipping_application::ports::{DroneRequirements, FleetGateway, GatewayError, RoutePlanner};
use shipping_domain::{DroneId, GeoPoint, ShipmentId};
use shipping_infrastructure::gateways::{HttpFleetGateway, HttpRoutePlanner, http_client};
use uuid::Uuid;

const TAKEN_DRONE: Uuid = Uuid::from_u128(666);
const UNKNOWN_RESERVATION: Uuid = Uuid::from_u128(404);
const NO_ROUTE_DRONE: Uuid = Uuid::from_u128(422);

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn fleet_service() -> Router {
    Router::new()
        .route(
            "/api/v1/drones/available",
            // Echoes the query back so the test can check what was sent.
            get(|Query(q): Query<HashMap<String, String>>| async move {
                let count: usize = q["limit"].parse().unwrap();
                let payload: u32 = q["payload_grams"].parse().unwrap();
                let drones: Vec<Value> = (1..=count as u128)
                    .map(|n| {
                        json!({
                            "id": Uuid::from_u128(n),
                            "max_payload_grams": payload,
                            "max_range_m": 10_000,
                        })
                    })
                    .collect();
                Json(json!({ "drones": drones }))
            }),
        )
        .route(
            "/api/v1/drones/{drone}/reservations/{shipment}",
            axum::routing::put(|Path((drone, _)): Path<(Uuid, Uuid)>| async move {
                if drone == TAKEN_DRONE {
                    (StatusCode::CONFLICT, "already reserved").into_response()
                } else {
                    StatusCode::NO_CONTENT.into_response()
                }
            })
            .delete(|Path((drone, _)): Path<(Uuid, Uuid)>| async move {
                if drone == UNKNOWN_RESERVATION {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::NO_CONTENT
                }
            }),
        )
}

fn planner_service() -> Router {
    Router::new().route(
        "/api/v1/routes/plan",
        post(|Json(body): Json<Value>| async move {
            if body["drone_id"] == json!(NO_ROUTE_DRONE) {
                return (StatusCode::UNPROCESSABLE_ENTITY, "restricted airspace").into_response();
            }
            let pickup = body["pickup"]["latitude"].as_f64().unwrap();
            assert!((pickup - 21.0285).abs() < 1e-9);
            Json(json!({ "distance_m": 3_200, "duration_secs": 420 })).into_response()
        }),
    )
}

fn broken_service() -> Router {
    Router::new().fallback(|| async { StatusCode::SERVICE_UNAVAILABLE })
}

fn point(lat: f64, lon: f64) -> GeoPoint {
    GeoPoint::new(lat, lon).unwrap()
}

fn drone(n: u128) -> DroneId {
    DroneId::from_uuid(Uuid::from_u128(n))
}

#[tokio::test]
async fn fleet_lists_available_drones_with_the_requested_filters() {
    let gateway = HttpFleetGateway::new(http_client(), serve(fleet_service()).await);

    let drones = gateway
        .find_available_drones(
            &DroneRequirements {
                payload_grams: 1_500,
                pickup: point(21.0285, 105.8542),
            },
            2,
        )
        .await
        .unwrap();

    assert_eq!(drones.len(), 2);
    assert_eq!(drones[0].id, drone(1));
    assert_eq!(drones[0].max_payload_grams, 1_500);
    assert_eq!(drones[0].max_range_m, 10_000);
}

#[tokio::test]
async fn fleet_reservation_conflict_is_a_rejection() {
    let base = serve(fleet_service()).await;
    // A trailing slash in the configuration must not break URLs.
    let gateway = HttpFleetGateway::new(http_client(), format!("{base}/"));
    let shipment = ShipmentId::new();

    gateway.reserve_drone(drone(1), shipment).await.unwrap();
    let err = gateway
        .reserve_drone(DroneId::from_uuid(TAKEN_DRONE), shipment)
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            GatewayError::Rejected {
                service: "fleet",
                ..
            }
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn releasing_an_unknown_reservation_succeeds() {
    let gateway = HttpFleetGateway::new(http_client(), serve(fleet_service()).await);
    let shipment = ShipmentId::new();

    gateway.release_drone(drone(1), shipment).await.unwrap();
    gateway
        .release_drone(DroneId::from_uuid(UNKNOWN_RESERVATION), shipment)
        .await
        .unwrap();
}

#[tokio::test]
async fn server_errors_and_connection_failures_are_unavailability() {
    let gateway = HttpFleetGateway::new(http_client(), serve(broken_service()).await);
    let err = gateway
        .reserve_drone(drone(1), ShipmentId::new())
        .await
        .unwrap_err();
    assert!(matches!(err, GatewayError::Unavailable { .. }), "{err:?}");

    // Nothing listens on port 1.
    let gateway = HttpFleetGateway::new(http_client(), "http://127.0.0.1:1");
    let err = gateway
        .find_available_drones(
            &DroneRequirements {
                payload_grams: 1,
                pickup: point(0.0, 0.0),
            },
            1,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, GatewayError::Unavailable { .. }), "{err:?}");
}

#[tokio::test]
async fn route_planner_maps_the_plan() {
    let planner = HttpRoutePlanner::new(http_client(), serve(planner_service()).await);

    let route = planner
        .plan_route(
            drone(1),
            &point(21.0285, 105.8542),
            &point(21.0380, 105.8340),
        )
        .await
        .unwrap();

    assert_eq!((route.distance_m(), route.duration_secs()), (3_200, 420));
}

#[tokio::test]
async fn route_planner_refusal_is_a_rejection_and_outage_is_unavailability() {
    let planner = HttpRoutePlanner::new(http_client(), serve(planner_service()).await);
    let err = planner
        .plan_route(
            DroneId::from_uuid(NO_ROUTE_DRONE),
            &point(21.0285, 105.8542),
            &point(21.0380, 105.8340),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(&err, GatewayError::Rejected { reason, .. } if reason.contains("restricted airspace")),
        "{err:?}"
    );

    let planner = HttpRoutePlanner::new(http_client(), serve(broken_service()).await);
    let err = planner
        .plan_route(drone(1), &point(0.0, 0.0), &point(0.1, 0.1))
        .await
        .unwrap_err();
    assert!(matches!(err, GatewayError::Unavailable { .. }), "{err:?}");
}
