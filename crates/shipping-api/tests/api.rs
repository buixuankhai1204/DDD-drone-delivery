//! End-to-end tests of the REST API: real router, real PostgreSQL, stubbed
//! Fleet and Flight Planning contexts.
//!
//! They need a database and are ignored by default:
//!
//! ```text
//! docker compose up -d db
//! DATABASE_URL=postgres://shipping:shipping@localhost:5432/shipping \
//!     cargo test -p shipping-api -- --include-ignored
//! ```

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use shipping_api::{AppState, router};
use shipping_infrastructure::gateways::{StubFleetGateway, StubRoutePlanner};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

struct Api {
    app: Router,
}

struct Reply {
    status: StatusCode,
    location: Option<String>,
    body: Value,
}

impl Api {
    fn new(pool: PgPool) -> Self {
        let state = AppState::new(
            pool,
            Arc::new(StubFleetGateway::new()),
            Arc::new(StubRoutePlanner),
        );
        Self { app: router(state) }
    }

    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> Reply {
        let mut request = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };

        let response = self
            .app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();

        let status = response.status();
        let location = response
            .headers()
            .get(header::LOCATION)
            .map(|v| v.to_str().unwrap().to_owned());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply {
            status,
            location,
            body,
        }
    }

    async fn get(&self, uri: &str) -> Reply {
        self.send(Method::GET, uri, None).await
    }

    async fn post(&self, uri: &str, body: Option<Value>) -> Reply {
        self.send(Method::POST, uri, body).await
    }

    /// Creates a shipment and returns its id.
    async fn request_shipment(&self, weight_grams: u32) -> String {
        let reply = self
            .post("/api/v1/shipments", Some(shipment_body(weight_grams)))
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        reply.body["id"].as_str().unwrap().to_owned()
    }

    async fn dispatch(&self, id: &str) -> Reply {
        self.post(&format!("/api/v1/shipments/{id}/dispatch"), None)
            .await
    }
}

fn shipment_body(weight_grams: u32) -> Value {
    json!({
        "customer_id": Uuid::now_v7(),
        "recipient": { "name": "Nguyen Van A", "phone": "+84901234567" },
        "parcel": {
            "weight_grams": weight_grams,
            "length_mm": 200,
            "width_mm": 150,
            "height_mm": 100,
            "description": "Phone case",
            "fragile": true
        },
        "pickup": { "latitude": 21.0285, "longitude": 105.8542, "address": "Hoan Kiem, Hanoi" },
        "dropoff": { "latitude": 21.0380, "longitude": 105.8340, "address": "Ba Dinh, Hanoi" }
    })
}

fn error_code(reply: &Reply) -> &str {
    reply.body["error"]["code"].as_str().unwrap_or_default()
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn customer_journey_from_request_to_delivery(pool: PgPool) {
    let api = Api::new(pool);

    // Request
    let created = api
        .post("/api/v1/shipments", Some(shipment_body(1_500)))
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let id = created.body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        created.location.as_deref(),
        Some(format!("/api/v1/shipments/{id}").as_str())
    );
    assert_eq!(created.body["status"], "requested");
    let tracking_number = created.body["tracking_number"].as_str().unwrap().to_owned();
    assert!(tracking_number.starts_with("DRN-"));

    // Dispatch
    let scheduled = api.dispatch(&id).await;
    assert_eq!(scheduled.status, StatusCode::OK, "{}", scheduled.body);
    assert_eq!(scheduled.body["status"], "scheduled");
    assert!(scheduled.body["drone_id"].is_string());
    assert!(scheduled.body["estimated_delivery_at"].is_string());
    assert!(scheduled.body["route"]["distance_m"].as_u64().unwrap() > 600);

    // Public tracking, case-insensitive, without personal data
    let tracked = api
        .get(&format!(
            "/api/v1/tracking/{}",
            tracking_number.to_lowercase()
        ))
        .await;
    assert_eq!(tracked.status, StatusCode::OK);
    assert_eq!(tracked.body["status"], "scheduled");
    assert_eq!(tracked.body["history"].as_array().unwrap().len(), 2);
    assert!(tracked.body.get("recipient").is_none());
    assert!(tracked.body.get("customer_id").is_none());

    // Drone reports progress
    let picked_up = api
        .post(&format!("/api/v1/shipments/{id}/pickup"), None)
        .await;
    assert_eq!(picked_up.body["status"], "in_transit");

    let delivered = api
        .post(&format!("/api/v1/shipments/{id}/delivery"), None)
        .await;
    assert_eq!(delivered.status, StatusCode::OK);
    assert_eq!(delivered.body["status"], "delivered");
    assert_eq!(delivered.body["history"].as_array().unwrap().len(), 4);

    // Read side
    let fetched = api.get(&format!("/api/v1/shipments/{id}")).await;
    assert_eq!(fetched.body["status"], "delivered");
    assert_eq!(fetched.body["version"], 4);

    let delivered_list = api.get("/api/v1/shipments?status=delivered").await;
    assert_eq!(delivered_list.body["total"], 1);
    assert_eq!(delivered_list.body["items"][0]["id"], id);

    let none = api.get("/api/v1/shipments?status=requested").await;
    assert_eq!(none.body["total"], 0);
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn lists_a_customers_shipments_with_paging(pool: PgPool) {
    let api = Api::new(pool);
    let customer = Uuid::now_v7();
    for _ in 0..3 {
        let mut body = shipment_body(500);
        body["customer_id"] = json!(customer);
        assert_eq!(
            api.post("/api/v1/shipments", Some(body)).await.status,
            StatusCode::CREATED
        );
    }
    api.request_shipment(500).await; // someone else's

    let page = api
        .get(&format!(
            "/api/v1/shipments?customer_id={customer}&limit=2&offset=0"
        ))
        .await;
    assert_eq!(page.body["total"], 3);
    assert_eq!(page.body["items"].as_array().unwrap().len(), 2);
    assert_eq!(page.body["limit"], 2);
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn rejects_invalid_requests_with_clear_errors(pool: PgPool) {
    let api = Api::new(pool);

    let overweight = api
        .post("/api/v1/shipments", Some(shipment_body(50_000)))
        .await;
    assert_eq!(overweight.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(&overweight), "invalid_parcel");

    let mut bad_phone = shipment_body(500);
    bad_phone["recipient"]["phone"] = json!("0901234567");
    let reply = api.post("/api/v1/shipments", Some(bad_phone)).await;
    assert_eq!(error_code(&reply), "invalid_phone_number");

    let mut same_place = shipment_body(500);
    same_place["dropoff"] = same_place["pickup"].clone();
    let reply = api.post("/api/v1/shipments", Some(same_place)).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(&reply), "unsupported_route");

    let mut bad_coordinates = shipment_body(500);
    bad_coordinates["pickup"]["latitude"] = json!(123.0);
    let reply = api.post("/api/v1/shipments", Some(bad_coordinates)).await;
    assert_eq!(error_code(&reply), "invalid_coordinates");

    let missing_fields = api
        .post(
            "/api/v1/shipments",
            Some(json!({ "customer_id": Uuid::now_v7() })),
        )
        .await;
    assert!(missing_fields.status.is_client_error());
    assert_eq!(error_code(&missing_fields), "invalid_request");

    let bad_id = api.get("/api/v1/shipments/not-a-uuid").await;
    assert_eq!(bad_id.status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&bad_id), "invalid_request");

    let bad_filter = api.get("/api/v1/shipments?status=flying").await;
    assert_eq!(bad_filter.status, StatusCode::BAD_REQUEST);

    // Nothing was stored along the way.
    assert_eq!(api.get("/api/v1/shipments").await.body["total"], 0);
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn unknown_resources_are_404(pool: PgPool) {
    let api = Api::new(pool);

    let missing = api
        .get(&format!("/api/v1/shipments/{}", Uuid::now_v7()))
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&missing), "shipment_not_found");

    let dispatch = api.dispatch(&Uuid::now_v7().to_string()).await;
    assert_eq!(dispatch.status, StatusCode::NOT_FOUND);

    let unknown_tracking = api.get("/api/v1/tracking/DRN-000000000000").await;
    assert_eq!(unknown_tracking.status, StatusCode::NOT_FOUND);

    let malformed_tracking = api.get("/api/v1/tracking/abc").await;
    assert_eq!(malformed_tracking.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(&malformed_tracking), "invalid_tracking_number");

    let no_route = api.get("/api/v1/nope").await;
    assert_eq!(no_route.status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&no_route), "not_found");
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn lifecycle_rules_are_enforced(pool: PgPool) {
    let api = Api::new(pool);
    let id = api.request_shipment(500).await;

    // Cannot pick up or deliver before a drone is assigned.
    let early_pickup = api
        .post(&format!("/api/v1/shipments/{id}/pickup"), None)
        .await;
    assert_eq!(early_pickup.status, StatusCode::CONFLICT);
    assert_eq!(error_code(&early_pickup), "invalid_state_transition");

    assert_eq!(api.dispatch(&id).await.status, StatusCode::OK);
    let again = api.dispatch(&id).await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(error_code(&again), "invalid_state_transition");

    // Once the drone has the parcel the customer can no longer cancel.
    api.post(&format!("/api/v1/shipments/{id}/pickup"), None)
        .await;
    let cancel = api
        .post(&format!("/api/v1/shipments/{id}/cancel"), None)
        .await;
    assert_eq!(cancel.status, StatusCode::CONFLICT);

    // ... but the drone can report the failure.
    let blank = api
        .post(
            &format!("/api/v1/shipments/{id}/delivery-failure"),
            Some(json!({ "reason": "  " })),
        )
        .await;
    assert_eq!(blank.status, StatusCode::UNPROCESSABLE_ENTITY);

    let failed = api
        .post(
            &format!("/api/v1/shipments/{id}/delivery-failure"),
            Some(json!({ "reason": "Recipient unreachable" })),
        )
        .await;
    assert_eq!(failed.status, StatusCode::OK);
    assert_eq!(failed.body["status"], "failed");
    assert_eq!(failed.body["history"][3]["note"], "Recipient unreachable");
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn cancelling_frees_the_drone_for_the_next_shipment(pool: PgPool) {
    let api = Api::new(pool);

    // The stub fleet has three drones.
    let mut ids = Vec::new();
    for _ in 0..3 {
        let id = api.request_shipment(500).await;
        assert_eq!(api.dispatch(&id).await.status, StatusCode::OK);
        ids.push(id);
    }

    let fourth = api.request_shipment(500).await;
    let exhausted = api.dispatch(&fourth).await;
    assert_eq!(exhausted.status, StatusCode::CONFLICT);
    assert_eq!(error_code(&exhausted), "no_drone_available");
    assert_eq!(
        api.get(&format!("/api/v1/shipments/{fourth}")).await.body["status"],
        "requested"
    );

    // Cancel with a reason, then retry the dispatch.
    let cancelled = api
        .post(
            &format!("/api/v1/shipments/{}/cancel", ids[0]),
            Some(json!({ "reason": "Changed my mind" })),
        )
        .await;
    assert_eq!(cancelled.status, StatusCode::OK);
    assert_eq!(cancelled.body["status"], "cancelled");
    assert_eq!(cancelled.body["history"][2]["note"], "Changed my mind");

    assert_eq!(api.dispatch(&fourth).await.status, StatusCode::OK);
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn heavy_parcels_only_get_drones_that_can_lift_them(pool: PgPool) {
    let api = Api::new(pool);

    // Only one stub drone lifts 4 kg.
    let first = api.request_shipment(4_000).await;
    let second = api.request_shipment(4_000).await;

    assert_eq!(api.dispatch(&first).await.status, StatusCode::OK);
    let reply = api.dispatch(&second).await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(error_code(&reply), "no_drone_available");
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn health_endpoints_report_liveness_and_database_readiness(pool: PgPool) {
    let api = Api::new(pool);

    assert_eq!(api.get("/health/live").await.status, StatusCode::OK);
    let ready = api.get("/health/ready").await;
    assert_eq!(ready.status, StatusCode::OK);
    assert_eq!(ready.body["status"], "ok");
}
