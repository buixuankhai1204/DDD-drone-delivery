# DDD Drone Delivery – Shipping service

REST API for the **Shipping** bounded context of a drone-delivery platform,
built with Rust, axum and PostgreSQL using a layered / hexagonal DDD
architecture.

Customers request a parcel delivery; the service finds and reserves a drone,
plans the flight, tracks the shipment until it is delivered and publishes
domain events for the rest of the platform. Everything outside Shipping (Fleet
Management, Flight Planning) is reached through **ports**, implemented as HTTP
clients, and can be replaced by in-process stubs for local work.

## Architecture

```
            ┌────────────────────────────────────────────────────────┐
 HTTP ───►  │ shipping-api            axum routes, extractors, errors│
            │   (interface layer + composition root)                 │
            └──────────────┬─────────────────────────────────────────┘
                           │ calls
            ┌──────────────▼─────────────────────────────────────────┐
            │ shipping-application    use cases, DTOs, outbound ports│
            │   FleetGateway · RoutePlanner · EventPublisher         │
            └──────────────┬─────────────────────────────────────────┘
                           │ uses
            ┌──────────────▼─────────────────────────────────────────┐
            │ shipping-domain         Shipment aggregate, value      │
            │   objects, events, DispatchPolicy, ShipmentRepository  │
            └──────────────▲─────────────────────────────────────────┘
                           │ implements the ports of the layers above
            ┌──────────────┴─────────────────────────────────────────┐
            │ shipping-infrastructure PostgreSQL repository,         │
            │   HTTP gateways, stubs, outbox relay                   │
            └────────────────────────────────────────────────────────┘
```

Each layer is its own crate, so the dependency rule is enforced by the
compiler: `domain` depends on nothing, `application` only on `domain`,
`infrastructure` on both, and only `api` knows all of them.

| Crate | Contents |
|-------|----------|
| [`shipping-domain`](crates/shipping-domain) | `Shipment` aggregate, value objects (`GeoPoint`, `Location`, `Parcel`, `Recipient`, `RoutePlan`, `TrackingNumber`, typed ids), `ShipmentEvent`s, `DispatchPolicy` domain service, `ShipmentRepository` trait |
| [`shipping-application`](crates/shipping-application) | one struct per use case, command/view DTOs, ports `FleetGateway`, `RoutePlanner`, `EventPublisher` |
| [`shipping-infrastructure`](crates/shipping-infrastructure) | `PgShipmentRepository` (sqlx), `HttpFleetGateway`, `HttpRoutePlanner`, stub gateways, `OutboxRelay`, migrations |
| [`shipping-api`](crates/shipping-api) | axum router, handlers, JSON error mapping, configuration, `main` |

## Domain model

`Shipment` is the aggregate root. All changes go through its methods, which
enforce the lifecycle and record domain events.

```
Requested ──► Scheduled ──► InTransit ──► Delivered
    │             │             │
    └──► Cancelled ◄┘          └──► Failed
```

Invariants (all enforced in the domain layer):

* parcel: 1 g – 5 kg, each side ≤ 500 mm, description required
* recipient phone in international format (`+84901234567`)
* pickup and dropoff between 50 m and 25 km apart
* a drone is only assigned if it can carry the parcel and fly the planned
  route (`DispatchPolicy`)
* cancelling is only possible before the drone took off with the parcel;
  delivery and failure only after pickup; terminal states are final

Events (`shipment.requested`, `.scheduled`, `.picked_up`, `.delivered`,
`.delivery_failed`, `.cancelled`) are stored in an **outbox** table in the same
transaction as the aggregate and relayed afterwards (at-least-once, ordered).
The default publisher only logs them; implement `EventPublisher` for your
broker to ship them elsewhere.

## Use cases and endpoints

Base path `/api/v1`. Errors always look like
`{"error": {"code": "...", "message": "..."}}`.

| Use case | Endpoint | Caller | Success | Typical errors |
|----------|----------|--------|---------|----------------|
| `RequestShipment` | `POST /shipments` | customer | `201` + `Location` | `422` invalid parcel / phone / coordinates / route |
| `DispatchShipment` | `POST /shipments/{id}/dispatch` | dispatcher / scheduler | `200` | `404`, `409 no_drone_available`, `409 invalid_state_transition`, `422 route_unavailable`, `503 dependency_unavailable` |
| `CancelShipment` | `POST /shipments/{id}/cancel` body `{"reason"?}` | customer / operator | `200` | `404`, `409` once in transit |
| `RecordPickup` | `POST /shipments/{id}/pickup` | drone / Fleet | `200` | `404`, `409` |
| `RecordDelivery` | `POST /shipments/{id}/delivery` | drone / Fleet | `200` | `404`, `409` |
| `ReportDeliveryFailure` | `POST /shipments/{id}/delivery-failure` body `{"reason"}` | drone / Fleet | `200` | `404`, `409`, `422` |
| `GetShipment` | `GET /shipments/{id}` | customer / operator | `200` | `404` |
| `ListShipments` | `GET /shipments?customer_id=&status=&limit=&offset=` | customer / operator | `200` paged | `400` |
| `TrackShipment` | `GET /tracking/{tracking_number}` | anyone with the number | `200`, no personal data | `404`, `422` |
| – | `GET /health/live`, `GET /health/ready` | probes | `200` | `503` if DB is down |

Dispatch is its own step so an operator or an automatic scheduler can drive it;
reacting to `shipment.requested` and dispatching automatically is a natural
next step.

### Example

```bash
curl -s -X POST localhost:8080/api/v1/shipments -H 'content-type: application/json' -d '{
  "customer_id": "0192a000-0000-7000-8000-000000000001",
  "recipient": { "name": "Nguyen Van A", "phone": "+84901234567" },
  "parcel": { "weight_grams": 1500, "length_mm": 200, "width_mm": 150, "height_mm": 100,
              "description": "Phone case", "fragile": true },
  "pickup":  { "latitude": 21.0285, "longitude": 105.8542, "address": "Hoan Kiem, Hanoi" },
  "dropoff": { "latitude": 21.0380, "longitude": 105.8340, "address": "Ba Dinh, Hanoi" }
}'
# → 201 {"id": "...", "tracking_number": "DRN-FB013FA58282", "status": "requested", ...}

curl -s -X POST localhost:8080/api/v1/shipments/<id>/dispatch   # → "scheduled", drone + ETA
curl -s localhost:8080/api/v1/tracking/DRN-FB013FA58282         # public timeline
curl -s -X POST localhost:8080/api/v1/shipments/<id>/pickup     # drone → "in_transit"
curl -s -X POST localhost:8080/api/v1/shipments/<id>/delivery   # → "delivered"
```

## Other bounded contexts

Shipping depends on two contexts through ports in `shipping-application`.
The HTTP adapters map their wire format to Shipping's own model (anti-corruption
layer); **the contracts below are assumptions** – adjust the adapters in
[`gateways/`](crates/shipping-infrastructure/src/gateways) to the real APIs.

**Fleet Management** (`FLEET_SERVICE_URL`)

| Call | Expected answer |
|------|-----------------|
| `GET /api/v1/drones/available?payload_grams&latitude&longitude&limit` | `200 {"drones":[{"id","max_payload_grams","max_range_m"}]}`, best candidate first |
| `PUT /api/v1/drones/{drone_id}/reservations/{shipment_id}` | `2xx`; `409` when the drone was taken; idempotent per shipment |
| `DELETE /api/v1/drones/{drone_id}/reservations/{shipment_id}` | `2xx`; `404` is treated as already released |

**Flight Planning** (`FLIGHT_PLANNING_URL`)

| Call | Expected answer |
|------|-----------------|
| `POST /api/v1/routes/plan` `{"drone_id","pickup":{"latitude","longitude"},"dropoff":{...}}` | `200 {"distance_m","duration_secs"}`; `422` when no flyable route exists |

4xx answers become *rejections* (business "no"), network errors, timeouts,
5xx, 408 and 429 become *unavailability* (HTTP `503` towards our caller).
Clients use a 2 s connect / 5 s total timeout and do not retry.

For local development set `EXTERNAL_SERVICES=stub`: an in-process fleet of
three drones (2 kg / 3 kg / 5 kg payload) and a straight-line route planner.

## Running it

Requirements: Rust (edition 2024), Docker for PostgreSQL.

```bash
docker compose up -d db           # PostgreSQL 16 on localhost:5432
cp .env.example .env              # stub mode, local database
cargo run -p shipping-api         # migrations run on start-up
```

Configuration (environment variables, `.env` is loaded if present):

| Variable | Default | |
|----------|---------|---|
| `DATABASE_URL` | – | required |
| `BIND_ADDR` | `0.0.0.0:8080` | |
| `DB_MAX_CONNECTIONS` | `10` | |
| `EXTERNAL_SERVICES` | `http` | `http` or `stub` |
| `FLEET_SERVICE_URL`, `FLIGHT_PLANNING_URL` | – | required when `EXTERNAL_SERVICES=http` |
| `RUST_LOG` | `info,sqlx=warn,tower_http=info` | |

## Design notes and trade-offs

* **Optimistic locking** – `shipments.version` is checked on every update; a
  concurrent change surfaces as `409 concurrent_modification`.
* **Dispatch across contexts** – there is no distributed transaction. The
  steps are ordered so nothing needs undoing before the drone is reserved; if
  saving the shipment fails afterwards the reservation is released (unless a
  concurrent dispatch won with the same drone). Up to three candidate drones
  are tried before giving up.
* **Releasing drones on cancel / delivery / failure is best effort** – the
  shipment state is the source of truth; if Fleet is down the call is logged
  and Fleet can release on its own from the published events.
* **Read model** – listing and tracking read the same table through the same
  repository. Move them to dedicated queries if read load grows.
* **Tracking history** is part of the aggregate and stored as `JSONB`.
* **No authentication** – assumed to be handled by an API gateway in front of
  the service; the drone-facing endpoints (`pickup`, `delivery`,
  `delivery-failure`) in particular must not be exposed publicly.
* **Not included** – pricing/billing, notifications, authentication, an
  OpenAPI document, a real message broker adapter.
