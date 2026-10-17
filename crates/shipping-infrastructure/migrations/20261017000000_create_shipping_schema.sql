-- Shipping bounded context: write model of the Shipment aggregate + outbox.

CREATE TABLE shipments (
    id                    UUID PRIMARY KEY,
    tracking_number       TEXT        NOT NULL UNIQUE,
    customer_id           UUID        NOT NULL,
    status                TEXT        NOT NULL
        CHECK (status IN ('requested', 'scheduled', 'in_transit', 'delivered', 'cancelled', 'failed')),

    recipient_name        TEXT        NOT NULL,
    recipient_phone       TEXT        NOT NULL,

    parcel_weight_grams   INTEGER     NOT NULL CHECK (parcel_weight_grams > 0),
    parcel_length_mm      INTEGER     NOT NULL CHECK (parcel_length_mm > 0),
    parcel_width_mm       INTEGER     NOT NULL CHECK (parcel_width_mm > 0),
    parcel_height_mm      INTEGER     NOT NULL CHECK (parcel_height_mm > 0),
    parcel_description    TEXT        NOT NULL,
    parcel_fragile        BOOLEAN     NOT NULL,

    pickup_latitude       DOUBLE PRECISION NOT NULL,
    pickup_longitude      DOUBLE PRECISION NOT NULL,
    pickup_address        TEXT        NOT NULL,
    dropoff_latitude      DOUBLE PRECISION NOT NULL,
    dropoff_longitude     DOUBLE PRECISION NOT NULL,
    dropoff_address       TEXT        NOT NULL,

    -- Drone reference (owned by the Fleet context, hence no foreign key).
    drone_id              UUID,
    assigned_at           TIMESTAMPTZ,
    route_distance_m      INTEGER,
    route_duration_secs   INTEGER,
    estimated_delivery_at TIMESTAMPTZ,

    -- Customer-visible timeline, owned by the aggregate.
    history               JSONB       NOT NULL DEFAULT '[]'::jsonb,

    requested_at          TIMESTAMPTZ NOT NULL,
    updated_at            TIMESTAMPTZ NOT NULL,
    -- Optimistic locking.
    version               BIGINT      NOT NULL,

    CONSTRAINT drone_assignment_is_complete CHECK (
        (drone_id IS NULL AND assigned_at IS NULL AND route_distance_m IS NULL
            AND route_duration_secs IS NULL AND estimated_delivery_at IS NULL)
        OR
        (drone_id IS NOT NULL AND assigned_at IS NOT NULL AND route_distance_m IS NOT NULL
            AND route_duration_secs IS NOT NULL AND estimated_delivery_at IS NOT NULL)
    )
);

CREATE INDEX shipments_customer_idx ON shipments (customer_id, requested_at DESC);
CREATE INDEX shipments_status_idx   ON shipments (status, requested_at DESC);
CREATE INDEX shipments_drone_idx    ON shipments (drone_id) WHERE drone_id IS NOT NULL;

-- Transactional outbox: domain events are written in the same transaction as
-- the aggregate and relayed to the outside world afterwards.
CREATE TABLE outbox_events (
    seq          BIGSERIAL PRIMARY KEY,
    id           UUID        NOT NULL UNIQUE,
    aggregate_id UUID        NOT NULL,
    event_type   TEXT        NOT NULL,
    payload      JSONB       NOT NULL,
    occurred_at  TIMESTAMPTZ NOT NULL,
    published_at TIMESTAMPTZ
);

CREATE INDEX outbox_events_unpublished_idx ON outbox_events (seq) WHERE published_at IS NULL;
