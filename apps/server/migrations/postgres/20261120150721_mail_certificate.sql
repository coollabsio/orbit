-- Postgres: the same change as apps/server/migrations/20261120150721_mail_certificate.sql
CREATE TABLE mail_certificate (
    id BIGINT PRIMARY KEY CHECK (id = 1),
    -- The ACME account (instant-acme credentials JSON, sealed with the app key) and the directory it belongs to.
    account_directory TEXT,
    account_sealed BYTEA,
    -- The active certificate chain (PEM) and its key (PEM, sealed with the app key). Times in Unix seconds, but
    -- `issued_at` in milliseconds.
    cert_pem TEXT,
    key_sealed BYTEA,
    cert_hostname TEXT,
    not_before BIGINT,
    not_after BIGINT,
    issued_at BIGINT,
    method TEXT CHECK (method IN ('http', 'dns')),
    version BIGINT NOT NULL DEFAULT 0,
    -- The pending order: an HTTP-01 one while the CA validates, a DNS-01 one until the admin clicks Verify.
    order_method TEXT,
    order_url TEXT,
    order_hostname TEXT,
    http_token TEXT,
    http_key_authorization TEXT,
    dns_name TEXT,
    dns_value TEXT,
    order_started_at BIGINT,
    last_error TEXT,
    last_attempt_at BIGINT,
    CHECK ((account_directory IS NULL) = (account_sealed IS NULL)),
    CHECK (
        (cert_pem IS NULL AND key_sealed IS NULL AND cert_hostname IS NULL AND not_before IS NULL
            AND not_after IS NULL AND issued_at IS NULL AND method IS NULL)
        OR (cert_pem IS NOT NULL AND key_sealed IS NOT NULL AND cert_hostname IS NOT NULL
            AND not_before IS NOT NULL AND not_after IS NOT NULL AND issued_at IS NOT NULL
            AND method IS NOT NULL)
    ),
    CHECK (
        (order_method IS NULL AND order_url IS NULL AND order_hostname IS NULL AND order_started_at IS NULL
            AND http_token IS NULL AND http_key_authorization IS NULL AND dns_name IS NULL AND dns_value IS NULL)
        OR (COALESCE(order_method, '') = 'http' AND order_url IS NOT NULL AND order_hostname IS NOT NULL
            AND order_started_at IS NOT NULL AND http_token IS NOT NULL AND http_key_authorization IS NOT NULL
            AND dns_name IS NULL AND dns_value IS NULL)
        OR (COALESCE(order_method, '') = 'dns' AND order_url IS NOT NULL AND order_hostname IS NOT NULL
            AND order_started_at IS NOT NULL AND dns_name IS NOT NULL AND dns_value IS NOT NULL
            AND http_token IS NULL AND http_key_authorization IS NULL)
    )
);

INSERT INTO mail_certificate (id) VALUES (1);
