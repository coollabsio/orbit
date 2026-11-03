#!/usr/bin/env bash
# Creates the Orbit Cloud product, its prices and the Billing Portal in Stripe, then prints
# ORBIT_STRIPE_PRICES.
# Run it once with a test key and once with the live key: each run finds what exists by product id
# and price lookup key, so running it again creates nothing new. It renames products whose name
# differs, since Orbit shows the product name on the billing page.
#
#   STRIPE_SECRET_KEY=sk_test_… scripts/stripe-setup.sh
#   STRIPE_SECRET_KEY=sk_live_… ORBIT_WEBHOOK_URL=https://<origin>/api/v1/billing/stripe/webhook scripts/stripe-setup.sh
set -euo pipefail

: "${STRIPE_SECRET_KEY:?set STRIPE_SECRET_KEY}"
api=https://api.stripe.com/v1

stripe() {
    local method=$1 path=$2
    shift 2
    curl -sS -X "$method" -u "$STRIPE_SECRET_KEY:" "$api$path" "$@"
}

# plan key, product id, product name, amount in cents, interval. The product gets the Stripe Tax code
# "SaaS - personal use" (txcd_10103001).
plans=(
    "monthly orbit Orbit 1000 month"
    "annual orbit Orbit 10000 year"
)

prices=()
for line in "${plans[@]}"; do
    read -r key product name amount interval <<<"$line"
    existing=$(stripe GET "/products/$product")
    if jq -e '.error' >/dev/null <<<"$existing"; then
        stripe POST /products -d id="$product" -d name="$name" -d tax_code=txcd_10103001 \
            | jq -er '.id' >/dev/null
        echo "created product $product" >&2
    elif [ "$(jq -r '.name' <<<"$existing")" != "$name" ]; then
        stripe POST "/products/$product" -d name="$name" | jq -er '.id' >/dev/null
        echo "renamed product $product to $name" >&2
    fi
    lookup="orbit_${key//-/_}"
    price=$(stripe GET /prices -G -d "lookup_keys[]=$lookup" -d active=true | jq -r '.data[0].id // empty')
    if [ -z "$price" ]; then
        price=$(stripe POST /prices -d product="$product" -d currency=usd -d unit_amount="$amount" \
            -d "recurring[interval]=$interval" -d lookup_key="$lookup" | jq -er '.id')
        echo "created price $lookup ($price)" >&2
    fi
    prices+=("$key:$price")
done

# Orbit opens the Billing Portal without naming a configuration, so the account needs a default one.
if [ -z "$(stripe GET /billing_portal/configurations -G -d is_default=true | jq -r '.data[0].id // empty')" ]; then
    portal=(-d "features[customer_update][enabled]=true"
        -d "features[customer_update][allowed_updates][]=email"
        -d "features[customer_update][allowed_updates][]=address"
        -d "features[customer_update][allowed_updates][]=tax_id"
        -d "features[invoice_history][enabled]=true"
        -d "features[payment_method_update][enabled]=true"
        -d "features[subscription_cancel][enabled]=true"
        -d "features[subscription_cancel][mode]=at_period_end"
        -d "features[subscription_update][enabled]=true"
        -d "features[subscription_update][default_allowed_updates][]=price"
        -d "features[subscription_update][proration_behavior]=create_prorations"
        -d "features[subscription_update][products][0][product]=orbit")
    for entry in "${prices[@]}"; do
        portal+=(-d "features[subscription_update][products][0][prices][]=${entry#*:}")
    done
    stripe POST /billing_portal/configurations "${portal[@]}" | jq -er '.id' >/dev/null
    echo "created the default Billing Portal configuration" >&2
fi

if [ -n "${ORBIT_WEBHOOK_URL:-}" ]; then
    existing=$(stripe GET /webhook_endpoints -G -d limit=100 |
        jq -r --arg url "$ORBIT_WEBHOOK_URL" '.data[] | select(.url == $url) | .id' | head -1)
    if [ -n "$existing" ]; then
        echo "webhook $existing already exists; its signing secret is in the Stripe Dashboard" >&2
    else
        secret=$(stripe POST /webhook_endpoints -d url="$ORBIT_WEBHOOK_URL" \
            -d "enabled_events[]=checkout.session.completed" \
            -d "enabled_events[]=customer.subscription.created" \
            -d "enabled_events[]=customer.subscription.updated" \
            -d "enabled_events[]=customer.subscription.deleted" | jq -er '.secret')
        echo "ORBIT_STRIPE_WEBHOOK_SECRET=$secret"
    fi
fi

(IFS=,; echo "ORBIT_STRIPE_PRICES=${prices[*]}")
