# Chatmail Notification Proxy

The Chatmail notification proxy is deployed as a central service
on https://notifications.delta.chat 

The notification proxy is a small Rust program 
that forwards "device tokens" to Apple and Google "Push Services"
that in turn wake up the clients
using [Chatmail core](https://github.com/chatmail/core/) on user's devices.

## Usage 

### Certificates

The certificate file provided must be a `.p12` file. Instructions for how to create can be found [here](https://stackoverflow.com/a/28962937/1358405).

### Running

```sh
$ cargo build --release
$ ./target/release/notifiers --certificate-file <file.p12> --password <password>
```

### Registering devices

```sh
$ curl -X POST -d '{ "token": "<device token>" }' http://localhost:9000/register
```

### Enabling metrics

To enable OpenMetrics (Prometheus) metrics endpoint,
run with `--metrics` argument,
e.g. `--metrics 127.0.0.1:9001`.
Metrics can then be retrieved with
`curl http://127.0.0.1:9001/metrics`.


## DRAFT [NEEDS REVIEW]: How notifications flow from core to relay to push proxy

In the **chatmail** architecture, push notifications follow a highly specialized path 
designed to minimize battery drain and maximize privacy.
By leveraging Dovecot's **dictproxy** protocol 
and a dedicated **notifiers** service with debouncing logic, 
chatmail avoids the need for a persistent IMAP connection.

---

### 1. `chatmail/core`: The IMAP Client Logic

The **chatmail core** is the Rust-based engine used by the mobile and desktop clients.
It initiates the registration process via the IMAP protocol.

* **Capability Discovery:**
The core identifies compatible servers by looking for the `XCHATMAIL` or `XDELTAPUSH` strings in the IMAP `CAPABILITY` response.
* **Location:** `src/capabilities.rs`


* **Token Registration:**
When the mobile OS provides a device token (FCM or APNs), 
the core uploads it using the **IMAP SETMETADATA** command.
* **Key:** `/private/vendor/deltachat/push-token`
* **Location:** `src/imap/metadata.rs`


* **Push Orchestration:**
The core tracks the token's lifecycle.
If the token is updated or the account is newly configured, 
this module flags the IMAP worker to sync the metadata.
* **Location:** `src/push.rs`



---

### 2. `chatmail/relay`: The Metadata Service & DictProxy

The **relay** repository handles the server-side delegation of metadata storage and the triggering of notifications.

* **`chatmaild/src/chatmaild/metadata.py` (The DictProxy Interface):**
This script is the backend for Dovecot’s **dictproxy**.
Dovecot is configured to proxy all metadata requests to this service over a Unix socket.
* **Command Handling:** It implements the line-based `dictproxy` protocol.
* **The `S` (Set) Command:** When the core sends an IMAP `SETMETADATA` command, Dovecot translates it into an `S` command.
The script then parses the `push-token` key and stores the opaque token in a local SQLite database.


* **`chatmaild/src/chatmaild/notifier.py` (The Trigger):**
This service reacts to new mail arrival events.
* **Logic:** It queries the metadata database for the recipient’s token.
* **Transmission:** It queues the token in a `PriorityQueue` and uses `NotifyThreads` to POST the token to the central notification proxy.
* **Location:** `chatmaild/src/chatmaild/notifier.py`



---

### 3. `chatmail/notifiers`: The Debouncing Proxy

The **notifiers** repository is a standalone Rust service deployed at `notifications.delta.chat`.
It acts as the final gateway to Apple (APNs) and Google (FCM).

* **Debouncing Logic:**
To protect battery life, this service implements a "coalescing" or debouncing window.
* **Location:** `src/server.rs` (Search for `feat: debounce notifications`)
* **The Window:** When a request arrives, the server checks if a timer (typically **10 seconds**) is already running for that specific `device_token`.
* **The Action:** If a timer is active, subsequent notification requests for the same token are ignored during that window.
Only **one** push "tickle" is sent to the phone at the end of the period.


* **Performance Benefit:**
Since the mobile app's **Core** fetches *all* pending messages upon waking up, a single push is sufficient even if 50 emails arrived.
* **Rate Limiting:**
This module also prevents the server from exceeding the strict rate limits imposed by Apple and Google.
* **Location:** `src/rate_limiter.rs`



---

### Summary of the Push Flow

| Stage | Component | Protocol / Action |
| --- | --- | --- |
| **Registration** | `chatmail/core` | `IMAP SETMETADATA` |
| **Delegation** | **Dovecot** | **`dictproxy` protocol (`S` command)** |
| **Bridge** | `chatmail/relay` | `metadata.py` stores token in SQLite |
| **Trigger** | `chatmail/relay` | `notifier.py` POSTs to proxy |
| **Debounce** | `chatmail/notifiers` | 10s Coalescing window |
| **Delivery** | `chatmail/notifiers` | APNs / FCM "Tickle" |
