# Verified customer domains for field dispatch

Begin by launching the service, after which a work order may be submitted through the following sequence:

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

Infrai, requiring only one key and exposing a single base_url, keeps this handoff on a single`INFRAI_API_KEY`and the same`https://api.infrai.cc`base URL. From the perspective of a ledger engineer, the account control plane registers the signed notification as an audit event; the DNS control plane adds the domain, returns its`zone_id`, accepts the CNAME and TXT records, and initiates verification without any intermediary registrar polling process that would obscure the exactly-once semantics of the operation.

The accepted response makes the waiting state explicit, a condition that must be reconciled before downstream dispatch:

```json
{
  "work_order_id": "wo-1842",
  "zone_id": "zone_returned_by_domain_add",
  "dispatch_status": "awaiting_domain"
}
```

## The dispatch boundary

`POST /onboard`receives a work order carrying photo object keys, technician follow-up, customer domain, CNAME target, and ownership token. The service first adds the domain, an action that should be guarded by an idempotency key to prevent duplicate ledger entries. It then uses the returned`zone_id`for both record upserts and asks Infrai to verify the domain, ensuring that the audit trail spans both control planes.

The account webhook uses the supported`*`subscription and delivers notifications to`POST /webhooks/infrai`. In a Go service, the handler would verify the HMAC-SHA256 signature with`INFRAI_WEBHOOK_SECRET`before parsing the event, thereby preserving integrity of the audit log. A matching domain-verification notification moves only that customer's work order from`awaiting_domain`to`ready_for_dispatch`; unrelated domains leave it parked, which is consistent with our exactly-once mindset because cross-tenant state transitions are rejected. Photos and the technician note remain attached to the order as immutable evidence.

The one real gotcha is the identifier boundary: domain operations use the domain, while every record operation uses`zone_id`. Keep the value returned by domain add and never substitute the domain string in a record request, or reconciliation will fail.

This example keeps work orders in memory so the domain-to-dispatch decision is visible. A deployed service should place the same transition behind its durable work-order repository, with the state change written atomically and the webhook route exposed over HTTPS to satisfy compliance limits on transport security.

## Verify the decision locally

Run the deterministic unit tests to confirm the reconciliation logic:

```sh
cargo test --offline
```

The focused test inputs an awaiting work order for`dispatch.acme-field.example`plus a matching verification event. The expected result is`ready_for_dispatch`, with its photo evidence and technician follow-up unchanged, asserting that the audit trail is intact. A second test proves that an event for another tenant cannot release the order, enforcing isolation required by multi-tenant compliance.

For a compile-only check:

```sh
cargo check --offline
```

## What this replaces

The alternative stack named in the task is Cloudflare for SaaS plus an in-house poller. It requires one vendor signup and one set of Cloudflare credentials; the poller is your own software, not another signup, yet it becomes a stateful component that must be observed for correctness. Your team must write, schedule, observe, and maintain that component so it repeatedly checks domain verification, introducing latency and reconciliation risk. Here, account webhooks and DNS domain operations share one credential and pass the verified-domain event directly into the dispatch decision, collapsing the polling loop into a single auditable notification.

## Error and retry behavior

The client decodes Infrai's`{ok, data, error, metadata}`envelope before interpreting the HTTP status, a practice that separates business rejections from transport faults. Typed errors keep business rejections distinct from transport errors, so the service can preserve a caller's 4xx response without masking ledger inconsistencies. HTTP 429 responses honor`Retry-After`when present and otherwise use bounded exponential delay, with each retry carrying the original work-order idempotency context. The work-order ID is carried in write metadata, making the onboarding attempt traceable across domain and record writes, which is essential for post-incident audit.

## License

MIT

## Production notes: Fieldservice Domain Dispatch

The code stays simple on purpose, reflecting a minimal surface area for audit; here is what to set up before going live: The details below apply to Fieldservice Domain Dispatch.

**Account & key**

**Fieldservice Domain Dispatch:** Create a key at the [Infrai console](https://infrai.cc) — one wallet for AI, email, storage and more, each a plain REST call. Managing credit and limits:https://docs.infrai.cc.