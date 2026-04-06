# P2P Transport Observability Runbook

## Purpose

This runbook covers phase-1 transport observability for clipboard and pairing failures.
The goal is to answer these questions directly from Seq without guessing:

1. Which listen addresses were active locally?
2. Which peer addresses were discovered?
3. Which address was actually dialed for this attempt?
4. Did this attempt reuse an existing connection or require a new dial?
5. Did the failure look like stale addresses, listener instability, or plain network unreachability?

## New Event Keys

Phase 1 adds or standardizes these event fields in Rust transport logs:

- `event="network.listen_addresses_selected"`
- `event="network.listen_registered"`
- `event="network.listen_register_failed"`
- `event="network.new_listen_addr"`
- `event="peer.mdns_discovered"`
- `event="peer.mdns_expired"`
- `event="peer.connection_established"`
- `event="peer.connection_closed"`
- `event="peer.outgoing_connection_error"`
- `event="peer.incoming_connection_error"`
- `event="business_stream.open_attempt"`
- `event="business_stream.open_failed"`
- `event="business_stream.open_timeout"`
- `event="business_stream.ensure_open_failed"`
- `event="business_stream.ensure_open_timeout"`
- `event="pairing_stream.open_attempt"`
- `event="pairing_stream.open_skipped"`
- `event="pairing_stream.open_succeeded"`
- `event="pairing_stream.open_failed"`
- `event="pairing_stream.open_timeout"`
- `event="pairing_stream.session_started"`
- `event="pairing_stream.closed"`
- `event="pairing_stream.ended"`
- `event="pairing_stream.ended_with_error"`
- `event="clipboard.outbound_peer_evaluated"`
- `event="clipboard.outbound_attempt"`
- `event="clipboard.outbound_payload_encrypted"`
- `event="clipboard.outbound_business_path_failed"`
- `event="clipboard.outbound_send_failed"`
- `event="clipboard.outbound_partial_failure"`
- `event="clipboard.outbound_partial_success"`
- `event="clipboard.outbound_success"`

## Repro Checklist

When reproducing a transport issue, capture this context before and after the action:

1. Both devices' `peer_id`
2. Both devices' active network interfaces and IPs
3. Whether either device has VPN / proxy / guest-network isolation enabled
4. Whether both OS firewalls explicitly allow UniClipboard
5. Whether the action reused an existing session or required a fresh reconnect

## Seq Query Templates

Replace placeholders before use.

### 1. Find one outbound clipboard attempt, then pivot to its trace

```text
event = 'clipboard.outbound_attempt' and first_target_peer_id = 'PEER_ID'
```

Open the matching outbound attempt in the relevant time window, then reuse its `@TraceId`:

```text
@TraceId = 'TRACE_ID'
```

Expected useful events:

- `clipboard.outbound_attempt`
- `clipboard.outbound_payload_encrypted`
- `clipboard.outbound_business_path_failed`
- `clipboard.outbound_send_failed`
- `clipboard.outbound_partial_failure`
- `clipboard.outbound_success`

### 2. Transport attempts for one peer

```text
peer_id = 'PEER_ID' and (event like 'business_stream.%' or event like 'peer.%')
```

Use this to correlate:

- discovered addresses
- outgoing connection errors
- connection established / closed
- business stream open failures

### 3. Listen / bind health on one device

```text
event like 'network.listen%' or event = 'network.new_listen_addr'
```

Look for:

- bind failures
- missing `new_listen_addr`
- repeated rebinds
- `Address already in use`

### 4. Pairing transport for one peer or session

```text
(peer_id = 'PEER_ID' or session_id = 'SESSION_ID') and event like 'pairing_stream.%'
```

Use this to answer:

- was transport open skipped because the pairing session already existed
- did pairing stream open on a reused connection or a fresh dial
- which candidate addresses were available at open failure / timeout time
- which address was inferred as the chosen dial target
- who initiated the close and how the session ended

Useful fields:

- `skip_reason`
- `dial_decision`
- `candidate_address_count`
- `preferred_candidate_transport`
- `chosen_dial_addr`
- `chosen_dial_addr_resolution`
- `candidate_addresses`
- `dial_attempt_addresses`
- `last_dial_outcome`
- `close_initiator`
- `end_reason`
- `completion_source`

### 5. Suspected stale address

```text
(peer_id = 'PEER_ID' and event in ['business_stream.open_attempt', 'pairing_stream.open_attempt'])
```

Inspect these fields:

- `dial_decision`
- `candidate_address_count`
- `preferred_candidate_transport`
- `peer_marked_reachable`
- `connected_age_ms`
- `discovered_age_ms`
- `last_seen_age_ms`

Then compare with the failure event for the same `@TraceId` or `peer_id`:

- `chosen_dial_addr`
- `chosen_dial_addr_resolution`
- `dial_attempt_addresses`
- `candidate_addresses`
- `error`
- `timeout_ms`

If failures cluster around old `last_seen_age_ms` or old `discovered_age_ms`, stale discovery is the likely first suspect.

## What “Good” Looks Like

For a healthy send path, Seq should let us read the chain in order:

1. `peer.mdns_discovered`
2. `peer.connection_established`
3. `clipboard.outbound_attempt`
4. `business_stream.open_attempt`
5. `clipboard.outbound_success`

For a healthy pairing path, Seq should let us read the chain in order:

1. `peer.mdns_discovered`
2. `pairing_stream.open_attempt`
3. `pairing_stream.open_succeeded`
4. `pairing.handle_request` / `pairing.handle_challenge` / `pairing.handle_response`
5. `pairing_stream.ended`

For a broken path, we should still be able to identify which category it belongs to:

- stale peer addresses
- wrong NIC / wrong subnet preference
- listener bind conflict
- transport refused / timeout

## Out Of Scope

Phase 1 does not change:

- address ranking / bad-address backoff
- fresh discovery refresh policy
- relay / hole punching / rendezvous
- business logic for clipboard payload handling
