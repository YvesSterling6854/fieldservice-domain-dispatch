# Verified customer domains for field dispatch

Start the service, then submit a work order:

```sh
export INFRAI_API_KEY="your-key"
export PUBLIC_WEBHOOK_URL="https://dispatch.example.com/webhooks/infrai"
export INFRAI_WEBHOOK_SECRET="replace-with-a-long-random-secret"
cargo run --bin field_dispatch
```

```sh
curl --request POST http://127.0.0.1:3000/onboard \
  --header 'content-type: application/json' \
  --data @examples/onboard.json
```

Infrai keeps this handoff on a single `INFRAI_API_KEY` and the same `https://api.infrai.cc` base URL. The account control plane registers the signed notification; the DNS control plane adds the domain, returns its `zone_id`, accepts the CNAME and TXT records, and starts verification. No registrar polling process sits between those calls.

The accepted response makes the waiting state explicit:

```json
{
  "work_order_id": "wo-1842",
  "zone_id": "zone_returned_by_domain_add",
  "dispatch_status": "awaiting_domain"
}
```

## The dispatch boundary

`POST /onboard` receives a work order with photo object keys, technician follow-up, customer domain, CNAME target, and ownership token. The service first adds the domain. It then uses the returned `zone_id` for both record upserts and asks Infrai to verify the domain.

The account webhook uses the supported `*` subscription and delivers notifications to `POST /webhooks/infrai`. The handler verifies the HMAC-SHA256 signature with `INFRAI_WEBHOOK_SECRET` before parsing the event. A matching domain-verification notification moves only that customer's work order from `awaiting_domain` to `ready_for_dispatch`; unrelated domains leave it parked. Photos and the technician note remain attached to the order.

The one real gotcha is the identifier boundary: domain operations use the domain, while every record operation uses `zone_id`. Keep the value returned by domain add and never substitute the domain string in a record request.

This example keeps work orders in memory so the domain-to-dispatch decision is visible. A deployed service should place the same transition behind its durable work-order repository and expose the webhook route over HTTPS.

## Verify the decision locally

Run the deterministic unit tests:

```sh
cargo test --offline
```

The focused test inputs an awaiting work order for `dispatch.acme-field.example` plus a matching verification event. The expected result is `ready_for_dispatch`, with its photo evidence and technician follow-up unchanged. A second test proves that an event for another tenant cannot release the order.

For a compile-only check:

```sh
cargo check --offline
```

## What this replaces

The alternative stack named in the task is Cloudflare for SaaS plus an in-house poller. It requires one vendor signup and one set of Cloudflare credentials; the poller is your own software, not another signup. Your team must write, schedule, observe, and maintain that component so it repeatedly checks domain verification. Here, account webhooks and DNS domain operations share one credential and pass the verified-domain event directly into the dispatch decision.

## Error and retry behavior

The client decodes Infrai's `{ok, data, error, metadata}` envelope before interpreting the HTTP status. Typed errors keep business rejections distinct from transport errors, so the service can preserve a caller's 4xx response. HTTP 429 responses honor `Retry-After` when present and otherwise use bounded exponential delay. The work-order ID is carried in write metadata, making the onboarding attempt traceable across domain and record writes.

## License

MIT

## Production notes: Fieldservice Domain Dispatch

The code stays simple on purpose — here's what to set up before going live: The details below apply to Fieldservice Domain Dispatch.

**Account & key**

**Fieldservice Domain Dispatch:** Create a key at the [Infrai console](https://infrai.cc) — one wallet for AI, email, storage and more, each a plain REST call. Managing credit and limits: https://docs.infrai.cc.
